use std::net::SocketAddr;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use serde::Serialize;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

use crate::cache::CacheManager;
use crate::cloud::CloudClient;
use crate::config::AppConfig;
use crate::db::DbPool;
use crate::models::{ParcelInfo, ReportPayload};
use crate::queue::QueueManager;
use crate::{AppError, AppResult};

#[derive(Clone)]
struct ServerState {
    db: DbPool,
    cloud: CloudClient,
    cache: CacheManager,
    queue: QueueManager,
    listen_url_prefix: String,
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

    let listen_url_prefix = format!("http://{}:{}", local_display_ip(&config.server.listen_ip), config.server.port);

    let state = ServerState {
        db,
        cloud,
        cache: cache.clone(),
        queue,
        listen_url_prefix,
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

fn local_display_ip(listen_ip: &str) -> String {
    // 0.0.0.0 對外時，給前端顯示用換成 127.0.0.1
    if listen_ip == "0.0.0.0" || listen_ip == "::" {
        "127.0.0.1".to_string()
    } else {
        listen_ip.to_string()
    }
}

#[derive(Serialize)]
struct HealthResp {
    ok: bool,
    name: &'static str,
}

async fn healthz() -> Json<HealthResp> {
    Json(HealthResp {
        ok: true,
        name: "cix3752i-label-print",
    })
}

#[derive(Serialize)]
struct ApiError {
    success: bool,
    code: String,
    message: String,
}

impl ApiError {
    fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self { success: false, code: code.into(), message: message.into() }
    }
}

/// GET /api/parcel/:query_no — 工控機呼叫
async fn get_parcel(
    State(state): State<ServerState>,
    Path(query_no): Path<String>,
) -> Result<Json<ParcelInfo>, (StatusCode, Json<ApiError>)> {
    match state.cloud.fetch_parcel(&query_no).await {
        Ok(mut info) => {
            // 處理 cache hit/miss 與背景補下載
            if let Some(key) = info.label_key.clone() {
                let cloud_url = info.label_url.clone();
                if state.cache.has_local(&key) {
                    info.label_source = "local".to_string();
                    info.label_url = Some(format!(
                        "{}/images/{}",
                        state.listen_url_prefix,
                        key.trim_start_matches('/')
                    ));
                    info.label_path = Some(
                        state.cache.local_path_for_key(&key).to_string_lossy().to_string(),
                    );
                    let _ = state.cache.record_hit(&key).await;
                } else {
                    let _ = state.cache.record_miss().await;
                    // 雲端有圖,本地沒圖 → 背景補下載(不阻塞回應)
                    if let Some(src) = cloud_url {
                        state.cache.spawn_prefetch(key, src);
                    }
                }
            }

            // 記一筆 daily request 統計
            let _ = sqlx::query(
                "INSERT INTO daily_stats (date, request_count, success_count)
                 VALUES (date('now'), 1, 1)
                 ON CONFLICT(date) DO UPDATE SET
                    request_count = request_count + 1,
                    success_count = success_count + 1"
            ).execute(&state.db).await;

            Ok(Json(info))
        }
        Err(AppError::Unauthorized) => {
            let _ = sqlx::query(
                "INSERT INTO daily_stats (date, request_count)
                 VALUES (date('now'), 1)
                 ON CONFLICT(date) DO UPDATE SET request_count = request_count + 1",
            )
            .execute(&state.db)
            .await;
            Err((
                StatusCode::UNAUTHORIZED,
                Json(ApiError::new("UNAUTHORIZED", "雲端未登入,請先在桌面 App 完成登入")),
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
            Err((
                StatusCode::BAD_GATEWAY,
                Json(ApiError::new("CLOUD_ERROR", e.to_string())),
            ))
        }
    }
}

#[derive(Serialize)]
struct ReportResp {
    success: bool,
    queue_id: i64,
}

/// POST /api/report — 工控機回報
async fn post_report(
    State(state): State<ServerState>,
    Json(req): Json<ReportPayload>,
) -> impl IntoResponse {
    match state.queue.enqueue(&req).await {
        Ok(id) => (StatusCode::OK, Json(serde_json::to_value(ReportResp { success: true, queue_id: id }).unwrap())),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::to_value(ApiError::new("QUEUE_WRITE_FAIL", e.to_string())).unwrap()),
        ),
    }
}
