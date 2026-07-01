use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path as StdPath, PathBuf};
use std::sync::Arc;

use crate::event_log;

use axum::{
    extract::{Host, Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

use crate::cache::{derive_label_key, CacheManager, FetchOutcome};
use crate::camera::CameraManager;
use crate::cloud::CloudClient;
use crate::config::{AppConfig, LabelPathConfig, LabelPathMode};
use crate::db::DbPool;
use crate::models::{
    ApiErrorBody, DataEnvelope, ParcelData, ReportPayload, SuccessEnvelope,
};
use crate::queue::QueueManager;
use crate::watermark::{derive_repeat_key, WatermarkRenderer};
use crate::bag_check::BagCheckState;
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

    pub fn current_mode(&self) -> LabelPathMode {
        self.inner.read().mode
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
            // DirectPrint 模式不經此方法，fallback 回本機路徑(理論上不會到達)
            LabelPathMode::DirectPrint => local_abs.to_string_lossy().to_string(),
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

/// 每個物流商代碼下一次該分配的 channel 索引（round-robin）。
/// 用 tokio async Mutex(非 parking_lot):resolve_channel_code 需在持鎖期間 await DB
/// 做原子 skip 消耗,把「讀-判定-扣減」全序列化,杜絕並發 double-skip 競態。
type RoundRobinState = Arc<tokio::sync::Mutex<HashMap<String, usize>>>;

#[derive(Clone)]
struct ServerState {
    db: DbPool,
    cloud: CloudClient,
    cache: CacheManager,
    queue: QueueManager,
    rr: RoundRobinState,
    label_resolver: LabelPathResolver,
    watermark: WatermarkRenderer,
    bag_check: BagCheckState,
    camera: CameraManager,
    /// 讀碼站存證目錄(獨立於面單快取,server 啟動時由 config 解析定版;存檔與 /captures 服務共用)
    captures_dir: PathBuf,
    app: tauri::AppHandle,
    /// DirectPrint 模式有序列印佇列:get_parcel 把工作丟進來,由單一 worker 逐筆 FIFO 處理。
    /// 保證列印順序 = 請求順序,且同時只有一筆在送印(不並發打 spooler)。
    direct_print_tx: mpsc::UnboundedSender<DirectPrintJob>,
}

/// DirectPrint 一筆待列印工作(下載 + 浮水印 + 送本機印表機所需的最小資料)
struct DirectPrintJob {
    label_key: String,
    image_url: String,
    provider: String,
    print_num: u32,
    query_no: String,
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
    bag_check: BagCheckState,
    camera: CameraManager,
    app: tauri::AppHandle,
) -> AppResult<ServerHandle> {
    let addr: SocketAddr = format!("{}:{}", config.server.listen_ip, config.server.port)
        .parse()
        .map_err(|e| AppError::Server(format!("無法解析 listen 位址: {e}")))?;

    let log_db = db.clone();

    // DirectPrint 有序列印 worker:單一 FIFO consumer,逐筆 await(下載→浮水印→列印)。
    // 保證列印順序 = 請求入列順序(工控機一件件刷即一件件入列),且同一時間只送印一筆,
    // 不並發打印表機 spooler(Windows GDI 對並發 stale-state 敏感)。
    let (direct_print_tx, mut direct_print_rx) = mpsc::unbounded_channel::<DirectPrintJob>();
    {
        let cache = cache.clone();
        let watermark = watermark.clone();
        let db = db.clone();
        tokio::spawn(async move {
            while let Some(job) = direct_print_rx.recv().await {
                run_direct_print_job(&cache, &watermark, &db, job).await;
            }
        });
    }

    // 存證目錄:啟動時依 config 解析定版(與面單快取分離)。存檔與 /captures 服務共用同一份,確保一致。
    let captures_dir = config.resolved_captures_dir(&app)?;

    let state = ServerState {
        db,
        cloud,
        cache: cache.clone(),
        queue,
        rr: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        label_resolver,
        watermark,
        bag_check,
        camera,
        captures_dir: captures_dir.clone(),
        app,
        direct_print_tx,
    };

    let images_service = ServeDir::new(cache.base_dir());
    let captures_service = ServeDir::new(&captures_dir);

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/api/parcel/:query_no", get(get_parcel))
        .route("/api/report", post(post_report))
        .route("/api/device-alert", post(post_device_alert))
        // 手機遙控分揀通道暫停(換紙等臨時暫停某通道,不影響其他通道)
        .route("/control", get(control_page))
        .route("/api/alerts", get(list_alerts))
        .route("/api/channels", get(list_channels))
        .route("/api/channels/:position", post(set_channel_enabled))
        .route("/api/channels/:position/skip", post(skip_channel))
        .route("/api/channels/:position/recent", get(channel_recent))
        .route("/camera/preview", get(camera_preview))
        .route("/camera/preview/stream", get(camera_preview_stream))
        .nest_service("/images", images_service)
        .nest_service("/captures", captures_service)
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
    event_log::log_bg(log_db, "info", "server", "伺服器啟動",
        format!("HTTP server 已啟動 {addr}"));

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

/// 工控機 GET /api/parcel 失敗時推給前端的提示(前端依 kind 播對應提示音 + toast)
#[derive(Serialize, Clone)]
struct ParcelAlert<'a> {
    kind: &'a str,
    message: String,
    query_no: String,
}

/// 依雲端回的機器可讀 code 歸類成前端 kind(對應提示音)。
/// 雲端 OrderPrintController 各錯誤分支回 code:STORE_CLOSED / UNCONFIRMED / NOT_FOUND /
/// STATUS_ABNORMAL / NOT_PROXY / NOT_FORWARD / LABEL_FAILED;後三者與未知 code 一律 "error"。
/// 舊版雲端未回 code 時 code 會是 HTTP 狀態碼字串 → 落入 "error"(仍會顯示原始 message)。
// =====================================================================
// 手機遙控:分揀通道暫停 / 恢復(同區網手機開 /control 網頁操作)
// =====================================================================

/// 回給手機控制頁的單一通道狀態
#[derive(Serialize)]
struct ChannelView {
    position: String,
    channel_code: Option<String>,
    enabled: bool,
    dispatch_codes: Vec<String>,
    /// 指派物流的「名稱」(對齊 dispatch_codes 順序;查無對應名稱時退回代碼)
    dispatch_names: Vec<String>,
    /// 該通道的貼標人員(現場個別指派,方便人員認出自己負責的通道)
    job_sticker: Option<String>,
    /// 待跳過本輪次數(round-robin 輪到時消耗一次)
    skip_count: i64,
    /// 該通道最近一筆分到的物流單號(print_event.shipping_no,供現場對單)
    last_tracking: Option<String>,
}

/// GET /api/channels — 列出 8 個分揀通道與啟用狀態(手機控制頁輪詢用)
async fn list_channels(State(state): State<ServerState>) -> impl IntoResponse {
    let rows = sqlx::query(
        "SELECT position, channel_code, enabled, job_sticker, skip_count
         FROM sort_channels
         ORDER BY
           CASE substr(position,1,1) WHEN 'L' THEN 0 WHEN 'R' THEN 1 ELSE 2 END,
           CAST(substr(position,2) AS INTEGER)",
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_else(|e| { tracing::warn!(?e, "list_channels 讀 sort_channels 失敗,回空清單"); Vec::new() });

    let disp_rows = sqlx::query(
        "SELECT position, dispatch_code FROM sort_channel_dispatch ORDER BY position, dispatch_code",
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_else(|e| { tracing::warn!(?e, "list_channels 讀 sort_channel_dispatch 失敗"); Vec::new() });
    let mut disp_map: HashMap<String, Vec<String>> = HashMap::new();
    for r in disp_rows {
        let p: String = r.try_get("position").unwrap_or_default();
        let c: String = r.try_get("dispatch_code").unwrap_or_default();
        if !p.is_empty() && !c.is_empty() {
            disp_map.entry(p).or_default().push(c);
        }
    }

    // 物流商代碼 → 名稱(顯示用;查無名稱時退回代碼)
    let provider_rows = sqlx::query("SELECT code, name FROM dispatch_provider")
        .fetch_all(&state.db)
        .await
        .unwrap_or_else(|e| { tracing::warn!(?e, "list_channels 讀 dispatch_provider 失敗"); Vec::new() });
    let mut name_map: HashMap<String, String> = HashMap::new();
    for r in provider_rows {
        let code: String = r.try_get("code").unwrap_or_default();
        let name: String = r.try_get("name").unwrap_or_default();
        if !code.is_empty() {
            name_map.insert(code, name);
        }
    }

    // 每個通道代碼最近一筆物流單號(SQLite:單一 MAX 聚合時,同列其他欄取自該最大列)
    let track_rows = sqlx::query(
        "SELECT channel_code, shipping_no, MAX(created_at) AS mx
         FROM print_event
         WHERE channel_code IS NOT NULL AND channel_code <> ''
           AND shipping_no IS NOT NULL AND shipping_no <> ''
         GROUP BY channel_code",
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_else(|e| { tracing::warn!(?e, "list_channels 讀最近單號失敗"); Vec::new() });
    let mut track_map: HashMap<String, String> = HashMap::new();
    for r in track_rows {
        let cc: String = r.try_get("channel_code").unwrap_or_default();
        let sn: String = r.try_get("shipping_no").unwrap_or_default();
        if !cc.is_empty() && !sn.is_empty() {
            track_map.insert(cc, sn);
        }
    }

    let list: Vec<ChannelView> = rows
        .into_iter()
        .map(|r| {
            let position: String = r.try_get("position").unwrap_or_default();
            let channel_code: Option<String> = r.try_get("channel_code").ok();
            let dispatch_codes = disp_map.remove(&position).unwrap_or_default();
            let dispatch_names = dispatch_codes
                .iter()
                .map(|c| {
                    name_map
                        .get(c)
                        .filter(|n| !n.is_empty())
                        .cloned()
                        .unwrap_or_else(|| c.clone())
                })
                .collect();
            let last_tracking = channel_code
                .as_deref()
                .and_then(|c| track_map.get(c).cloned());
            ChannelView {
                enabled: r.try_get::<i64, _>("enabled").unwrap_or(1) != 0,
                job_sticker: r.try_get("job_sticker").ok(),
                skip_count: r.try_get::<i64, _>("skip_count").unwrap_or(0),
                channel_code,
                last_tracking,
                dispatch_codes,
                dispatch_names,
                position,
            }
        })
        .collect();
    Json(list)
}

#[derive(Deserialize)]
struct SetEnabledBody {
    enabled: bool,
}

/// POST /api/channels/:position — 手機暫停 / 恢復某通道,即時生效並廣播給桌面 GUI
async fn set_channel_enabled(
    State(state): State<ServerState>,
    Path(position): Path<String>,
    Json(body): Json<SetEnabledBody>,
) -> impl IntoResponse {
    use crate::commands::sort_channel_commands::POSITIONS;
    use tauri::Emitter;

    if !POSITIONS.contains(&position.as_str()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("無效的通道位置: {position}") })),
        )
            .into_response();
    }

    let res = sqlx::query(
        "UPDATE sort_channels SET enabled = ?, updated_at = datetime('now','localtime') WHERE position = ?",
    )
    .bind(if body.enabled { 1 } else { 0 })
    .bind(&position)
    .execute(&state.db)
    .await;

    if let Err(e) = res {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response();
    }

    // 廣播給桌面分揀通道頁即時同步開關狀態
    let _ = state.app.emit(
        "sort-channel-updated",
        serde_json::json!({ "position": position, "enabled": body.enabled }),
    );

    Json(serde_json::json!({ "position": position, "enabled": body.enabled })).into_response()
}

#[derive(Deserialize, Default)]
struct SkipBody {
    /// true=清除待跳過;否則累加一次
    #[serde(default)]
    clear: bool,
}

/// POST /api/channels/:position/skip — 跳過本輪該通道(累加一次),或清除(clear=true)
async fn skip_channel(
    State(state): State<ServerState>,
    Path(position): Path<String>,
    Json(body): Json<SkipBody>,
) -> impl IntoResponse {
    use crate::commands::sort_channel_commands::POSITIONS;

    if !POSITIONS.contains(&position.as_str()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("無效的通道位置: {position}") })),
        )
            .into_response();
    }

    // 累加上限 20,避免誤觸暴增
    let sql = if body.clear {
        "UPDATE sort_channels SET skip_count = 0, updated_at = datetime('now','localtime') WHERE position = ?"
    } else {
        "UPDATE sort_channels SET skip_count = MIN(skip_count + 1, 20), updated_at = datetime('now','localtime') WHERE position = ?"
    };
    if let Err(e) = sqlx::query(sql).bind(&position).execute(&state.db).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response();
    }

    let skip_count: i64 = sqlx::query("SELECT skip_count FROM sort_channels WHERE position = ?")
        .bind(&position)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .and_then(|r| r.try_get::<i64, _>("skip_count").ok())
        .unwrap_or(0);

    Json(serde_json::json!({ "position": position, "skip_count": skip_count })).into_response()
}

