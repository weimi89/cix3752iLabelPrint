use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path as StdPath;
use std::sync::Arc;

use axum::{
    extract::{Host, Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use sqlx::Row;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

use crate::cache::{derive_label_key, CacheManager};
use crate::cloud::CloudClient;
use crate::config::{AppConfig, LabelPathConfig, LabelPathMode};
use crate::db::DbPool;
use crate::models::{
    ApiErrorBody, DataEnvelope, ParcelData, ReportPayload, SuccessEnvelope,
};
use crate::queue::QueueManager;
use crate::watermark::{derive_repeat_key, WatermarkRenderer};
use crate::{AppError, AppResult};

/// 面單路徑解析器:依設定把本地絕對路徑轉成 local / share / http 三種形態
#[derive(Clone)]
pub struct LabelPathResolver {
    inner: Arc<RwLock<LabelPathConfig>>,
}

impl LabelPathResolver {
    pub fn new(config: &AppConfig) -> Self {
        Self { inner: Arc::new(RwLock::new(config.label_path.clone())) }
    }

    pub fn apply_config(&self, config: &AppConfig) {
        *self.inner.write() = config.label_path.clone();
    }

    /// 將本地絕對路徑依當前模式轉換為要回給工控機的字串
    /// - `local_abs`: cache 命中後產生的本地絕對路徑
    /// - `cache_base`: cache 根目錄(用來推出相對路徑)
    /// - `label_key`: 相對 key(fallback 用)
    /// - `host`: 請求的 Host header(http 模式 base 留空時自動採用)
    pub fn resolve(
        &self,
        local_abs: &StdPath,
        cache_base: &StdPath,
        label_key: &str,
        host: Option<&str>,
    ) -> String {
        let cfg = self.inner.read();
        match cfg.mode {
            LabelPathMode::Local => local_abs.to_string_lossy().to_string(),
            LabelPathMode::Share => {
                let root = cfg.share_root.trim();
                if root.is_empty() {
                    return local_abs.to_string_lossy().to_string();
                }
                let relative = local_abs
                    .strip_prefix(cache_base)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| label_key.trim_start_matches('/').to_string());
                join_share(root, &relative)
            }
            LabelPathMode::Http => {
                let key = label_key.trim_start_matches('/');
                match host {
                    Some(h) => format!("http://{}/images/{}", h, key),
                    None => format!("/images/{}", key),
                }
            }
        }
    }
}

/// 依 share_root 的分隔符風格(`\\` 或 `/`)合成路徑
fn join_share(root: &str, relative: &str) -> String {
    let use_backslash = root.contains('\\');
    let rel_normalized = if use_backslash {
        relative.replace('/', "\\")
    } else {
        relative.replace('\\', "/")
    };
    let sep = if use_backslash { '\\' } else { '/' };
    let root_trimmed = root.trim_end_matches(['\\', '/']);
    let rel_trimmed = rel_normalized.trim_start_matches(['\\', '/']);
    format!("{}{}{}", root_trimmed, sep, rel_trimmed)
}

/// 每個物流商代碼下一次該分配的 channel 索引（round-robin）
type RoundRobinState = Arc<Mutex<HashMap<String, usize>>>;

#[derive(Clone)]
struct ServerState {
    db: DbPool,
    cloud: CloudClient,
    cache: CacheManager,
    queue: QueueManager,
    rr: RoundRobinState,
    label_resolver: LabelPathResolver,
    watermark: WatermarkRenderer,
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

/// 啟動 axum HTTP server,回傳一個可以關閉它的 handle
pub async fn start(
    config: &AppConfig,
    db: DbPool,
    cloud: CloudClient,
    cache: CacheManager,
    queue: QueueManager,
    label_resolver: LabelPathResolver,
    watermark: WatermarkRenderer,
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
        label_resolver,
        watermark,
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
    Host(host): Host,
    Path(query_no): Path<String>,
) -> Result<Json<DataEnvelope<ParcelData>>, (StatusCode, Json<ApiErrorBody>)> {
    match state.cloud.fetch_parcel(&query_no).await {
        Ok(info) => {
            // 用 shipping_no 作為快取 key (檔名末段帶副檔名,從 shipping_image URL 推得)
            let label_key = derive_label_key(&info.shipping_image);
            let cache_base = state.cache.base_dir();

            // 第一階段:確保原圖已在本地快取
            let original_ok = if state.cache.has_local(&label_key) {
                let _ = state.cache.record_hit(&label_key).await;
                true
            } else {
                let _ = state.cache.record_miss().await;
                match state.cache.fetch_now(&label_key, &info.shipping_image).await {
                    Ok(()) => true,
                    Err(e) => {
                        tracing::warn!(label_key = %label_key, ?e, "同步下載快取失敗");
                        false
                    }
                }
            };

            // 第二階段:若 print_num > 1,套用列印次數浮水印(對齊雲端 OrderPrintController)
            // 浮水印失敗(字型缺、寫檔失敗等)時 fallback 回原圖,不阻斷正常出單流程
            let print_num = info.print_num.unwrap_or(0);
            let effective_key = if original_ok && print_num > 1 {
                let repeat_key = derive_repeat_key(&label_key, &info.shipping_provider);
                let src = state.cache.local_path_for_key(&label_key);
                let dst = cache_base.join(&repeat_key);
                match state.watermark.apply(&src, &dst, print_num, &info.shipping_provider) {
                    Ok(()) => repeat_key,
                    Err(e) => {
                        tracing::warn!(label_key = %label_key, print_num, %e, "浮水印生成失敗,回原圖");
                        label_key.clone()
                    }
                }
            } else {
                label_key.clone()
            };

            // 第三階段:依 label_path.mode 把絕對路徑轉成回應字串
            let label_path = if original_ok {
                let local_abs = cache_base.join(&effective_key);
                Some(state.label_resolver.resolve(
                    &local_abs,
                    &cache_base,
                    &effective_key,
                    Some(host.as_str()),
                ))
            } else {
                None
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
                       (response_id, query_no, tracking_no, shipping_provider, sort_channel, print_profile, should_print, label_key)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                     ON CONFLICT(response_id) DO UPDATE SET
                       query_no = excluded.query_no,
                       tracking_no = excluded.tracking_no,
                       shipping_provider = excluded.shipping_provider,
                       sort_channel = excluded.sort_channel,
                       print_profile = excluded.print_profile,
                       should_print = excluded.should_print,
                       label_key = excluded.label_key,
                       created_at = datetime('now')",
                )
                .bind(rid)
                .bind(&query_no)
                .bind(&info.shipping_no)
                .bind(&info.shipping_provider)
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
/// 驗 response_id 存在於 parcel_query_log,通過後寫入本機 queue (status=pending);
/// 背景 worker 會推送 logistic-cat webhook + 追蹤 status/retry/last_error
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

    // 寫入本機 queue (status=pending,含分流通道 + 貼標人員),
    // 背景 worker 會推送 logistic-cat webhook 並追蹤 status/retry/last_error
    if let Err(e) = state
        .queue
        .enqueue(
            &req,
            &tracking_no,
            channel_code.as_deref(),
            job_sticker.as_deref(),
        )
        .await
    {
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

    (
        StatusCode::OK,
        Json(serde_json::json!({ "message": "OK" })),
    )
}
