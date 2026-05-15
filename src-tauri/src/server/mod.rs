use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use parking_lot::Mutex;
use serde::Serialize;
use sqlx::Row;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

use crate::cache::{derive_label_key, CacheManager};
use crate::cloud::CloudClient;
use crate::config::AppConfig;
use crate::db::DbPool;
use crate::models::{
    ApiErrorBody, DataEnvelope, ParcelData, ReportPayload, SuccessEnvelope,
};
use crate::queue::QueueManager;
use crate::{AppError, AppResult};

/// 每個物流商代碼下一次該分配的 channel 索引（round-robin）
type RoundRobinState = Arc<Mutex<HashMap<String, usize>>>;

#[derive(Clone)]
struct ServerState {
    db: DbPool,
    cloud: CloudClient,
    cache: CacheManager,
    queue: QueueManager,
    rr: RoundRobinState,
}

pub struct ServerHandle {
    pub bind_addr: String,
    handle: JoinHandle<()>,
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
}

impl ServerHandle {
    /// 主動關閉 HTTP server
    pub async fn shutdown(self) {
        let _ = self.shutdown_tx.send(());
        let _ = self.handle.await;
    }
}

/// 啟動 axum HTTP server，回傳一個可以關閉它的 handle
pub async fn start(
    config: &AppConfig,
    db: DbPool,
    cloud: CloudClient,
    cache: CacheManager,
    queue: QueueManager,
) -> AppResult<ServerHandle> {
    let addr: SocketAddr = format!("{}:{}", config.server.listen_ip, config.server.port)
        .parse()
        .map_err(|e| AppError::Server(format!("無法解析 listen 位址: {e}")))?;

    let state = ServerState {
        db,
        cloud,
        cache: cache.clone(),
        queue,
        rr: Arc::new(Mutex::new(HashMap::new())),
    };

    let images_service = ServeDir::new(cache.base_dir());

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/api/parcel/:query_no", get(get_parcel))
        .route("/api/report", post(post_report))
        .nest_service("/images", images_service)
        .with_state(state)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| AppError::Server(format!("無法綁定 {addr}: {e}")))?;

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let server_future = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = shutdown_rx.await;
        });

    let handle = tokio::spawn(async move {
        if let Err(e) = server_future.await {
            tracing::error!(?e, "HTTP server 結束");
        }
    });

    tracing::info!(%addr, "本地 HTTP server 已啟動");

    Ok(ServerHandle {
        bind_addr: addr.to_string(),
        handle,
        shutdown_tx,
    })
}

#[derive(Serialize)]
struct HealthData {
    name: &'static str,
}

async fn healthz() -> Json<SuccessEnvelope<HealthData>> {
    Json(SuccessEnvelope::ok(HealthData {
        name: "cix3752i-label-print",
    }))
}

fn err_resp(status: StatusCode, message: impl Into<String>) -> (StatusCode, Json<ApiErrorBody>) {
    (
        status,
        Json(ApiErrorBody {
            message: message.into(),
            status_code: status.as_u16(),
        }),
    )
}