/// 雲端查件異常記錄(回看清單用)
#[derive(Serialize)]
struct AlertView {
    id: i64,
    kind: String,
    code: Option<String>,
    query_no: Option<String>,
    message: Option<String>,
    channel_code: Option<String>,
    /// 雲端帶出的精確物流單號(查得到訂單才有),供清單對單;與工控機掃的 query_no 區分
    shipping_no: Option<String>,
    created_at: String,
}

/// 共用查詢:最近 N 筆雲端查件異常(手機 endpoint 與桌面 command 共用)
pub async fn fetch_recent_alerts(db: &DbPool, limit: i64) -> Vec<serde_json::Value> {
    let rows = sqlx::query(
        "SELECT id, kind, code, query_no, message, channel_code, shipping_no, created_at
         FROM parcel_alert ORDER BY id DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(db)
    .await
    .unwrap_or_else(|e| { tracing::warn!(?e, "fetch_recent_alerts 讀 parcel_alert 失敗"); Vec::new() });
    rows.into_iter()
        .map(|r| {
            serde_json::to_value(AlertView {
                id: r.try_get("id").unwrap_or_default(),
                kind: r.try_get("kind").unwrap_or_default(),
                code: r.try_get("code").ok(),
                query_no: r.try_get("query_no").ok(),
                message: r.try_get("message").ok(),
                channel_code: r.try_get("channel_code").ok(),
                shipping_no: r.try_get("shipping_no").ok().flatten(),
                created_at: r.try_get("created_at").unwrap_or_default(),
            })
            .unwrap_or(serde_json::Value::Null)
        })
        .collect()
}

/// GET /api/alerts — 最近雲端查件異常(手機輪詢 / 清單)
async fn list_alerts(State(state): State<ServerState>) -> impl IntoResponse {
    Json(fetch_recent_alerts(&state.db, 50).await)
}

/// 單一通道最近活動(正常單號 + 錯誤合併,依時間新→舊)
#[derive(Serialize)]
struct ChannelRecentItem {
    num: String,            // 物流單號 / 查詢單號
    kind: Option<String>,   // null=正常出貨;有值=錯誤類別(store_closed…)
    created_at: String,
}

/// GET /api/channels/:position/recent — 該通道最近 3 筆(當前 + 歷史 2 筆)
async fn channel_recent(
    State(state): State<ServerState>,
    Path(position): Path<String>,
) -> impl IntoResponse {
    use crate::commands::sort_channel_commands::POSITIONS;
    if !POSITIONS.contains(&position.as_str()) {
        return Json(Vec::<ChannelRecentItem>::new());
    }
    // 取該通道目前的 channel_code
    let cc: Option<String> = sqlx::query("SELECT channel_code FROM sort_channels WHERE position = ?")
        .bind(&position)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .and_then(|r| r.try_get::<Option<String>, _>("channel_code").ok().flatten())
        .filter(|s| !s.is_empty());
    let Some(cc) = cc else {
        return Json(Vec::<ChannelRecentItem>::new());
    };

    let rows = sqlx::query(
        "SELECT created_at, shipping_no AS num, NULL AS kind
           FROM print_event
          WHERE channel_code = ? AND shipping_no IS NOT NULL AND shipping_no <> ''
         UNION ALL
         SELECT created_at, COALESCE(NULLIF(shipping_no, ''), query_no) AS num, kind
           FROM parcel_alert
          WHERE channel_code = ? AND COALESCE(NULLIF(shipping_no, ''), query_no) IS NOT NULL
            AND COALESCE(NULLIF(shipping_no, ''), query_no) <> ''
         ORDER BY created_at DESC, num DESC
         LIMIT 3",
    )
    .bind(&cc)
    .bind(&cc)
    .fetch_all(&state.db)
    .await
    .unwrap_or_else(|e| { tracing::warn!(?e, "channel_recent 讀最近活動失敗"); Vec::new() });

    let items: Vec<ChannelRecentItem> = rows
        .into_iter()
        .map(|r| ChannelRecentItem {
            num: r.try_get("num").unwrap_or_default(),
            kind: r.try_get::<Option<String>, _>("kind").ok().flatten(),
            created_at: r.try_get("created_at").unwrap_or_default(),
        })
        .collect();
    Json(items)
}

/// GET /control — 手機版分揀通道暫停控制頁(自帶 CSS/JS,離線可用)
/// 相機即時預覽(單張):回傳記憶體中「最新一幀」JPEG。保留作為串流不可用時的退路。
async fn camera_preview(State(state): State<ServerState>) -> impl IntoResponse {
    match state.camera.latest_jpeg() {
        Some(jpeg) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "image/jpeg"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            jpeg,
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// 相機即時預覽(**MJPEG 串流**):`multipart/x-mixed-replace`,持續把「最新一幀」推給前端 `<img>`。
/// 比 1 秒輪詢順得多(約 10fps,= 擷取迴圈速率),且**相機只由後端 nokhwa 獨佔**——
/// 不像前端 getUserMedia 會在 Windows 工控機上跟存證擷取搶相機;預覽畫面就是存證實際畫面(含已套用 zoom)。
/// 前端關掉 `<img>`(離開設定頁)時連線中斷,stream 自動結束,不殘留。
async fn camera_preview_stream(State(state): State<ServerState>) -> impl IntoResponse {
    let camera = state.camera.clone();
    let stream = futures::stream::unfold(camera, |camera| async move {
        // ~10fps:對位用足夠順;與擷取迴圈同速率,不額外吃 CPU
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let chunk = match camera.latest_jpeg() {
            Some(jpeg) if !jpeg.is_empty() => {
                let mut c = Vec::with_capacity(jpeg.len() + 80);
                c.extend_from_slice(b"--frame\r\nContent-Type: image/jpeg\r\nContent-Length: ");
                c.extend_from_slice(jpeg.len().to_string().as_bytes());
                c.extend_from_slice(b"\r\n\r\n");
                c.extend_from_slice(&jpeg);
                c.extend_from_slice(b"\r\n");
                c
            }
            _ => Vec::new(), // 尚無幀:本輪不送內容,下輪再試
        };
        Some((Ok::<Vec<u8>, std::io::Error>(chunk), camera))
    });
    (
        [(
            header::CONTENT_TYPE,
            "multipart/x-mixed-replace; boundary=frame",
        )],
        axum::body::Body::from_stream(stream),
    )
        .into_response()
}

async fn control_page() -> impl IntoResponse {
    axum::response::Html(include_str!("control_page.html"))
}

fn classify_parcel_alert(code: &str) -> &'static str {
    match code {
        "STORE_CLOSED" => "store_closed",
        "UNCONFIRMED" => "unconfirmed",
        "NOT_FOUND" => "not_found",
        "NOT_PROXY" => "not_proxy",
        "NOT_FORWARD" => "not_forward",
        _ => "error",
    }
}

/// 依物流商代碼解析分揀通道:有指派通道時 round-robin 輪流分配;
/// 未指派任何通道時退回 fallback「未指派通道代碼」設定(settings.unassigned_channel_code)。
/// 回傳 `(channel_code, has_assigned)`,`has_assigned=false` 代表該物流商沒有任何指派通道。
/// 正常面單與錯誤面單共用,確保兩者分揀行為一致。
async fn resolve_channel_code(
    db: &DbPool,
    rr: &RoundRobinState,
    provider: &str,
) -> (Option<String>, bool) {
    // 一個物流可被指派到多個通道,一個通道也可指派多個物流(多對多,sort_channel_dispatch)
    let rows = sqlx::query(
        "SELECT sc.position, sc.channel_code
         FROM sort_channel_dispatch scd
         JOIN sort_channels sc ON sc.position = scd.position
         WHERE scd.dispatch_code = ?
           AND sc.enabled = 1
           AND sc.channel_code IS NOT NULL
           AND sc.channel_code <> ''
         ORDER BY
           CASE substr(sc.position,1,1) WHEN 'L' THEN 0 WHEN 'R' THEN 1 ELSE 2 END,
           CAST(substr(sc.position,2) AS INTEGER)",
    )
    .bind(provider)
    .fetch_all(db)
    .await
    .unwrap_or_else(|e| { tracing::warn!(?e, provider, "resolve_channel_code 讀通道指派失敗,退回 fallback"); Vec::new() });

    // (position, channel_code)
    let candidates: Vec<(String, String)> = rows
        .into_iter()
        .filter_map(|r| {
            let pos: String = r.try_get("position").ok()?;
            let code: Option<String> = r.try_get("channel_code").ok().flatten();
            code.filter(|c| !c.is_empty()).map(|c| (pos, c))
        })
        .collect();

    if candidates.is_empty() {
        // 未設定指派物流時，使用 fallback 通道代碼
        return (fetch_unassigned_channel_code(db).await, false);
    }

    let n = candidates.len();
    // 全程持 async 鎖跨 await:把「輪轉選位 + skip 原子消耗」序列化,杜絕並發 double-skip。
    // skip 消耗用條件 UPDATE(WHERE skip_count > 0)+ rows_affected 判定:
    // 真的扣到一次才視為「跳過此通道」,扣不到(額度已被其他請求用盡)就選它 —— 不依賴鎖外快照。
    let chosen: Option<String> = {
        let mut rr = rr.lock().await;
        let entry = rr.entry(provider.to_string()).or_insert(0);
        let start = *entry % n;
        let mut picked: Option<String> = None;
        for step in 0..n {
            let idx = (start + step) % n;
            let (pos, code) = &candidates[idx];
            let consumed = sqlx::query(
                "UPDATE sort_channels SET skip_count = skip_count - 1 WHERE position = ? AND skip_count > 0",
            )
            .bind(pos)
            .execute(db)
            .await
            .map(|r| r.rows_affected() > 0)
            .unwrap_or(false);
            if consumed {
                // 該通道本輪待跳過:已原子消耗一次,改看下一個
                continue;
            }
            picked = Some(code.clone());
            *entry = (idx + 1) % n;
            break;
        }
        if picked.is_none() {
            // 全部通道本輪都被跳過:不分配,前進指標避免卡同一位置
            *entry = (start + 1) % n;
        }
        picked
    };

    match chosen {
        Some(c) => (Some(c), true),
        // 全部待跳過 → 視為當下無可用通道,退回 fallback
        None => (fetch_unassigned_channel_code(db).await, false),
    }
}

/// 取設定頁的「未指派通道代碼」(settings.unassigned_channel_code),未設定或留空回 None。
/// 物流商無指派通道、或錯誤面單查不到物流商(NOT_FOUND / 雲端連線失敗)時的統一 fallback。
async fn fetch_unassigned_channel_code(db: &DbPool) -> Option<String> {
    sqlx::query("SELECT value FROM settings WHERE key = 'unassigned_channel_code'")
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
        .and_then(|r| r.try_get::<String, _>("value").ok())
        .filter(|s| !s.is_empty())
}

/// 取物流商在「指派物流」頁設定的 print_profile
async fn fetch_print_profile(db: &DbPool, provider: &str) -> Option<String> {
    sqlx::query("SELECT print_profile FROM dispatch_provider WHERE code = ?")
        .bind(provider)
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
        .and_then(|r| r.try_get::<Option<String>, _>("print_profile").ok().flatten())
}

/// 找一台可用的列印機,優先序:
/// 1. `provider` 指定的物流商在 dispatch_provider 設定的 printer_name(錯誤面單跟著該物流商的印表機出)
/// 2. 任何已設定 printer_name 的物流商設定
/// 3. 系統預設印表機
/// 在 background task 中呼叫,失敗只 warn 不 panic。
async fn find_any_printer(db: &DbPool, provider: Option<&str>) -> Option<String> {
    if let Some(code) = provider {
        let row = sqlx::query(
            "SELECT printer_name FROM dispatch_provider WHERE code = ? AND printer_name IS NOT NULL AND printer_name != ''",
        )
        .bind(code)
        .fetch_optional(db)
        .await
        .ok()
        .flatten();
        if let Some(r) = row {
            if let Ok(Some(name)) = r.try_get::<Option<String>, _>("printer_name") {
                return Some(name);
            }
        }
    }

    let row = sqlx::query(
        "SELECT printer_name FROM dispatch_provider WHERE printer_name IS NOT NULL AND printer_name != '' LIMIT 1",
    )
    .fetch_optional(db)
    .await
    .ok()
    .flatten();

    if let Some(r) = row {
        if let Ok(Some(name)) = r.try_get::<Option<String>, _>("printer_name") {
            return Some(name);
        }
    }

    // fallback：系統預設印表機
    crate::printer::list_printers()
        .into_iter()
        .find(|p| p.is_default)
        .map(|p| p.name)
}

/// 把已產生的錯誤面單 bytes 背景送到本機印表機（fire-and-forget，不阻塞主流程）。
/// 找不到印表機或列印失敗時，除了 warn 也 emit `error-label-print-failed` 讓桌面 GUI 跳提示，
/// 避免「以為印了其實沒印」的靜默盲區。
fn spawn_print_error_label_bytes(
    db: DbPool,
    app: tauri::AppHandle,
    query_no: String,
    label_bytes: Vec<u8>,
    provider: Option<String>,
) {
    tokio::spawn(async move {
        match find_any_printer(&db, provider.as_deref()).await {
            Some(printer_name) => {
                let qn = query_no.clone();
                tokio::task::spawn_blocking(move || {
                    if let Err(e) = crate::printer::print_image_bytes(&printer_name, &label_bytes) {
                        tracing::warn!(?e, query_no = %qn, "錯誤面單列印失敗");
                        emit_error_label_failed(&app, &qn, "print_failed");
                    }
                });
            }
            None => {
                tracing::warn!(query_no = %query_no, "無可用印表機，略過錯誤面單列印");
                emit_error_label_failed(&app, &query_no, "no_printer");
            }
        }
    });
}

/// 產生錯誤面單並背景列印（GUI 掃描 / 自動印單用：操作員在本機，直接送本機印表機）。
pub(crate) fn spawn_error_label_print(
    db: DbPool,
    app: tauri::AppHandle,
    query_no: String,
    error_code: String,
    provider: Option<String>,
) {
    let label_bytes = crate::error_label::generate(
        &query_no,
        &error_code,
        crate::error_label::LabelHeight::H100mm,
    );
    spawn_print_error_label_bytes(db, app, query_no, label_bytes, provider);
}

/// emit `error-label-print-failed` 給桌面前端（reason: "no_printer" / "print_failed" / "cache_write_failed"）。
fn emit_error_label_failed(app: &tauri::AppHandle, query_no: &str, reason: &str) {
    use tauri::Emitter;
    let payload = serde_json::json!({ "query_no": query_no, "reason": reason });
    if let Err(e) = app.emit("error-label-print-failed", payload) {
        tracing::warn!(?e, "emit error-label-print-failed 失敗");
    }
}

/// 將錯誤面單 PNG 寫入 cache（key 前綴 `@error/`，對齊浮水印 `@repeat/` 慣例），
/// 讓工控機可透過 `/images/{key}` 或 share / local 路徑自行取用列印。
/// 回傳 cache 相對 key；寫檔失敗回 None（呼叫端退回本機直接列印 + 502）。
/// 亦供 cloud_commands(GUI 掃描 / 自動印單)組裝錯誤面單 URL 用。
pub(crate) async fn write_error_label_to_cache(
    cache_base: &StdPath,
    query_no: &str,
    error_code: &str,
    bytes: &[u8],
) -> Option<String> {
    // 檔名只保留安全字元，避免 query_no 帶斜線 / 空白等破壞路徑
    let safe_qn: String = query_no
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let key = format!("@error/{safe_qn}_{error_code}.png");
    let path = cache_base.join(&key);
    if let Some(parent) = path.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            tracing::warn!(?e, "建立錯誤面單快取目錄失敗");
            return None;
        }
    }
    match tokio::fs::write(&path, bytes).await {
        Ok(()) => Some(key),
        Err(e) => {
            tracing::warn!(?e, "寫入錯誤面單快取失敗");
            None
        }
    }
}

