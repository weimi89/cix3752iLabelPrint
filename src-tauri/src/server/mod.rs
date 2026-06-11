use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path as StdPath;
use std::sync::Arc;

use crate::event_log;

use axum::{
    extract::{Host, Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
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
    bag_check: BagCheckState,
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

    let state = ServerState {
        db,
        cloud,
        cache: cache.clone(),
        queue,
        rr: Arc::new(Mutex::new(HashMap::new())),
        label_resolver,
        watermark,
        bag_check,
        app,
        direct_print_tx,
    };

    let images_service = ServeDir::new(cache.base_dir());

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/api/parcel/:query_no", get(get_parcel))
        .route("/api/report", post(post_report))
        .route("/api/device-alert", post(post_device_alert))
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
    .bind(provider)
    .fetch_all(db)
    .await
    .unwrap_or_default();

    let codes: Vec<String> = rows
        .into_iter()
        .filter_map(|r| r.try_get::<Option<String>, _>("channel_code").ok().flatten())
        .collect();

    if codes.is_empty() {
        // 未設定指派物流時，使用 fallback 通道代碼
        (fetch_unassigned_channel_code(db).await, false)
    } else {
        let mut rr = rr.lock();
        let entry = rr.entry(provider.to_string()).or_insert(0);
        let idx = *entry % codes.len();
        *entry = (idx + 1) % codes.len();
        (Some(codes[idx].clone()), true)
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
    /// 廣播次數,可省略(預設 1);後端 clamp 到 1..=3
    repeat: Option<u32>,
}

/// 工控機設備異常推給前端的提示。前端 useDeviceAlert 依 `alert_type` 取雙語文案,
/// 播 `repeat` 次廣播提示現場人員,並顯示 toast。
#[derive(Serialize, Clone)]
struct DeviceAlert {
    alert_type: String,
    message: String,
    repeat: u32,
}

/// emit `device-alert` 給前端;失敗只記 warn,不影響回應工控機
fn emit_device_alert(app: &tauri::AppHandle, alert_type: &str, message: &str, repeat: u32) {
    use tauri::Emitter;
    let payload = DeviceAlert {
        alert_type: alert_type.to_string(),
        message: message.to_string(),
        repeat,
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

    // 1. 確保原圖已在本地快取
    let original_ok = if cache.has_local(&label_key) {
        let _ = cache.record_hit(&label_key).await;
        true
    } else {
        let _ = cache.record_miss().await;
        match cache.fetch_now(&label_key, &image_url).await {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!(label_key = %label_key, ?e, "direct_print 背景下載快取失敗");
                false
            }
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
    match state.cloud.fetch_parcel(&query_no).await {
        Ok(info) => {
            let cloud_ms = t_start.elapsed().as_millis() as i64;

            // 用 shipping_no 作為快取 key (檔名末段帶副檔名,從 shipping_image URL 推得)
            let label_key = derive_label_key(&info.shipping_image);
            let cache_base = state.cache.base_dir();

            let print_num = info.print_num.unwrap_or(0);

            // DirectPrint 模式:工控機拿到 label_path=null(由中介機列印),不需要圖檔本身 ──
            // 立即回應,圖檔下載 + 浮水印 + 列印全部丟背景,不讓工控機等雲端(設計原則 #2)。
            // 其餘模式(local/share/http):工控機要讀檔,必須同步下載到完成才回(設計原則 #3)。
            let (label_path, label_ms) =
                if state.label_resolver.current_mode() == LabelPathMode::DirectPrint {
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
                    // 第一階段:確保原圖已在本地快取(工控機要讀,同步下載到完成)
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
                "INSERT INTO print_event (source, shipping_no, provider_code, sticker_user)
                 VALUES ('ipc', ?, ?, ?)",
            )
            .bind(&info.shipping_no)
            .bind(&info.shipping_provider)
            .bind(&sticker_user)
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
            let (kind, msg, err_code, err_provider) = match &e {
                AppError::Cloud { code, message, shipping_provider } => (
                    classify_parcel_alert(code),
                    message.clone(),
                    code.clone(),
                    shipping_provider.clone(),
                ),
                other => ("error", other.to_string(), "ERROR".to_string(), None),
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
/// 工控機只負責「喊一聲」,不需要等廣播放完;前端收到 `device-alert` 事件後用 TTS
/// 雙語(中文 + 越南語)重複廣播 N 次提示現場人員,並顯示 toast。
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
    // 廣播次數:省略 → 1;clamp 到 1..=3(上限 3,避免工控機誤帶大數造成連續廣播洗版)
    let repeat = req.repeat.unwrap_or(1).clamp(1, 3);

    // 立刻推給前端廣播(emit 失敗只 warn,不影響回應工控機)
    emit_device_alert(&state.app, &alert_type, &message, repeat);

    let detail = if message.trim().is_empty() {
        format!("工控機設備異常 type={alert_type} repeat={repeat}")
    } else {
        format!("工控機設備異常 type={alert_type} repeat={repeat} message={message}")
    };
    event_log::log_bg(state.db.clone(), "warn", "server", "設備異常", detail);

    (
        StatusCode::OK,
        Json(serde_json::json!({ "message": "OK" })),
    )
}