/// GET /api/parcel/:query_no — 工控機呼叫
async fn get_parcel(
    State(state): State<ServerState>,
    Path(query_no): Path<String>,
) -> Result<Json<DataEnvelope<ParcelData>>, (StatusCode, Json<ApiErrorBody>)> {
    match state.cloud.fetch_parcel(&query_no).await {
        Ok(info) => {
            // 用 shipping_no 作為快取 key (檔名末段帶副檔名,從 shipping_image URL 推得)
            let label_key = derive_label_key(&info.shipping_image);

            // 本地沒快取就同步下載到完成,完成才回最新路徑給工控機
            let label_path = if state.cache.has_local(&label_key) {
                let _ = state.cache.record_hit(&label_key).await;
                Some(state.cache.local_path_for_key(&label_key).to_string_lossy().to_string())
            } else {
                let _ = state.cache.record_miss().await;
                match state.cache.fetch_now(&label_key, &info.shipping_image).await {
                    Ok(()) => Some(
                        state.cache.local_path_for_key(&label_key).to_string_lossy().to_string(),
                    ),
                    Err(e) => {
                        tracing::warn!(label_key = %label_key, ?e, "同步下載快取失敗");
                        None
                    }
                }
            };

            // 依雲端回的物流商代碼 (shipping_provider) 查所有對應分揀通道
            // 同物流商可能配多個通道,用 round-robin 輪流分配
            let rows = sqlx::query(
                "SELECT channel_code
                 FROM sort_channels
                 WHERE dispatch_code = ?
                   AND channel_code IS NOT NULL
                   AND channel_code <> ''
                 ORDER BY
                   CASE substr(position,1,1) WHEN 'L' THEN 0 WHEN 'R' THEN 1 ELSE 2 END,
                   CAST(substr(position,2) AS INTEGER)",
            )
            .bind(&info.shipping_provider)
            .fetch_all(&state.db)
            .await
            .unwrap_or_default();

            let codes: Vec<String> = rows
                .into_iter()
                .filter_map(|r| r.try_get::<Option<String>, _>("channel_code").ok().flatten())
                .collect();

            let channel_code: Option<String> = if codes.is_empty() {
                None
            } else {
                let mut rr = state.rr.lock();
                let entry = rr.entry(info.shipping_provider.clone()).or_insert(0);
                let idx = *entry % codes.len();
                *entry = (idx + 1) % codes.len();
                Some(codes[idx].clone())
            };

            // 從 dispatch_provider 取 print_profile (使用者在「指派物流」頁面設定)
            let print_profile: Option<String> = sqlx::query(
                "SELECT print_profile FROM dispatch_provider WHERE code = ?",
            )
            .bind(&info.shipping_provider)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten()
            .and_then(|r| r.try_get::<Option<String>, _>("print_profile").ok().flatten());

            // 紀錄這次查詢,POST /api/report 用 response_id 反查
            if let Some(rid) = info.response_id {
                let _ = sqlx::query(
                    "INSERT INTO parcel_query_log
                       (response_id, query_no, tracking_no, sort_channel, print_profile, should_print, label_key)
                     VALUES (?, ?, ?, ?, ?, ?, ?)
                     ON CONFLICT(response_id) DO UPDATE SET
                       query_no = excluded.query_no,
                       tracking_no = excluded.tracking_no,
                       sort_channel = excluded.sort_channel,
                       print_profile = excluded.print_profile,
                       should_print = excluded.should_print,
                       label_key = excluded.label_key,
                       created_at = datetime('now')",
                )
                .bind(rid)
                .bind(&query_no)
                .bind(&info.shipping_no)
                .bind(&channel_code)
                .bind(&print_profile)
                .bind(1) // 雲端 v2 路徑表示「要列印」,固定寫 1
                .bind(&label_key)
                .execute(&state.db)
                .await;
            }

            // 記一筆 daily request 統計
            let _ = sqlx::query(
                "INSERT INTO daily_stats (date, request_count, success_count)
                 VALUES (date('now'), 1, 1)
                 ON CONFLICT(date) DO UPDATE SET
                    request_count = request_count + 1,
                    success_count = success_count + 1",
            )
            .execute(&state.db)
            .await;

            Ok(Json(DataEnvelope::new(ParcelData {
                channel_code,
                print_profile,
                label_path,
                response_id: info.response_id,
            })))
        }
        Err(AppError::Unauthorized) => {
            let _ = sqlx::query(
                "INSERT INTO daily_stats (date, request_count)
                 VALUES (date('now'), 1)
                 ON CONFLICT(date) DO UPDATE SET request_count = request_count + 1",
            )
            .execute(&state.db)
            .await;
            Err(err_resp(
                StatusCode::UNAUTHORIZED,
                "雲端未登入,請先在桌面 App 完成登入",
            ))
        }
        Err(e) => {
            let _ = sqlx::query(
                "INSERT INTO daily_stats (date, request_count)
                 VALUES (date('now'), 1)
                 ON CONFLICT(date) DO UPDATE SET request_count = request_count + 1",
            )
            .execute(&state.db)
            .await;
            Err(err_resp(StatusCode::BAD_GATEWAY, e.to_string()))
        }
    }
}

/// POST /api/report — 工控機回報
/// 工控機只傳 response_id，server 驗證它存在於 parcel_query_log（防止亂帶值送上雲端）
/// 通過驗證即 enqueue，背景 worker 推送 { response_id } 給雲端
async fn post_report(
    State(state): State<ServerState>,
    Json(req): Json<ReportPayload>,
) -> impl IntoResponse {
    let row = match sqlx::query(
        "SELECT tracking_no, sort_channel FROM parcel_query_log WHERE response_id = ?",
    )
    .bind(req.response_id)
    .fetch_optional(&state.db)
    .await
    {
        Ok(Some(r)) => r,
        Ok(None) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(
                    serde_json::to_value(ApiErrorBody {
                        message: format!(
                            "找不到 response_id={} 對應的查詢紀錄，請先 GET /api/parcel",
                            req.response_id
                        ),
                        status_code: StatusCode::UNPROCESSABLE_ENTITY.as_u16(),
                    })
                    .unwrap(),
                ),
            );
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    serde_json::to_value(ApiErrorBody {
                        message: e.to_string(),
                        status_code: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                    })
                    .unwrap(),
                ),
            );
        }
    };

    let tracking_no: String = row.try_get("tracking_no").unwrap_or_default();
    let channel_code: Option<String> = row.try_get("sort_channel").ok();

    // 用 channel_code 反查通道設定上的貼標人員
    let job_sticker: Option<String> = if let Some(code) = channel_code.as_deref() {
        sqlx::query("SELECT job_sticker FROM sort_channels WHERE channel_code = ?")
            .bind(code)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten()
            .and_then(|r| r.try_get::<Option<String>, _>("job_sticker").ok().flatten())
    } else {
        None
    };

    match state.queue.enqueue(&req, &tracking_no).await {
        Ok(_) => {
            // 發 logistic-cat webhook (fire-and-forget，不阻塞工控機回應)
            // job_user 由 cloud client 內部從設定注入
            let cloud = state.cloud.clone();
            let response_id = req.response_id;
            let sticker = job_sticker.clone();
            tokio::spawn(async move {
                let mut payload = serde_json::json!({
                    "job_id": response_id,
                    "job_sticker": sticker,
                });
                if let Err(e) = cloud.notify_logistic_cat(&mut payload).await {
                    tracing::warn!(response_id, ?e, "logistic-cat webhook 發送失敗");
                }
            });

            (
                StatusCode::OK,
                Json(serde_json::json!({ "message": "OK" })),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                serde_json::to_value(ApiErrorBody {
                    message: e.to_string(),
                    status_code: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                })
                .unwrap(),
            ),
        ),
    }
}