/// emit `parcel-alert` 給前端;失敗只記 warn,不影響回應工控機
fn emit_parcel_alert(app: &tauri::AppHandle, kind: &str, message: &str, query_no: &str) {
    use tauri::Emitter;
    let payload = ParcelAlert {
        kind,
        message: message.to_string(),
        query_no: query_no.to_string(),
    };
    if let Err(e) = app.emit("parcel-alert", payload) {
        tracing::warn!(?e, "emit parcel-alert 失敗");
    }
}

/// 工控機回報「設備異常」(卡包裹 / USB 斷線 …)時的請求 body。
/// `type` 為機器可讀分類碼(對應前端 i18n 文案 + 語音廣播);`message` 可選,
/// 補充現場細節(如卡在哪個通道),會接在語音/toast 後面唸出。兩者皆可省略。
#[derive(Deserialize)]
struct DeviceAlertReq {
    #[serde(rename = "type")]
    alert_type: Option<String>,
    message: Option<String>,
}

/// 工控機設備異常推給前端的提示。前端 useDeviceAlert 依 `alert_type` 取雙語文案,
/// 廣播一次提示現場人員,並顯示 toast。
#[derive(Serialize, Clone)]
struct DeviceAlert {
    alert_type: String,
    message: String,
}

/// emit `device-alert` 給前端;失敗只記 warn,不影響回應工控機
fn emit_device_alert(app: &tauri::AppHandle, alert_type: &str, message: &str) {
    use tauri::Emitter;
    let payload = DeviceAlert {
        alert_type: alert_type.to_string(),
        message: message.to_string(),
    };
    if let Err(e) = app.emit("device-alert", payload) {
        tracing::warn!(?e, "emit device-alert 失敗");
    }
}

/// DirectPrint 模式:把一筆面單排入有序列印佇列(不阻塞工控機回應)。
/// 此模式下工控機拿到的回應不含 `label_path`(由中介機列印),圖檔處理全在背景 worker 進行;
/// 入列即代表確定要印,單一 worker 會照入列順序逐筆下載+列印,保證列印順序 = 請求順序。
fn enqueue_direct_print(
    state: &ServerState,
    label_key: String,
    image_url: String,
    provider: String,
    print_num: u32,
    query_no: String,
) {
    let job = DirectPrintJob {
        label_key,
        image_url,
        provider,
        print_num,
        query_no,
    };
    if let Err(e) = state.direct_print_tx.send(job) {
        tracing::warn!(?e, "direct_print 列印佇列已關閉,無法排入");
    }
}

/// DirectPrint worker 逐筆執行的單元:下載面單 → 套列印次數浮水印 → 送本機印表機。
/// **整個 await 到列印送出才回傳**,讓 worker 在處理下一筆前確保本筆已送印,保證順序且不並發打 spooler。
async fn run_direct_print_job(
    cache: &CacheManager,
    watermark: &WatermarkRenderer,
    db: &DbPool,
    job: DirectPrintJob,
) {
    let DirectPrintJob {
        label_key,
        image_url,
        provider,
        print_num,
        query_no,
    } = job;
    let cache_base = cache.base_dir();

    // 1. 確保原圖(對應這次 image_url)已在本地快取;由 fetch_now 比對 source_url 決定命中或重抓
    let original_ok = match cache.fetch_now(&label_key, &image_url).await {
        Ok(FetchOutcome::Hit) => {
            let _ = cache.record_hit(&label_key).await;
            true
        }
        Ok(FetchOutcome::Downloaded) => {
            let _ = cache.record_miss().await;
            true
        }
        Err(e) => {
            let _ = cache.record_miss().await;
            tracing::warn!(label_key = %label_key, ?e, "direct_print 背景下載快取失敗");
            false
        }
    };
    if !original_ok {
        return;
    }

    // 2. 列印次數浮水印(print_num > 1);失敗 fallback 回原圖
    let effective_key = if print_num > 1 {
        let repeat_key = derive_repeat_key(&label_key, &provider);
        let src = cache.local_path_for_key(&label_key);
        let dst = cache_base.join(&repeat_key);
        match watermark.apply(&src, &dst, print_num, &provider) {
            Ok(()) => repeat_key,
            Err(e) => {
                tracing::warn!(label_key = %label_key, print_num, %e, "direct_print 浮水印生成失敗,回原圖");
                label_key.clone()
            }
        }
    } else {
        label_key.clone()
    };

    // 3. 查 dispatch_provider.printer_name → 讀圖 → 送印
    let printer_name: Option<String> = sqlx::query(
        "SELECT printer_name FROM dispatch_provider WHERE code = ?",
    )
    .bind(&provider)
    .fetch_optional(db)
    .await
    .ok()
    .flatten()
    .and_then(|r| r.try_get::<Option<String>, _>("printer_name").ok().flatten());

    let Some(pname) = printer_name else {
        tracing::warn!(shipping_provider = %provider, "direct_print 模式但物流商未設定印表機");
        return;
    };

    let img_path = cache_base.join(&effective_key);
    let bytes = match tokio::fs::read(&img_path).await {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(?e, "讀取面單圖失敗，無法列印");
            return;
        }
    };
    // await 此筆送印完成,才讓 worker 取下一筆 → 嚴格順序 + 不並發打 spooler
    let _ = tokio::task::spawn_blocking(move || {
        if let Err(e) = crate::printer::print_image_bytes(&pname, &bytes) {
            tracing::warn!(?e, query_no = %query_no, "直接列印失敗");
        }
    })
    .await;
}

/// GET /api/parcel/:query_no — 工控機呼叫
async fn get_parcel(
    State(state): State<ServerState>,
    Host(host): Host,
    Path(query_no): Path<String>,
) -> Result<Json<DataEnvelope<ParcelData>>, (StatusCode, Json<ApiErrorBody>)> {
    let t_start = std::time::Instant::now();
    // 收到請求的「當下」就釘住讀碼站相機最新一幀(離工控機實際讀碼僅約 20ms),
    // 不在這裡存檔(避免擋住回應),純記憶體複製;後續查得到訂單才丟背景寫檔 + 回寫 photo_path。
    let snapshot = state.camera.latest_jpeg();
    match state.cloud.fetch_parcel(&query_no).await {
        Ok(info) => {
            let cloud_ms = t_start.elapsed().as_millis() as i64;

            // 用 shipping_no 作為快取 key (檔名末段帶副檔名,從 shipping_image URL 推得)
            let label_key = derive_label_key(&info.shipping_image);
            let cache_base = state.cache.base_dir();

            let print_num = info.print_num.unwrap_or(0);

            // 雲端回空 shipping_image(無面單可印)時直接視為「無面單」:不嘗試下載、不排列印,
            // 避免空 URL 推出兜底 key 後發無謂請求 + 噪音警告(內容正確性已由 fetch_now source_url 校驗保證)。
            let has_image = !info.shipping_image.trim().is_empty();

            // DirectPrint 模式:工控機拿到 label_path=null(由中介機列印),不需要圖檔本身 ──
            // 立即回應,圖檔下載 + 浮水印 + 列印全部丟背景,不讓工控機等雲端(設計原則 #2)。
            // 其餘模式(local/share/http):工控機要讀檔,必須同步下載到完成才回(設計原則 #3)。
            let (label_path, label_ms) =
                if !has_image {
                    tracing::warn!(query_no = %query_no, "雲端回空 shipping_image,視為無面單");
                    (None, 0i64)
                } else if state.label_resolver.current_mode() == LabelPathMode::DirectPrint {
                    enqueue_direct_print(
                        &state,
                        label_key.clone(),
                        info.shipping_image.clone(),
                        info.shipping_provider.clone(),
                        print_num,
                        query_no.clone(),
                    );
                    (None, 0i64)
                } else {
                    let t_label = std::time::Instant::now();
                    // 第一階段:確保原圖(對應這次 source_url)已在本地快取(工控機要讀,同步下載到完成)。
                    // 一律走 fetch_now,由它比對 cache_meta.source_url 決定命中或重抓,避免回陳舊面單圖。
                    let original_ok = match state
                        .cache
                        .fetch_now(&label_key, &info.shipping_image)
                        .await
                    {
                        Ok(FetchOutcome::Hit) => {
                            let _ = state.cache.record_hit(&label_key).await;
                            true
                        }
                        Ok(FetchOutcome::Downloaded) => {
                            let _ = state.cache.record_miss().await;
                            true
                        }
                        Err(e) => {
                            let _ = state.cache.record_miss().await;
                            tracing::warn!(label_key = %label_key, ?e, "同步下載快取失敗");
                            false
                        }
                    };

                    // 第二階段:若 print_num > 1,套用列印次數浮水印(對齊雲端 OrderPrintController)
                    // 浮水印失敗(字型缺、寫檔失敗等)時 fallback 回原圖,不阻斷正常出單流程
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
                    let label_ms = t_label.elapsed().as_millis() as i64;

                    // 第三階段:依面單路徑模式 resolve 成工控機可存取的路徑/URL
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
                    (label_path, label_ms)
                };

            // 依雲端回的物流商代碼 (shipping_provider) 解析分揀通道
            // 同物流商可能配多個通道,round-robin 輪流分配;未指派時退回 fallback 通道代碼
            let (channel_code, has_assigned) =
                resolve_channel_code(&state.db, &state.rr, &info.shipping_provider).await;

            // 無指派物流通道時，面單路徑不回傳（工控機無通道可分揀，不需要面單）
            let label_path = if has_assigned { label_path } else { None };

            // 從 dispatch_provider 取 print_profile (使用者在「指派物流」頁面設定)
            let print_profile = fetch_print_profile(&state.db, &info.shipping_provider).await;

            // 紀錄這次查詢,POST /api/report 用 response_id 反查
            if let Some(rid) = info.response_id {
                let total_ms = t_start.elapsed().as_millis() as i64;
                let _ = sqlx::query(
                    "INSERT INTO parcel_query_log
                       (response_id, query_no, tracking_no, shipping_provider, sort_channel, print_profile, should_print, label_key, created_at, cloud_ms, label_ms, total_ms)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, datetime('now','localtime'), ?, ?, ?)
                     ON CONFLICT(response_id) DO UPDATE SET
                       query_no = excluded.query_no,
                       tracking_no = excluded.tracking_no,
                       shipping_provider = excluded.shipping_provider,
                       sort_channel = excluded.sort_channel,
                       print_profile = excluded.print_profile,
                       should_print = excluded.should_print,
                       label_key = excluded.label_key,
                       created_at = datetime('now','localtime'),
                       cloud_ms = excluded.cloud_ms,
                       label_ms = excluded.label_ms,
                       total_ms = excluded.total_ms",
                )
                .bind(rid)
                .bind(&query_no)
                .bind(&info.shipping_no)
                .bind(&info.shipping_provider)
                .bind(&channel_code)
                .bind(&print_profile)
                .bind(1) // 雲端 v2 路徑表示「要列印」,固定寫 1
                .bind(&label_key)
                .bind(cloud_ms)
                .bind(label_ms)
                .bind(total_ms)
                .execute(&state.db)
                .await;
                use tauri::Emitter;
                let _ = state.app.emit("parcel-query-logged", ());

                // 讀碼站存證:把開頭釘住的那一幀丟背景寫檔 + 回寫 photo_path。
                // 此時 parcel_query_log 該列已 INSERT 完成(上面已 await),UPDATE by response_id 不會 race。
                // 抓不到幀(相機未啟用/未接)時 snapshot=None,整段略過,photo_path 保持 NULL。
                if let Some(jpeg) = snapshot.clone() {
                    let db = state.db.clone();
                    let captures_dir = state.captures_dir.clone();
                    let qn = query_no.clone();
                    let app = state.app.clone();
                    tokio::spawn(async move {
                        let key = tokio::task::spawn_blocking(move || {
                            crate::camera::save_snapshot(&captures_dir, &qn, &jpeg)
                        })
                        .await
                        .ok()
                        .flatten();
                        if let Some(key) = key {
                            let _ = sqlx::query(
                                "UPDATE parcel_query_log SET photo_path = ? WHERE response_id = ?",
                            )
                            .bind(&key)
                            .bind(rid)
                            .execute(&db)
                            .await;
                            let _ = app.emit("parcel-query-logged", ());
                        }
                    });
                }
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

            // 印單事件:source='ipc'(工控機 GET /api/parcel),sticker 由 channel_code 反查 sort_channels.job_sticker
            // 失敗不影響 API 回應(統計次要,不能干擾正常出單)
            let sticker_user: Option<String> = if let Some(code) = channel_code.as_deref() {
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
            let insert_res = sqlx::query(
                "INSERT INTO print_event (source, shipping_no, provider_code, sticker_user, channel_code)
                 VALUES ('ipc', ?, ?, ?, ?)",
            )
            .bind(&info.shipping_no)
            .bind(&info.shipping_provider)
            .bind(&sticker_user)
            .bind(&channel_code)
            .execute(&state.db)
            .await;
            if insert_res.is_ok() {
                crate::commands::print_stats_commands::emit_print_stats_updated(
                    &state.app,
                    "ipc",
                    &info.shipping_no,
                );
            }

            // 分揀袋件核對:新袋背景 examine 取整袋清單,舊袋就地更新列印時間(非阻塞,不讓工控機等雲端)
            state.bag_check.on_parcel(
                info.package_sn.clone(),
                &info.order_sn,
                &info.shipping_no,
                &info.shipping_provider,
            );

            Ok(Json(DataEnvelope::new(ParcelData {
                channel_code,
                print_profile,
                label_path,
                response_id: info.response_id,
                is_error_label: false,
                error_code: None,
                message: None,
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
            emit_parcel_alert(
                &state.app,
                "unauthorized",
                "雲端未登入,請先在桌面 App 完成登入",
                &query_no,
            );
            Err(err_resp(
                StatusCode::UNAUTHORIZED,
                "雲端未登入,請先在桌面 App 完成登入",
            ))
        }
        Err(e) => {
            let cloud_ms = t_start.elapsed().as_millis() as i64;
            let _ = sqlx::query(
                "INSERT INTO daily_stats (date, request_count)
                 VALUES (date('now'), 1)
                 ON CONFLICT(date) DO UPDATE SET request_count = request_count + 1",
            )
            .execute(&state.db)
            .await;
            // 依雲端錯誤訊息分類,讓桌面前端播放對應提示音(門市關轉 / 未確認 / 找不到 / 一般失敗)
            let (kind, msg, err_code, err_provider, err_shipping_no) = match &e {
                AppError::Cloud { code, message, shipping_provider, shipping_no } => (
                    classify_parcel_alert(code),
                    message.clone(),
                    code.clone(),
                    shipping_provider.clone(),
                    shipping_no.clone(),
                ),
                other => ("error", other.to_string(), "ERROR".to_string(), None, None),
            };
            emit_parcel_alert(&state.app, kind, &msg, &query_no);

            // 雲端帶出物流商代碼時(查得到訂單的業務錯誤,如 STORE_CLOSED / UNCONFIRMED),
            // 錯誤面單照正常面單流程解析分揀通道與 print_profile;
            // 查不到物流商時(NOT_FOUND / 雲端連線失敗)統一退回「未指派通道代碼」,
            // 讓所有錯誤面單只要有設 fallback 就一定有格口可分揀
            let (channel_code, print_profile) = match err_provider.as_deref() {
                Some(p) => {
                    let (cc, _) = resolve_channel_code(&state.db, &state.rr, p).await;
                    (cc, fetch_print_profile(&state.db, p).await)
                }
                None => (fetch_unassigned_channel_code(&state.db).await, None),
            };

            // 記錄雲端查件異常(門市關轉等),供手機 / 桌面回看清單
            // shipping_no 為雲端帶出的精確物流單號(查得到訂單才有),與工控機掃的 query_no 區分
            let _ = sqlx::query(
                "INSERT INTO parcel_alert (kind, code, query_no, message, channel_code, shipping_no, created_at)
                 VALUES (?, ?, ?, ?, ?, ?, datetime('now','localtime'))",
            )
            .bind(kind)
            .bind(&err_code)
            .bind(&query_no)
            .bind(&msg)
            .bind(&channel_code)
            .bind(&err_shipping_no)
            .execute(&state.db)
            .await;

            // 錯誤面單(取向 A):產生提示圖,依面單路徑模式決定出口 —
            //   direct_print : 中介 PC 本機直接列印(label_path 回 null)
            //   local/share/http : 寫入 cache 後回 label_path,讓工控機如同一般面單自行列印
            // 這樣 http 模式(工控機跨機、本機無印表機)也能在分揀線印出錯誤面單,
            // 不再只有 direct_print 模式能用。
            let t_label = std::time::Instant::now();
            let label_bytes = crate::error_label::generate(
                &query_no,
                &err_code,
                crate::error_label::LabelHeight::H100mm,
            );
            let (label_path, err_label_key) = if state.label_resolver.current_mode()
                == LabelPathMode::DirectPrint
            {
                spawn_print_error_label_bytes(
                    state.db.clone(),
                    state.app.clone(),
                    query_no.clone(),
                    label_bytes,
                    err_provider.clone(),
                );
                (None, None)
            } else {
                let cache_base = state.cache.base_dir();
                match write_error_label_to_cache(&cache_base, &query_no, &err_code, &label_bytes)
                    .await
                {
                    Some(key) => {
                        let local_abs = cache_base.join(&key);
                        let resolved = state.label_resolver.resolve(
                            &local_abs,
                            &cache_base,
                            &key,
                            Some(host.as_str()),
                        );
                        (Some(resolved), Some(key))
                    }
                    None => {
                        // 寫檔失敗 → 退回舊行為:本機嘗試列印 + 回 502,不讓工控機誤以為有面單
                        emit_error_label_failed(&state.app, &query_no, "cache_write_failed");
                        spawn_print_error_label_bytes(
                            state.db.clone(),
                            state.app.clone(),
                            query_no.clone(),
                            label_bytes,
                            err_provider.clone(),
                        );
                        return Err(err_resp(StatusCode::BAD_GATEWAY, e.to_string()));
                    }
                }
            };
            let label_ms = t_label.elapsed().as_millis() as i64;

            // 錯誤面單也寫入查詢記錄:response_id 用本地負數遞減(與雲端正數 ID 區隔),
            // 工控機照正常流程 POST /api/report 才反查得到(負數回報只記錄、不推雲端 webhook);
            // 查詢記錄頁同時也看得到錯誤查詢,不再是診斷盲區。
            // 寫入失敗時退回 response_id=null(工控機不回報,同 v0.5.5 行為),不影響出單
            let total_ms = t_start.elapsed().as_millis() as i64;
            let response_id: Option<i64> = match sqlx::query_scalar::<_, i64>(
                "INSERT INTO parcel_query_log
                   (response_id, query_no, tracking_no, shipping_provider, sort_channel, print_profile, should_print, label_key, created_at, cloud_ms, label_ms, total_ms)
                 VALUES (
                   (SELECT COALESCE(MIN(response_id), 0) - 1 FROM parcel_query_log WHERE response_id < 0),
                   ?, '', ?, ?, ?, 1, ?, datetime('now','localtime'), ?, ?, ?)
                 RETURNING response_id",
            )
            .bind(&query_no)
            .bind(&err_provider)
            .bind(&channel_code)
            .bind(&print_profile)
            .bind(&err_label_key)
            .bind(cloud_ms)
            .bind(label_ms)
            .bind(total_ms)
            .fetch_one(&state.db)
            .await
            {
                Ok(rid) => {
                    use tauri::Emitter;
                    let _ = state.app.emit("parcel-query-logged", ());
                    Some(rid)
                }
                Err(e) => {
                    tracing::warn!(?e, query_no = %query_no, "錯誤面單查詢記錄寫入失敗");
                    None
                }
            };

            Ok(Json(DataEnvelope::new(ParcelData {
                channel_code,
                print_profile,
                label_path,
                response_id,
                is_error_label: true,
                error_code: Some(err_code),
                message: Some(msg),
            })))
        }
    }
}

/// POST /api/report — 工控機回報
/// 驗 response_id 存在於 parcel_query_log,通過後寫入本機 queue (status=pending);
/// 背景 worker 會推送 logistic-cat webhook + 追蹤 status/retry/last_error。
/// 負數 response_id = 錯誤面單(本地產生,雲端無對應列印記錄):驗證通過後只記錄、不推 webhook,
/// 讓工控機對正常/錯誤面單走完全相同的回報流程。
async fn post_report(
    State(state): State<ServerState>,
    Json(req): Json<ReportPayload>,
) -> impl IntoResponse {
    let row = match sqlx::query(
        "SELECT query_no, tracking_no, sort_channel FROM parcel_query_log WHERE response_id = ?",
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

    // 錯誤面單回報:雲端沒有對應列印記錄,推 webhook 只會失敗重試 → 只記事件即回 200
    if req.response_id < 0 {
        let query_no: String = row.try_get("query_no").unwrap_or_default();
        event_log::log_bg(state.db.clone(), "info", "server", "收到回報",
            format!("工控機回報錯誤面單已處理 query_no={query_no} (本地記錄,不推雲端)"));
        return (
            StatusCode::OK,
            Json(serde_json::json!({ "message": "OK" })),
        );
    }

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

    event_log::log_bg(state.db.clone(), "info", "server", "收到回報",
        format!("工控機回報已排隊 tracking_no={tracking_no}"));

    (
        StatusCode::OK,
        Json(serde_json::json!({ "message": "OK" })),
    )
}

/// POST /api/device-alert — 工控機回報設備異常(卡包裹 / USB 斷線 …)
///
/// 設計原則同 POST /api/report:**不讓工控機等** — 立即回 200,語音廣播由前端背景處理。
/// 工控機只負責「喊一聲」,不需要等廣播放完;前端收到 `device-alert` 事件後用
/// 雙語(中文 + 越南語)廣播一次提示現場人員,並顯示 toast。
async fn post_device_alert(
    State(state): State<ServerState>,
    Json(req): Json<DeviceAlertReq>,
) -> impl IntoResponse {
    // type 統一正規化成大寫(工控機傳大小寫皆可,canonical 一律大寫,對齊雲端機器碼風格)。
    // 省略或空字串 → 落入通用 "ERROR";不限制集合,前端 i18n 找不到對應碼會 fallback。
    let alert_type = req
        .alert_type
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_uppercase())
        .unwrap_or_else(|| "ERROR".to_string());
    let message = req.message.unwrap_or_default();

    // 立刻推給前端廣播一次(emit 失敗只 warn,不影響回應工控機)
    emit_device_alert(&state.app, &alert_type, &message);

    let detail = if message.trim().is_empty() {
        format!("工控機設備異常 type={alert_type}")
    } else {
        format!("工控機設備異常 type={alert_type} message={message}")
    };
    event_log::log_bg(state.db.clone(), "warn", "server", "設備異常", detail);

    (
        StatusCode::OK,
        Json(serde_json::json!({ "message": "OK" })),
    )
}
