use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path as StdPath, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use crate::event_log;

mod assets;
pub(crate) mod auth;
mod events;
mod rpc;

use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
use tower_http::compression::{predicate::{NotForContentType, Predicate, SizeAbove}, CompressionLayer};
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
use crate::queue::{CancelOutcome, QueueManager};
use crate::watermark::{derive_repeat_key, WatermarkRenderer};
use crate::bag_check::BagCheckState;
use crate::{AppError, AppResult};

/// 面單路徑解析器:依設定把本地絕對路徑轉成 local / share / http 三種形態
#[derive(Clone)]
pub struct LabelPathResolver {
    inner: Arc<RwLock<LabelPathConfig>>,
    /// 純分揀模式旗標(獨立於面單路徑模式)。開啟時 `get_parcel` 完全不產出/回傳面單、不記印單。
    /// 與面單路徑模式同放此解析器:兩者都在每筆 `get_parcel` 決定「面單輸出」行為,
    /// 且共用同一條 `update_config` 熱套用路徑(config_commands 只呼叫一次 `apply_config`)。
    sort_only: Arc<AtomicBool>,
    /// 錯誤面單開關。關閉(預設)時 `get_parcel` 遇雲端業務錯誤只回 `error_code`,
    /// 不產出提示面單、也不回分揀通道。同放此解析器的理由同 `sort_only`。
    error_label: Arc<AtomicBool>,
}

impl LabelPathResolver {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            inner: Arc::new(RwLock::new(config.label_path.clone())),
            sort_only: Arc::new(AtomicBool::new(config.sort_only.enabled)),
            error_label: Arc::new(AtomicBool::new(config.error_label.enabled)),
        }
    }

    pub fn apply_config(&self, config: &AppConfig) {
        *self.inner.write() = config.label_path.clone();
        self.sort_only.store(config.sort_only.enabled, Ordering::Relaxed);
        self.error_label.store(config.error_label.enabled, Ordering::Relaxed);
    }

    pub fn current_mode(&self) -> LabelPathMode {
        self.inner.read().mode
    }

    /// 純分揀模式是否開啟(熱套用,即時反映設定變更)
    pub fn is_sort_only(&self) -> bool {
        self.sort_only.load(Ordering::Relaxed)
    }

    /// 錯誤面單是否開啟(熱套用,即時反映設定變更)
    pub fn is_error_label_enabled(&self) -> bool {
        self.error_label.load(Ordering::Relaxed)
    }

    /// DirectPrint 自補回報前,等工控機回報的寬限秒數(熱套用,改設定不需重啟 server)。
    /// **在此夾上限**(見 [`MAX_REPORT_DELAY_SECS`]):設定檔可被手改成任意值,
    /// 而過大的位移會讓 SQLite `datetime()` 回 NULL、被 worker 當成「立即可送」——
    /// 「等久一點」變成「馬上送」是最難察覺的那種錯,夾在單一出口最保險。
    pub fn report_delay_secs(&self) -> u64 {
        self.inner
            .read()
            .direct_print_report_delay_secs
            .min(crate::queue::MAX_REPORT_DELAY_SECS)
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

/// 本進程分配出去的排序值基準:一定大於任何 print_event.id,
/// 讓「這次跑起來之後分過的」永遠排在啟動前的歷史之後。
const RUNTIME_SEQ_BASE: i64 = 1 << 40;

/// 一個格口最近一次收件的狀態
#[derive(Clone)]
struct ChannelLast {
    /// 收件先後的排序值,越大越新。啟動前的歷史取 print_event.id,本進程分配取 RUNTIME_SEQ_BASE + 序號。
    /// 0 = 這個格口沒收過任何件,排最前面(最該輪到它)。
    seq: i64,
    /// 最近一次收到的配送單號,用來比對後兩碼。None = 沒有歷史。
    no: Option<String>,
}

/// 分揀分配的跨請求狀態。
/// 用 tokio async Mutex(非 parking_lot):resolve_channel_code 需在持鎖期間 await DB
/// 做原子 skip 消耗,把「讀-判定-扣減」全序列化,杜絕並發 double-skip 競態。
/// 各格口的最近收件狀態共用同一把鎖:選格口時排序與尾碼要一起看,拆兩把會在並發下
/// 讀到半新半舊的組合,選出錯的格口。
#[derive(Default)]
struct SortRouting {
    /// 通道代碼 → 最近一次收件狀態。缺項代表還沒回查過 DB。
    last: HashMap<String, ChannelLast>,
    /// 本進程已分配次數,作為收件先後的排序值
    seq: i64,
}

type SortRoutingState = Arc<tokio::sync::Mutex<SortRouting>>;

#[derive(Clone)]
struct ServerState {
    db: DbPool,
    cloud: CloudClient,
    cache: CacheManager,
    queue: QueueManager,
    routing: SortRoutingState,
    label_resolver: LabelPathResolver,
    watermark: WatermarkRenderer,
    bag_check: BagCheckState,
    camera: CameraManager,
    /// 讀碼站存證目錄(獨立於面單快取,server 啟動時由 config 解析定版;存檔與 /captures 服務共用)
    captures_dir: PathBuf,
    app: tauri::AppHandle,
    /// server 即將關閉的通知:長連線(SSE / MJPEG)訂閱它主動收線,
    /// 否則 graceful shutdown 會等不到它們結束(見 ServerHandle::shutdown)
    close_tx: broadcast::Sender<()>,
    /// DirectPrint 模式有序列印佇列:get_parcel 把工作丟進來,由單一 worker 逐筆 FIFO 處理。
    /// 保證列印順序 = 請求順序,且同時只有一筆在送印(不並發打 spooler)。
    direct_print_tx: mpsc::UnboundedSender<DirectPrintJob>,
}

/// DirectPrint 一筆待列印工作(下載 + 浮水印 + 送本機印表機所需的最小資料)
struct DirectPrintJob {
    label_key: String,
    image_url: String,
    /// 物流商代碼:浮水印位置(順豐右下角)與 repeat key 推導仍依物流商
    provider: String,
    /// 分配到的分揀通道代碼(僅供 log / 診斷,不再拿來反查印表機)
    channel_code: String,
    /// **入列當下就解析好的印表機名稱**,不在 worker 端才反查 ——
    /// 佇列積壓期間操作員若在「分揀通道」頁改了通道代碼,延後反查會整批落空、
    /// 全部被當 no_printer 丟棄(工控機早已收 200、袋核對已標已印,實體卻沒印)。
    printer_name: String,
    print_num: u32,
    query_no: String,
    /// 雲端列印記錄 ID:**列印成功後**用它補一筆回報進 report_queue(直印模式下工控機多半不回報,
    /// 這是貼標人員唯一能送達雲端的路)。雲端 debug 模式等未回 id 時為 None,此時無從配對、不補記。
    response_id: Option<i64>,
    /// 面單單號(= shipping_no):補記回報時一併寫入,佇列歷史頁直接顯示,免再 join
    tracking_no: String,
    /// 入列當下解析到的貼標人員(與 print_event 同一份來源,確保兩張表對得起來)
    job_sticker: Option<String>,
}

pub struct ServerHandle {
    pub bind_addr: String,
    handle: JoinHandle<()>,
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
    /// 通知長連線(事件 SSE、相機 MJPEG 預覽)主動收線
    close_tx: broadcast::Sender<()>,
}

impl ServerHandle {
    /// 主動關閉 HTTP server。
    ///
    /// axum 的 graceful shutdown 會等到**所有連線結束**才返回,而事件 SSE 與相機預覽
    /// 是永遠不會自己結束的長連線 —— 只要有人開著網頁看板或設定頁的相機預覽,
    /// 「重啟伺服器」就會一直等下去。更糟的是觸發重啟的那個請求往往正好跑在這台
    /// server 上(網頁版按重啟、或存下改了 port 的設定),等於自己等自己結束。
    ///
    /// 因此先請長連線離場,再等 graceful shutdown,最後用逾時強制收工。
    pub async fn shutdown(self) {
        // 沒有任何長連線時 send 會回 Err,屬正常狀態
        let _ = self.close_tx.send(());
        let _ = self.shutdown_tx.send(());

        let mut handle = self.handle;
        if tokio::time::timeout(std::time::Duration::from_secs(3), &mut handle)
            .await
            .is_err()
        {
            // 保底:仍有連線賴著不走(例如卡住的上傳)。不強制中止的話 port 不會釋放,
            // 接著要綁同一個 port 的新 server 會失敗,整台服務起不來。
            tracing::warn!("HTTP server 未在 3 秒內收工,強制中止以釋放 port");
            handle.abort();
        }
    }
}

/// 啟動 axum HTTP server,回傳一個可以關閉它的 handle
/// 啟動本地 HTTP server。
///
/// 回傳刻意 box 成 trait object 而非 `async fn` 的 `impl Future`:router 裡的 `/rpc`
/// 能呼叫到 `server_restart`,而它又會回頭呼叫本函式,形成型別上的自我遞迴。
/// `async fn` 的 `Send` 推導無法收斂這種循環(編譯器報 "cannot satisfy impl Future: Send"),
/// box 成 `dyn Future + Send` 等於由 trait bound 直接斷言,推導就此打住。
pub fn start(
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
) -> std::pin::Pin<Box<dyn std::future::Future<Output = AppResult<ServerHandle>> + Send>> {
    let config = config.clone();
    Box::pin(start_inner(
        config,
        db,
        cloud,
        cache,
        queue,
        label_resolver,
        watermark,
        bag_check,
        camera,
        app,
    ))
}

#[allow(clippy::too_many_arguments)]
async fn start_inner(
    config: AppConfig,
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
        let app = app.clone();
        let queue = queue.clone();
        let resolver = label_resolver.clone();
        tokio::spawn(async move {
            while let Some(job) = direct_print_rx.recv().await {
                run_direct_print_job(&cache, &watermark, &db, &app, &queue, &resolver, job).await;
            }
        });
    }

    // 存證目錄:啟動時依 config 解析定版(與面單快取分離)。存檔與 /captures 服務共用同一份,確保一致。
    let captures_dir = config.resolved_captures_dir(&app)?;
    // /images 服務目錄:用 CacheManager 同一套「安全解析」(驗證 + app_data fallback),
    // **不可**各自 resolved_cache_dir —— 壞設定(legacy ~/Pictures / 拔除的磁碟)下會 split-brain:
    // 下載寫 fallback、/images 供壞目錄 → 面單全 404,甚至把使用者資料夾以 HTTP 曝露給整個區網。
    // 由 config(而非 cache.base_dir() 當下值)解析,讓「改快取目錄 → 重啟 server」直接供新目錄。
    let images_dir = CacheManager::resolve_safe_dir(&app, &config)?;

    // 長連線(事件 SSE、相機 MJPEG 預覽)的收線通知,關閉 server 時廣播(見 ServerHandle::shutdown)
    let (close_tx, _) = broadcast::channel::<()>(1);

    let state = ServerState {
        db,
        cloud,
        cache: cache.clone(),
        queue,
        routing: Arc::new(tokio::sync::Mutex::new(SortRouting::default())),
        label_resolver,
        watermark,
        bag_check,
        camera,
        captures_dir: captures_dir.clone(),
        app,
        close_tx: close_tx.clone(),
        direct_print_tx,
    };

    let images_service = ServeDir::new(images_dir);
    let captures_service = ServeDir::new(&captures_dir);

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/api/parcel/{query_no}", get(get_parcel))
        .route("/api/report", post(post_report))
        .route("/api/device-alert", post(post_device_alert))
        // 手機遙控分揀通道暫停(換紙等臨時暫停某通道,不影響其他通道)
        .route("/control", get(control_page))
        // 分揀看板:/board 是網頁版(大螢幕開網址),/board/stream 是它的即時推播
        .route("/board", get(board_page))
        .route("/board/stream", get(events::board_stream))
        // 網頁版事件通道:桌面 listen() 在這裡有一條同名對應
        .route("/events/stream", get(events::events_stream))
        .route("/api/alerts", get(list_alerts))
        .route("/api/channels", get(list_channels))
        .route("/api/channels/{position}", post(set_channel_enabled))
        .route("/api/channels/{position}/skip", post(skip_channel))
        .route("/api/channels/{position}/recent", get(channel_recent))
        .route("/api/channels/{position}/assign", post(assign_channel))
        .route("/api/dispatch-providers", get(list_dispatch_providers))
        .route("/api/sticker-history", get(list_sticker_history))
        // 網頁版資料通道:桌面的 invoke 在這裡有一支同名對應
        .route("/rpc/{command}", post(rpc::rpc_handler))
        .route("/camera/preview", get(camera_preview))
        .route("/camera/preview/stream", get(camera_preview_stream))
        .nest_service("/images", images_service)
        .nest_service("/captures", captures_service)
        // 登入相關:自身不能被登入中介層擋住,否則外網永遠登不進來
        .route("/auth/status", get(auth::status))
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        // 網頁版前端:以上都沒中的路徑交給它,含前端路由的 SPA fallback
        .fallback(assets::serve)
        // 存取控制:內網放行、外網要密碼、工控機端點只認內網(見 auth.rs)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::guard,
        ))
        // 壓縮回應:網頁版前端未壓縮約 5 MB,現場手機走 Wi-Fi 開起來很慢。
        //
        // 刻意排除兩種長連線:事件 SSE 與相機 MJPEG 預覽。壓縮器要累積到一定量才吐資料,
        // 套在串流上會讓事件延遲送達、預覽畫面一頓一頓 —— 兩者都是「即時」才有意義的東西。
        //(SizeAbove 之外還要自己排 multipart:內建的 DefaultPredicate 只排掉 SSE。)
        .layer(
            CompressionLayer::new().compress_when(
                SizeAbove::new(512)
                    .and(NotForContentType::SSE)
                    .and(NotForContentType::IMAGES)
                    .and(NotForContentType::new("multipart/"))
                    // 音檔(設備異常的預錄語音)本身就是壓縮格式,再壓一次省不到什麼,
                    // 只是多花 CPU 又延後送達 —— 而這些是要立刻播給現場人員聽的
                    .and(NotForContentType::new("audio/"))
                    .and(NotForContentType::new("video/")),
            ),
        )
        .with_state(state)
        // 不設 CORS:網頁版與後端同源,工控機是機器對機器(不受瀏覽器同源政策約束),
        // 桌面端的圖片走 <img> 也不需要。先前的 permissive 等於允許任何網站的 JS
        // 對這台機器發請求,對外開放後風險不成比例。
        .layer(TraceLayer::new_for_http());

    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| AppError::Server(format!("無法綁定 {addr}: {e}")))?;

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    // into_make_service_with_connect_info:讓中介層取得 TCP 對端位址。
    // 內外網的判斷完全依賴它 —— 少了這行,ConnectInfo 抽取失敗會讓所有請求被擋。
    let server_future = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
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
        close_tx,
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
    /// 待跳過本輪次數(輪到它時消耗一次)
    skip_count: i64,
    /// 該通道最近一筆分到的物流單號(print_event.shipping_no,供現場對單)
    last_tracking: Option<String>,
}

/// GET /api/channels — 列出 10 個分揀通道與啟用狀態(手機控制頁輪詢用)
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
    let _ = crate::event_bridge::emit(
        &state.app,
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

// =====================================================================
// 手機遙控:通道指派(貼標人員 / 指派物流)
// =====================================================================

/// GET /api/dispatch-providers — 物流商清單(手機端「指派物流」的選項來源)
async fn list_dispatch_providers(State(state): State<ServerState>) -> impl IntoResponse {
    let rows = sqlx::query("SELECT code, name FROM dispatch_provider ORDER BY sort_order, code")
        .fetch_all(&state.db)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!(?e, "list_dispatch_providers 讀取失敗,回空清單");
            Vec::new()
        });
    let list: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            let code: String = r.try_get("code").unwrap_or_default();
            let name: String = r.try_get("name").unwrap_or_default();
            serde_json::json!({ "code": code, "name": name })
        })
        .filter(|v| !v["code"].as_str().unwrap_or_default().is_empty())
        .collect();
    Json(list)
}

/// GET /api/sticker-history — 人員歷史名單(與桌面「分揀通道 / 掃描列印」共用同一份)
async fn list_sticker_history(State(state): State<ServerState>) -> impl IntoResponse {
    let rows = sqlx::query("SELECT name FROM sticker_history ORDER BY used_at DESC LIMIT 200")
        .fetch_all(&state.db)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!(?e, "list_sticker_history 讀取失敗,回空清單");
            Vec::new()
        });
    let list: Vec<String> = rows
        .iter()
        .filter_map(|r| r.try_get::<String, _>("name").ok())
        .filter(|n| !n.trim().is_empty())
        .collect();
    Json(list)
}

/// 手機可改的通道指派內容。刻意不含通道代碼 / 印表機 —— 那兩項改錯會讓整條線分錯格口
/// 或靜默漏印,留桌面端統一管理;手機只改現場每班會換的貼標人員與指派物流。
#[derive(Deserialize)]
struct AssignBody {
    #[serde(default)]
    job_sticker: Option<String>,
    #[serde(default)]
    dispatch_codes: Vec<String>,
}

/// 錯誤同時帶機器可讀 code 與中文訊息:手機控制頁是中越雙語,
/// 越南語操作員看 code 對照到自己語言的說明,對不到才退回顯示中文原文。
fn assign_err(status: StatusCode, code: &str, msg: String) -> axum::response::Response {
    (status, Json(serde_json::json!({ "error": msg, "code": code }))).into_response()
}

/// POST /api/channels/:position/assign — 手機設定該通道的貼標人員與指派物流
async fn assign_channel(
    State(state): State<ServerState>,
    Path(position): Path<String>,
    Json(body): Json<AssignBody>,
) -> impl IntoResponse {
    use crate::commands::sort_channel_commands::{upsert_sticker_history, POSITIONS};
    

    if !POSITIONS.contains(&position.as_str()) {
        return assign_err(
            StatusCode::BAD_REQUEST,
            "BAD_POSITION",
            format!("無效的通道位置: {position}"),
        );
    }

    let job_sticker = body
        .job_sticker
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    // 去重 + 去空白,保留送來的順序(與桌面 sort_channel_save 同語意)
    let mut dispatch_codes: Vec<String> = Vec::new();
    for code in body.dispatch_codes {
        let code = code.trim().to_string();
        if !code.is_empty() && !dispatch_codes.contains(&code) {
            dispatch_codes.push(code);
        }
    }

    let row = match sqlx::query(
        "SELECT channel_code, printer_name FROM sort_channels WHERE position = ?",
    )
    .bind(&position)
    .fetch_optional(&state.db)
    .await
    {
        Ok(r) => r,
        Err(e) => return assign_err(StatusCode::INTERNAL_SERVER_ERROR, "DB_ERROR", e.to_string()),
    };
    let Some(row) = row else {
        return assign_err(
            StatusCode::NOT_FOUND,
            "NO_CHANNEL",
            format!("找不到通道 {position}"),
        );
    };
    let norm = |v: Result<Option<String>, sqlx::Error>| {
        v.ok()
            .flatten()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let channel_code = norm(row.try_get::<Option<String>, _>("channel_code"));
    let printer_name = norm(row.try_get::<Option<String>, _>("printer_name"));

    // 送來的物流代碼必須真的存在 —— 手機的選項清單可能是「刪掉某物流商之前」抓的,
    // 放行會讓該通道指到不存在的物流,現場看起來有指派、實際永遠分不到件。
    if !dispatch_codes.is_empty() {
        let known: std::collections::HashSet<String> =
            match sqlx::query("SELECT code FROM dispatch_provider")
                .fetch_all(&state.db)
                .await
            {
                Ok(rows) => rows
                    .iter()
                    .filter_map(|r| r.try_get::<String, _>("code").ok())
                    .collect(),
                Err(e) => {
                    return assign_err(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "DB_ERROR",
                        e.to_string(),
                    )
                }
            };
        if let Some(bad) = dispatch_codes.iter().find(|c| !known.contains(*c)) {
            return assign_err(
                StatusCode::BAD_REQUEST,
                "UNKNOWN_PROVIDER",
                format!("物流代碼 \"{bad}\" 不存在,請重新整理後再試"),
            );
        }
    }

    // direct_print 模式下「會實際接件卻沒設印表機」的通道,每一件都會靜默漏印
    //(工控機收 200、統計與袋核對都記成已印,實體沒印),故擋在指派當下 ——
    // 與桌面「分揀通道」頁存檔前的同一道防護,只是這裡由後端把關。
    //
    // 只擋「這次真的多接了物流」。手機的草稿是拿通道現有指派開的,只改貼標人員也會
    // 原樣送回整份物流清單;若連這種請求都擋,站點改成 direct_print 後(印表機還沒設),
    // 換班的人連改個名字都會被擋、還被叫去桌面設印表機,而他根本沒動物流。
    // 移除物流同理放行:只會少接件,不會生出新的漏印。
    let adds_dispatch = if dispatch_codes.is_empty() {
        false
    } else {
        let existing: std::collections::HashSet<String> = sqlx::query(
            "SELECT dispatch_code FROM sort_channel_dispatch WHERE position = ?",
        )
        .bind(&position)
        .fetch_all(&state.db)
        .await
        .unwrap_or_else(|e| {
            // 查不到現況時無從判斷「有沒有新增」,一律當成有新增(寧可擋下請人去設印表機,
            // 也不要放行一個可能每件都靜默漏印的通道)
            tracing::warn!(?e, %position, "指派:讀取現有物流指派失敗,本次以「有新增」處理");
            Vec::new()
        })
        .iter()
        .filter_map(|r| r.try_get::<String, _>("dispatch_code").ok())
        .collect();
        dispatch_codes.iter().any(|c| !existing.contains(c))
    };

    if state.label_resolver.current_mode() == LabelPathMode::DirectPrint
        && channel_code.is_some()
        && adds_dispatch
        && printer_name.is_none()
    {
        return assign_err(
            StatusCode::BAD_REQUEST,
            "PRINTER_REQUIRED",
            format!("通道 {position} 尚未設定本機印表機,請先在桌面「分揀通道」頁設定後再指派物流"),
        );
    }

    // 通道本身與多對多指派一起寫,交易保證原子性(避免刪了舊指派卻沒寫入新指派)
    let tx_res = async {
        let mut tx = state.db.begin().await?;
        sqlx::query(
            "UPDATE sort_channels SET job_sticker = ?, updated_at = datetime('now','localtime')
             WHERE position = ?",
        )
        .bind(&job_sticker)
        .bind(&position)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM sort_channel_dispatch WHERE position = ?")
            .bind(&position)
            .execute(&mut *tx)
            .await?;
        for code in &dispatch_codes {
            sqlx::query(
                "INSERT OR IGNORE INTO sort_channel_dispatch (position, dispatch_code)
                 VALUES (?, ?)",
            )
            .bind(&position)
            .bind(code)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok::<(), sqlx::Error>(())
    }
    .await;
    if let Err(e) = tx_res {
        return assign_err(StatusCode::INTERNAL_SERVER_ERROR, "DB_ERROR", e.to_string());
    }

    // 人員歷史名單與桌面共用:手機新填的名字,桌面下拉也選得到
    if let Some(name) = job_sticker.as_deref() {
        if let Err(e) = upsert_sticker_history(&state.db, name).await {
            tracing::warn!(?e, name, "手機指派:寫入人員歷史名單失敗(不影響本次指派)");
        }
    }

    // 廣播給桌面分揀通道頁即時同步(payload 帶哪幾項,桌面就只套用哪幾項)
    let _ = crate::event_bridge::emit(
        &state.app,
        "sort-channel-updated",
        serde_json::json!({
            "position": position,
            "job_sticker": job_sticker,
            "dispatch_codes": dispatch_codes,
        }),
    );

    Json(serde_json::json!({
        "position": position,
        "job_sticker": job_sticker,
        "dispatch_codes": dispatch_codes,
    }))
    .into_response()
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
    // 這條和事件 SSE 一樣是永不自己結束的長連線,同樣要訂閱收線通知。
    // 少了它,設定頁開著預覽時去重啟 server(改 port、改存證目錄都會),
    // graceful shutdown 等不到這條連線,每次都得撐滿逾時走強制中止 ——
    // 那條路徑是設計來當保底的,不該變成常態。
    let close_rx = state.close_tx.subscribe();
    let stream = futures::stream::unfold((camera, close_rx), |(camera, mut close_rx)| async move {
        // ~10fps:對位用足夠順;與擷取迴圈同速率,不額外吃 CPU
        tokio::select! {
            _ = close_rx.recv() => return None,
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {}
        }
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
        Some((Ok::<Vec<u8>, std::io::Error>(chunk), (camera, close_rx)))
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

/// GET /board — 導向網頁版的分揀看板。
///
/// 舊版是一份獨立手刻的 HTML;功能併入網頁版之後改為導向,讓貼在電視上的舊網址、
/// 掃過的 QR 仍然指得到東西 —— 現場不必為了改版重貼一輪。
async fn board_page() -> impl IntoResponse {
    axum::response::Redirect::to("/#/sort-board")
}

/// GET /control — 導向網頁版的分揀通道頁(原手機遙控頁的功能所在)
async fn control_page() -> impl IntoResponse {
    axum::response::Redirect::to("/#/sort-channels")
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

/// 取配送單號的後兩碼(統一大寫)。不足兩碼回 None —— 比不出後兩碼就不套用尾碼迴避。
fn tail2(no: &str) -> Option<String> {
    let chars: Vec<char> = no.trim().chars().collect();
    if chars.len() < 2 {
        return None;
    }
    Some(
        chars[chars.len() - 2..]
            .iter()
            .collect::<String>()
            .to_uppercase(),
    )
}

/// 該格口最近一次收件的狀態(收件先後 + 單號)。
/// 記憶體沒有紀錄(進程剛啟動)時回查一次列印記錄補上:不補的話每次重開 App,
/// 分配順序與尾碼迴避都會從頭來過 —— 剛收過件的格口會被當成整天沒收件,連著再收一件。
async fn channel_last(
    db: &DbPool,
    routing: &mut SortRouting,
    channel_code: &str,
) -> ChannelLast {
    if !routing.last.contains_key(channel_code) {
        let row = sqlx::query(
            "SELECT id, shipping_no FROM print_event
              WHERE channel_code = ? AND shipping_no IS NOT NULL AND shipping_no <> ''
              ORDER BY id DESC LIMIT 1",
        )
        .bind(channel_code)
        .fetch_optional(db)
        .await
        .ok()
        .flatten();
        let last = match row {
            Some(r) => ChannelLast {
                seq: r.try_get::<i64, _>("id").unwrap_or(0),
                no: r.try_get::<String, _>("shipping_no").ok(),
            },
            None => ChannelLast { seq: 0, no: None },
        };
        routing.last.insert(channel_code.to_string(), last);
    }
    routing.last.get(channel_code).cloned().unwrap_or(ChannelLast { seq: 0, no: None })
}

/// 消耗一次該通道的「跳過本輪」額度。回傳 true 代表這次真的扣到、該通道本輪不參與分配。
async fn consume_skip(db: &DbPool, position: &str) -> bool {
    sqlx::query(
        "UPDATE sort_channels SET skip_count = skip_count - 1 WHERE position = ? AND skip_count > 0",
    )
    .bind(position)
    .execute(db)
    .await
    .map(|r| r.rows_affected() > 0)
    .unwrap_or(false)
}

/// 依物流商代碼解析分揀通道:有指派通道時挑最久沒收件的那個(公平輪替);
/// 未指派任何通道時退回 fallback「未指派通道代碼」設定(settings.unassigned_channel_code)。
/// 回傳 `(channel_code, has_assigned)`,`has_assigned=false` 代表該物流商沒有任何指派通道。
/// 正常面單與錯誤面單共用,確保兩者分揀行為一致。
///
/// `shipping_no` 是本件的配送單號,用來避開「同一格口連著兩張後兩碼相同的單」——
/// 作業員貼單就是靠後兩碼確認手上的單對得上包裹,連兩張尾碼一樣會分不出誰是誰。
async fn resolve_channel_code(
    db: &DbPool,
    routing: &SortRoutingState,
    provider: &str,
    shipping_no: Option<&str>,
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
    // 尾碼迴避只在「這家物流有兩個以上可用通道」時成立 —— 只有一格時無處可換,
    // 硬迴避會變成該件不分配。單號短於兩碼也不套用(比不出後兩碼)。
    let tail = if n >= 2 { shipping_no.and_then(tail2) } else { None };

    // 全程持 async 鎖跨 await:把「排序選位 + skip 原子消耗」序列化,杜絕並發 double-skip。
    // skip 消耗用條件 UPDATE(WHERE skip_count > 0)+ rows_affected 判定:
    // 真的扣到一次才視為「跳過此通道」,扣不到(額度已被其他請求用盡)就選它 —— 不依賴鎖外快照。
    let chosen: Option<String> = {
        let mut rt = routing.lock().await;

        // 依「最久沒收件」排序,平手時照 L1→R5 的固定順序 ——
        // 排序值看的是格口實際收了什麼,不分物流。共用格口(例如同時掛 7-11 與全家)
        // 剛被別家的件用掉,下一件就會讓給比較閒的格口,現場各格口的量才會平均。
        let mut order: Vec<usize> = (0..n).collect();
        let mut seqs: Vec<i64> = Vec::with_capacity(n);
        for (_, code) in &candidates {
            seqs.push(channel_last(db, &mut rt, code).await.seq);
        }
        order.sort_by_key(|&i| (seqs[i], i));

        let mut picked: Option<String> = None;
        // 只因尾碼撞號而讓過的候選(依上面的排序)。它們的 skip 額度尚未消耗,
        // 第二輪回頭挑時才扣 —— 先扣會把「跳過本輪」的額度浪費在根本沒分給它的那次。
        let mut tail_blocked: Vec<usize> = Vec::new();

        for &idx in &order {
            let (pos, code) = &candidates[idx];
            if let Some(t) = tail.as_deref() {
                let last_tail = channel_last(db, &mut rt, code).await.no.and_then(|no| tail2(&no));
                if last_tail.as_deref() == Some(t) {
                    tail_blocked.push(idx);
                    continue;
                }
            }
            if consume_skip(db, pos).await {
                // 該通道本輪待跳過:已原子消耗一次,改看下一個
                continue;
            }
            picked = Some(code.clone());
            break;
        }

        // 尾碼迴避只跑一輪:繞完一圈每個候選不是撞尾碼就是待跳過,就不再堅持,
        // 回頭在「只因撞尾碼而讓過」的候選裡照同一個順序挑 —— 否則這件會無格口可去。
        if picked.is_none() {
            for idx in tail_blocked {
                let (pos, code) = &candidates[idx];
                if consume_skip(db, pos).await {
                    continue;
                }
                picked = Some(code.clone());
                break;
            }
        }

        // 記住這格口這次收到誰、以及它是最新收件的那個。以「分配」為準而非列印結果:
        // 包裹已經滾進那個格口,後面印不印得出來都不影響作業員看到的順序。
        if let Some(code) = &picked {
            rt.seq += 1;
            let seq = RUNTIME_SEQ_BASE + rt.seq;
            let no = shipping_no
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .or_else(|| rt.last.get(code).and_then(|l| l.no.clone()));
            rt.last.insert(code.clone(), ChannelLast { seq, no });
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

/// 取某分揀通道在「分揀通道」頁設定的本機印表機(空字串視同未設)。
/// direct_print 入列前與錯誤面單列印共用此單一查詢來源。
async fn fetch_channel_printer(db: &DbPool, channel_code: &str) -> Option<String> {
    if channel_code.is_empty() {
        return None;
    }
    sqlx::query(
        "SELECT printer_name FROM sort_channels
         WHERE channel_code = ? AND printer_name IS NOT NULL AND printer_name != ''",
    )
    .bind(channel_code)
    .fetch_optional(db)
    .await
    .ok()
    .flatten()
    .and_then(|r| r.try_get::<Option<String>, _>("printer_name").ok().flatten())
}

/// 取某分揀通道在「分揀通道」頁設定的貼標人員(未指派通道 / 未填時為 None)。
/// **印單統計、DirectPrint 自補回報、工控機回報三處共用此單一查詢來源**,
/// 避免各自寫一份 SQL 而在欄位或空值規則上長歪。
async fn fetch_channel_sticker(db: &DbPool, channel_code: Option<&str>) -> Option<String> {
    let code = channel_code.filter(|c| !c.is_empty())?;
    match sqlx::query("SELECT job_sticker FROM sort_channels WHERE channel_code = ?")
        .bind(code)
        .fetch_optional(db)
        .await
    {
        Ok(row) => row.and_then(|r| r.try_get::<Option<String>, _>("job_sticker").ok().flatten()),
        Err(e) => {
            // 查詢失敗與「通道沒填貼標人員」都回 None,但兩者意義天差地遠:
            // 前者會讓這件的貼標人員被永久記成空白(推出去就補不回來),必須留痕才追得到。
            tracing::warn!(channel_code = %code, ?e, "查詢通道貼標人員失敗,本件將記為未填");
            event_log::log_bg(db.clone(), "warn", "server", "貼標人員查詢失敗",
                format!("通道 {code} 的貼標人員查詢失敗,本件回報將不帶貼標人員"));
            None
        }
    }
}

/// 找一台可用的列印機給**錯誤面單**用,優先序:
/// 1. `channel_code` 對應分揀通道設定的 printer_name(錯誤面單跟著該包裹要去的格口出)
/// 2. 系統預設印表機
///
/// **刻意不退回「任一已設印表機的通道」**:那是無 ORDER BY 的 `LIMIT 1`,實務上等於隨機挑一條
/// 分揀線,錯誤面單會從別條線吐出來 —— 該線作業員不知情、異常件所在的線什麼也沒印,
/// 異常包裹被當正常件放行。落到系統預設印表機至少是可預測、可事先設定的單一出口。
/// 在 background task 中呼叫,失敗只 warn 不 panic。
async fn find_any_printer(db: &DbPool, channel_code: Option<&str>) -> Option<String> {
    if let Some(code) = channel_code {
        if let Some(name) = fetch_channel_printer(db, code).await {
            return Some(name);
        }
        tracing::warn!(channel_code = %code,
            "錯誤面單:該通道未設印表機(或代碼為未指派 fallback),退回系統預設印表機");
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
    channel_code: Option<String>,
) {
    tokio::spawn(async move {
        match find_any_printer(&db, channel_code.as_deref()).await {
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

/// emit `error-label-print-failed` 給桌面前端（reason: "no_printer" / "print_failed" / "cache_write_failed"）。
fn emit_error_label_failed(app: &tauri::AppHandle, query_no: &str, reason: &str) {
    
    let payload = serde_json::json!({ "query_no": query_no, "reason": reason });
    if let Err(e) = crate::event_bridge::emit(app, "error-label-print-failed", payload) {
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
    // 原子寫入(同 cache 下載 / 浮水印,見 crate::fs_atomic):寫臨時檔再 rename 覆蓋,避免
    // /images 服務在寫入過程中讀到截斷的錯誤面單(printer 端解碼報 "unexpected end of file")。
    match crate::fs_atomic::write_async(&path, bytes).await {
        Ok(()) => Some(key),
        Err(e) => {
            tracing::warn!(?e, "寫入錯誤面單快取失敗");
            None
        }
    }
}

/// emit `parcel-alert` 給前端;失敗只記 warn,不影響回應工控機
/// 看板事件的流水號:看板用它分辨「這是新的一件」還是重連後補到的同一筆。
static BOARD_SEQ: AtomicU64 = AtomicU64::new(0);

/// 分揀看板的一則即時狀態 —— 一件包裹剛分完格口,看板該亮哪盞燈、中央顯示什麼。
#[derive(Clone, Serialize)]
struct BoardEvent {
    /// 亮燈的格口位置(L1..R5)。未指派通道或查無物流時為 None,看板只出字不亮燈。
    position: Option<String>,
    /// 中央特大字的單號。查得到訂單用配送單號,查不到則退回工控機掃到的條碼。
    no: String,
    /// 單號下方的物流名稱。雲端帶不出物流商時為 None。
    provider: Option<String>,
    /// ok=正常(白字) / error=查件異常(紅字) / unassigned=該物流沒有指派通道(黃字)
    status: &'static str,
    /// 異常訊息,只在 status=error 時有值
    message: Option<String>,
    /// 分揀時刻(本機時間,到秒)。看板顯示它,現場核對包裹經過的時間用
    at: String,
    seq: u64,
}

/// 把一則看板狀態同時送到桌面看板頁(Tauri 事件)與網頁看板(SSE)。
/// 兩邊看同一份資料,不會一邊即時、一邊慢半拍。
/// 看板顯示的分揀時刻:取推播當下的本機時間,日期到秒都給 ——
/// 夜班跨午夜時只有時分秒會分不出是哪一天的件
fn board_now() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn publish_board(state: &ServerState, ev: BoardEvent) {
    // 桌面與網頁看板共用同一條匯流排 —— 網頁端沒人開著時內部 send 會回 Err,
    // 屬正常狀態(見 event_bridge),不必在這裡處理
    let _ = crate::event_bridge::emit(&state.app, "sort-board", &ev);
}

/// 依通道代碼反查它掛在哪個位置(L1..R5)。看板亮燈看的是位置,不是代碼。
async fn fetch_position_by_code(db: &DbPool, channel_code: &str) -> Option<String> {
    sqlx::query("SELECT position FROM sort_channels WHERE channel_code = ?")
        .bind(channel_code)
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
        .and_then(|r| r.try_get::<String, _>("position").ok())
}

/// 取物流商顯示名稱(「指派物流」頁維護的主檔);查不到就退回代碼本身,看板不會空一塊。
async fn fetch_provider_name(db: &DbPool, code: &str) -> Option<String> {
    if code.trim().is_empty() {
        return None;
    }
    let name = sqlx::query("SELECT name FROM dispatch_provider WHERE code = ?")
        .bind(code)
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
        .and_then(|r| r.try_get::<String, _>("name").ok())
        .filter(|s| !s.trim().is_empty());
    Some(name.unwrap_or_else(|| code.to_string()))
}

fn emit_parcel_alert(app: &tauri::AppHandle, kind: &str, message: &str, query_no: &str) {
    
    let payload = ParcelAlert {
        kind,
        message: message.to_string(),
        query_no: query_no.to_string(),
    };
    if let Err(e) = crate::event_bridge::emit(app, "parcel-alert", payload) {
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
    
    let payload = DeviceAlert {
        alert_type: alert_type.to_string(),
        message: message.to_string(),
    };
    if let Err(e) = crate::event_bridge::emit(app, "device-alert", payload) {
        tracing::warn!(?e, "emit device-alert 失敗");
    }
}

/// DirectPrint 模式:把一筆面單排入有序列印佇列(不阻塞工控機回應)。
/// 此模式下工控機拿到的回應不含 `label_path`(由中介機列印),圖檔處理全在背景 worker 進行;
/// 入列即代表確定要印,單一 worker 會照入列順序逐筆下載+列印,保證列印順序 = 請求順序。
#[allow(clippy::too_many_arguments)]
fn enqueue_direct_print(
    state: &ServerState,
    label_key: String,
    image_url: String,
    provider: String,
    channel_code: String,
    printer_name: String,
    print_num: u32,
    query_no: String,
    response_id: Option<i64>,
    tracking_no: String,
    job_sticker: Option<String>,
) {
    let job = DirectPrintJob {
        label_key,
        image_url,
        provider,
        channel_code,
        printer_name,
        print_num,
        query_no: query_no.clone(),
        response_id,
        tracking_no,
        job_sticker,
    };
    if let Err(e) = state.direct_print_tx.send(job) {
        let DirectPrintJob { response_id, tracking_no, .. } = e.0;
        tracing::warn!("direct_print 列印佇列已關閉,無法排入");
        // 入列失敗 = 此件確定不會被印(worker 已死),與下載/列印失敗同屬
        // 「工控機已收 200 但實體沒印」的靜默缺口,必須走同一條通報
        report_direct_print_failed(
            &state.app, &state.db, &state.queue, &query_no, "print_failed",
            response_id, Some(tracking_no.as_str()),
        );
    }
}

/// DirectPrint 失敗通報:emit `direct-print-failed`(前端 useParcelAlert 播音 + toast)+ 寫 event_log,
/// 並**攔下這件的雲端回報**(面單沒印出來,雲端不該記成完成)。
///
/// **DirectPrint 的失敗不可靜默**:工控機在入列當下已拿到成功回應、print_event 已記、袋核對已標已印,
/// 若這裡只 tracing::warn,分揀線整批漏印卻所有畫面都顯示正常(現場最難察覺的靜默故障)。
///
/// `response_id` / `tracking_no` 為 None 時只通報不攔截(雲端本就沒有對應列印記錄可回報)。
fn report_direct_print_failed(
    app: &tauri::AppHandle,
    db: &DbPool,
    queue: &QueueManager,
    query_no: &str,
    reason: &str,
    response_id: Option<i64>,
    tracking_no: Option<&str>,
) {
    
    let payload = serde_json::json!({ "query_no": query_no, "reason": reason });
    if let Err(e) = crate::event_bridge::emit(app, "direct-print-failed", payload) {
        tracing::warn!(?e, "emit direct-print-failed 失敗");
    }
    event_log::log_bg(db.clone(), "error", "printer", "直印失敗",
        format!("DirectPrint 列印失敗 query_no={query_no} reason={reason}(工控機已收到成功回應,此件實際未印出)"));

    // 攔截雲端回報:工控機可能在列印完成前就回報過(它只知道自己分揀完了,不知道面單沒印出來),
    // 也可能稍後才回報 —— 兩種都要擋,故即使目前沒有對應佇列項也會先立一筆墓碑。
    let (Some(rid), Some(tno)) = (response_id, tracking_no) else { return };
    let (queue, db, tno, reason, qn) = (
        queue.clone(), db.clone(), tno.to_string(), reason.to_string(), query_no.to_string(),
    );
    tokio::spawn(async move {
        match queue.cancel_report_on_print_failure(rid, &tno, &reason).await {
            Ok(CancelOutcome::Blocked) => {
                tracing::info!(query_no = %qn, "直印失敗,已攔下該件雲端回報");
            }
            Ok(CancelOutcome::SendInProgress) => {
                // 推送正在進行,結果未定 —— 攔截旗標已設下,由佇列 worker 收斂:
                // 這次推送失敗就定案攔下,成功則另發「無法撤回」告警。此處不預判、不誤報。
                tracing::info!(query_no = %qn, "直印失敗,該件正在推送中,已標記攔截待佇列收斂");
            }
            Ok(CancelOutcome::AlreadySent) => {
                // 收不回來了:雲端已記成完成,但實體沒印出 —— 必須讓現場知道要人工補處理
                event_log::log_bg(db, "error", "queue", "回報已送出無法撤回",
                    format!("直印失敗但該件回報已推送雲端(query_no={qn} 單號={tno});雲端已記為完成,此件需人工補印/更正"));
            }
            Err(e) => {
                tracing::warn!(?e, query_no = %qn, "攔截雲端回報失敗");
                event_log::log_bg(db, "error", "queue", "攔截回報失敗",
                    format!("直印失敗後無法攔截雲端回報(query_no={qn});此件可能被回報成完成,請人工確認"));
            }
        }
    });
}

/// DirectPrint worker 逐筆執行的單元:下載面單 → 套列印次數浮水印 → 送本機印表機。
/// **整個 await 到列印送出才回傳**,讓 worker 在處理下一筆前確保本筆已送印,保證順序且不並發打 spooler。
async fn run_direct_print_job(
    cache: &CacheManager,
    watermark: &WatermarkRenderer,
    db: &DbPool,
    app: &tauri::AppHandle,
    queue: &QueueManager,
    resolver: &LabelPathResolver,
    job: DirectPrintJob,
) {
    let DirectPrintJob {
        label_key,
        image_url,
        provider,
        channel_code,
        printer_name: pname,
        print_num,
        query_no,
        response_id,
        tracking_no,
        job_sticker,
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
        report_direct_print_failed(app, db, queue, &query_no, "download_failed",
            response_id, Some(tracking_no.as_str()));
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

    // 3. 讀圖 → 送印(印表機已於入列當下解析,見 DirectPrintJob::printer_name)
    tracing::debug!(channel_code = %channel_code, printer = %pname, "direct_print 送印");

    let img_path = cache_base.join(&effective_key);
    let bytes = match tokio::fs::read(&img_path).await {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(?e, "讀取面單圖失敗，無法列印");
            report_direct_print_failed(app, db, queue, &query_no, "read_failed",
                response_id, Some(tracking_no.as_str()));
            return;
        }
    };
    // await 此筆送印完成,才讓 worker 取下一筆 → 嚴格順序 + 不並發打 spooler
    let qn = query_no.clone();
    let printed = tokio::task::spawn_blocking(move || {
        crate::printer::print_image_bytes(&pname, &bytes)
    })
    .await;
    match printed {
        Ok(Ok(())) => {
            // 印出來了才補回報:直印模式下工控機沒有列印動作、實務上多半不會 POST /api/report,
            // 這是貼標人員唯一能送到雲端的路。刻意不立刻送 —— 先留寬限時間給工控機回報,
            // 它若在期間內回報就以它為準立即送出(詳見 QueueManager::enqueue_direct_print)。
            // 列印失敗的分支一律不補記:沒印出來的東西不該回報成完成。
            if let Some(rid) = response_id {
                let delay = resolver.report_delay_secs();
                match queue
                    .enqueue_direct_print(
                        rid,
                        &tracking_no,
                        Some(channel_code.as_str()),
                        job_sticker.as_deref(),
                        delay,
                    )
                    .await
                {
                    // 已存在時不重複建立(工控機搶先回報過);先前被攔下的則會在此解除攔截
                    Ok(()) => {}
                    Err(e) => {
                        tracing::warn!(?e, query_no = %qn, "直印自補回報入列失敗");
                        event_log::log_bg(db.clone(), "warn", "queue", "自補回報失敗",
                            format!("直印已印出但補記回報失敗 query_no={qn}(雲端不會收到這筆的貼標人員)"));
                    }
                }
            }
        }
        Ok(Err(e)) => {
            tracing::warn!(?e, query_no = %qn, "直接列印失敗");
            report_direct_print_failed(app, db, queue, &qn, "print_failed",
                response_id, Some(tracking_no.as_str()));
        }
        Err(e) => {
            tracing::warn!(?e, query_no = %qn, "直接列印 task 失敗");
            report_direct_print_failed(app, db, queue, &qn, "print_failed",
                response_id, Some(tracking_no.as_str()));
        }
    }
}

/// 每日統計 upsert:一次請求 `request_count` +1;`success` 時 `success_count` +1;`noread` 時 `noread_count` +1。
/// 四個請求結局(成功 / 未登入 / 其他錯誤 / NoRead)共用此一函式,新增計數欄位只需改這一處,
/// 避免多處手抄 SQL 漏改造成某分支少計。統計為次要,失敗只吞、不影響出單。
/// 用匿名 `?`(SQLite 位置綁定)重複 bind,規避 `?N` 編號規則陷阱。
async fn bump_daily_stats(db: &DbPool, success: bool, noread: bool) {
    let s = success as i64;
    let n = noread as i64;
    let _ = sqlx::query(
        "INSERT INTO daily_stats (date, request_count, success_count, noread_count)
         VALUES (date('now'), 1, ?, ?)
         ON CONFLICT(date) DO UPDATE SET
            request_count = request_count + 1,
            success_count = success_count + ?,
            noread_count  = noread_count + ?",
    )
    .bind(s)
    .bind(n)
    .bind(s)
    .bind(n)
    .execute(db)
    .await;
}

/// 判斷工控機傳入的查詢碼是否為「讀碼失敗」訊號。
/// 正規化:僅保留英數字並轉小寫後比對 "noread",容錯 `NoRead` / `NO_READ` / `no read` / `NO-READ`。
fn is_noread(query_no: &str) -> bool {
    query_no
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect::<String>()
        == "noread"
}

/// 處理 NoRead(相機讀不到單號):不打雲端,拍照存證 + 計數 + 記查詢紀錄(供回看照片),
/// emit `parcel-alert`(kind=`noread`,前端只 toast 不出聲)。立即回 200,label_path=null。
///
/// 存證 key 為 `NoRead_{YYYYMMDDHHMMSS}_{seq}.jpg`;`seq` 為進程內單調遞增序號,
/// 避免「同一秒多筆讀碼失敗」用固定字首 + 秒級時間戳產生相同檔名互相覆蓋(存證是本功能核心,不可遺失)。
/// 該檔名(去副檔名)即作為這筆的 pseudo 單號(tracking_no),對齊「沒有單號就用 NoRead_時間」。
async fn handle_noread(
    state: &ServerState,
    snapshot: Option<Vec<u8>>,
    t_start: std::time::Instant,
) -> Json<DataEnvelope<ParcelData>> {
    use std::sync::atomic::{AtomicU64, Ordering};
    // 進程內單調序號:保證同秒多筆 NoRead 檔名互異(不依賴時鐘解析度)。
    static NOREAD_SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = NOREAD_SEQ.fetch_add(1, Ordering::Relaxed);

    // pseudo 單號 = 存證檔名主幹(時間 + 序號),兩者一致;無相機時仍有唯一 pseudo(僅無照片)。
    let pseudo = format!(
        "NoRead_{}_{}",
        chrono::Local::now().format("%Y%m%d%H%M%S"),
        seq
    );

    let total_ms = t_start.elapsed().as_millis() as i64;

    // 1. 查詢紀錄先同步寫入(photo_path 先留 NULL):讓「請求記錄」頁立即看得到這筆;存證照片改由背景
    //    寫檔完成後再 UPDATE 回填,**不讓工控機等磁碟 I/O**(對齊成功路徑「先記錄、背景寫照片」作法)。
    //    負數 response_id 與雲端正數 ID 區隔;should_print=0;tracking_no=pseudo(唯一);工控機無需回報。
    //    寫入失敗記 warn 不靜默吞。
    if let Err(e) = sqlx::query(
        "INSERT INTO parcel_query_log
           (response_id, query_no, tracking_no, shipping_provider, sort_channel, print_profile, should_print, label_key, photo_path, created_at, cloud_ms, label_ms, total_ms)
         VALUES (
           (SELECT COALESCE(MIN(response_id), 0) - 1 FROM parcel_query_log WHERE response_id < 0),
           'NoRead', ?, NULL, NULL, NULL, 0, NULL, NULL, datetime('now','localtime'), 0, 0, ?)",
    )
    .bind(&pseudo)
    .bind(total_ms)
    .execute(&state.db)
    .await
    {
        tracing::warn!(?e, pseudo = %pseudo, "NoRead 查詢紀錄寫入失敗");
    }

    // 2. 存證:背景寫檔 + 回填 photo_path(不阻塞回應)。無相機幀時略過,photo_path 保持 NULL。
    //    以唯一的 tracking_no(pseudo)定位回填,寫完再 emit 讓頁面顯示照片。
    if let Some(jpeg) = snapshot {
        let dir = state.captures_dir.clone();
        let db = state.db.clone();
        let app = state.app.clone();
        let pseudo_bg = pseudo.clone();
        tokio::spawn(async move {
            let stem = pseudo_bg.clone();
            let key = tokio::task::spawn_blocking(move || {
                crate::camera::save_snapshot_named(&dir, &stem, &jpeg)
            })
            .await
            .ok()
            .flatten();
            if let Some(key) = key {
                let _ = sqlx::query(
                    "UPDATE parcel_query_log SET photo_path = ? WHERE tracking_no = ?",
                )
                .bind(&key)
                .bind(&pseudo_bg)
                .execute(&db)
                .await;
                
                let _ = crate::event_bridge::emit(&app, "parcel-query-logged", ());
            }
        });
    }

    // 3. 統計:request_count 照計(仍是一次請求)、success 不計、noread_count +1。
    bump_daily_stats(&state.db, false, true).await;

    // 4. 通知前端:parcel-alert kind=noread(前端只 toast 不出聲)+ 刷新請求記錄頁。
    //    message + query_no 皆傳空 → 前端顯示乾淨的在地化標題「讀碼失敗(NoRead)」(中/越雙語);
    //    不附上 pseudo(那是內部存證檔名,操作員看不懂也無從處理,只會製造雜訊)。存證編號在請求記錄頁可查。
    emit_parcel_alert(&state.app, "noread", "", "");
    {
        
        let _ = crate::event_bridge::emit(&state.app, "parcel-query-logged", ());
    }

    // 5. event_log 記一筆供診斷(NoRead 屬需人工處理的異常件)
    event_log::log_bg(
        state.db.clone(),
        "warn",
        "server",
        "讀碼失敗",
        format!("工控機讀碼失敗 NoRead,已存證 {pseudo}(未提交雲端)"),
    );

    // 立即回 200:無面單、無通道;error_code=NOREAD 讓工控機辨識此為讀碼失敗(不需 POST /api/report)。
    Json(DataEnvelope::new(ParcelData {
        channel_code: None,
        print_profile: None,
        label_path: None,
        response_id: None,
        is_error_label: false,
        error_code: Some("NOREAD".to_string()),
        message: Some("讀碼失敗,未提交雲端".to_string()),
    }))
}

/// GET /api/parcel/:query_no — 工控機呼叫
async fn get_parcel(
    State(state): State<ServerState>,
    headers: axum::http::HeaderMap,
    Path(query_no): Path<String>,
) -> Result<Json<DataEnvelope<ParcelData>>, (StatusCode, Json<ApiErrorBody>)> {
    // axum 0.8 起沒有 Host extractor(舊版會採信 X-Forwarded-Host,對外服務有假冒風險);
    // 本服務只在區網內給工控機呼叫,直接取 Host header,不看任何 forwarded 標頭。
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let t_start = std::time::Instant::now();
    // 收到請求的「當下」就釘住讀碼站相機最新一幀(離工控機實際讀碼僅約 20ms),
    // 不在這裡存檔(避免擋住回應),純記憶體複製;後續查得到訂單才丟背景寫檔 + 回寫 photo_path。
    let snapshot = state.camera.latest_jpeg();

    // NoRead 短路:工控機相機讀不到單號(送 "NoRead")→ 不打雲端,只拍照存證 + 計數。
    // 沒有單號、無面單、無通道,故不進 print_event / bag_check(active_bag 不變 = 連續不中斷)。
    if is_noread(&query_no) {
        return Ok(handle_noread(&state, snapshot, t_start).await);
    }

    // 純分揀模式:中介機只回分揀通道,不碰面單(獨立於面單路徑模式)。
    // 一併帶給雲端(?sort_only=1),讓雲端該支請求也不記任何印單。
    let is_sort_only = state.label_resolver.is_sort_only();

    match state.cloud.fetch_parcel(&query_no, is_sort_only).await {
        Ok(info) => {
            let cloud_ms = t_start.elapsed().as_millis() as i64;

            // 用 shipping_no 作為快取 key (檔名末段帶副檔名,從 shipping_image URL 推得)
            let label_key = derive_label_key(&info.shipping_image);
            let cache_base = state.cache.base_dir();

            let print_num = info.print_num.unwrap_or(0);

            // 雲端回空 shipping_image(無面單可印)時直接視為「無面單」:不嘗試下載、不排列印,
            // 避免空 URL 推出兜底 key 後發無謂請求 + 噪音警告(內容正確性已由 fetch_now source_url 校驗保證)。
            let has_image = !info.shipping_image.trim().is_empty();

            // 先解析分揀通道(便宜的本地 DB 查詢,**必須在面單處理之前**):
            // 未指派任何通道(has_assigned=false)時工控機無格口可分揀、不需面單 ——
            // DirectPrint 不可入列送印(否則印出一疊無格口可分揀的面單),
            // 其他模式也不必同步下載(白等雲端一趟、結果直接被丟棄)。四種模式行為一致。
            let (channel_code, has_assigned) = resolve_channel_code(
                &state.db,
                &state.routing,
                &info.shipping_provider,
                Some(info.shipping_no.as_str()),
            )
            .await;

            // 貼標人員在此一次查妥,下面 print_event 與 DirectPrint 自補回報共用同一份 ——
            // 兩處各查一次會在「查詢之間操作員剛好改了通道設定」時對不起來(印單統計記 A、回報推 B)。
            let sticker_user = fetch_channel_sticker(&state.db, channel_code.as_deref()).await;

            // DirectPrint 模式:工控機拿到 label_path=null(由中介機列印),不需要圖檔本身 ──
            // 立即回應,圖檔下載 + 浮水印 + 列印全部丟背景,不讓工控機等雲端(設計原則 #2)。
            // 其餘模式(local/share/http):工控機要讀檔,必須同步下載到完成才回(設計原則 #3)。
            let (label_path, label_ms) =
                if is_sort_only {
                    // 純分揀:只回分揀通道,不產出面單 —— 不下載、不浮水印、不列印、不入 DirectPrint 佇列。
                    (None, 0i64)
                } else if !has_image {
                    tracing::warn!(query_no = %query_no, "雲端回空 shipping_image,視為無面單");
                    (None, 0i64)
                } else if !has_assigned {
                    // 未指派通道:無格口可分揀 → 不印、不下載(label_path 一律 None)
                    tracing::info!(query_no = %query_no, provider = %info.shipping_provider,
                        "物流商未指派分揀通道,略過面單處理");
                    (None, 0i64)
                } else if state.label_resolver.current_mode() == LabelPathMode::DirectPrint {
                    // has_assigned=true 保證 channel_code 為 Some(實際通道代碼)。
                    // **印表機在此當場解析**(而非丟給背景 worker 反查):查不到就立即通報,
                    // 讓「通道漏設印表機」在第一件就被聽見,而不是整批靜默積在佇列裡才發現。
                    let cc = channel_code.clone().unwrap_or_default();
                    match fetch_channel_printer(&state.db, &cc).await {
                        Some(pname) => {
                            enqueue_direct_print(
                                &state,
                                label_key.clone(),
                                info.shipping_image.clone(),
                                info.shipping_provider.clone(),
                                cc,
                                pname,
                                print_num,
                                query_no.clone(),
                                info.response_id,
                                info.shipping_no.clone(),
                                sticker_user.clone(),
                            );
                        }
                        None => {
                            tracing::warn!(channel_code = %cc, provider = %info.shipping_provider,
                                "direct_print 模式但分揀通道未設定印表機,此件不會印出");
                            report_direct_print_failed(
                                &state.app, &state.db, &state.queue, &query_no, "no_printer",
                                info.response_id, Some(info.shipping_no.as_str()),
                            );
                        }
                    }
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

            // 從 dispatch_provider 取 print_profile (使用者在「指派物流」頁面設定)
            let print_profile = fetch_print_profile(&state.db, &info.shipping_provider).await;

            // 紀錄這次查詢,POST /api/report 用 response_id 反查。
            // 一般模式:雲端回正數 response_id(order_print_log 主鍵)→ 直接用 + ON CONFLICT。
            // 純分揀模式:雲端不記印單、不回 response_id → 產本地負數 id(與錯誤面單同池),
            //   仍完整保留查詢記錄 + 相機存證(純分揀刻意保留請求記錄)。
            let total_ms = t_start.elapsed().as_millis() as i64;
            let logged_rid: Option<i64> = if let Some(rid) = info.response_id {
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
                // should_print:一般模式寫 1(工控機要印)。此分支僅在雲端回正數 response_id 時進入;
                // 純分揀正常部署下雲端不回 id、不會走到這裡(僅雲端未同步仍回正數時才會,此時寫 0)。
                .bind(if is_sort_only { 0 } else { 1 })
                .bind(&label_key)
                .bind(cloud_ms)
                .bind(label_ms)
                .bind(total_ms)
                .execute(&state.db)
                .await;
                Some(rid)
            } else if is_sort_only {
                // 純分揀:雲端未回 response_id,產本地負數 id(RETURNING 取回供存證 UPDATE);
                // should_print 固定 0(不出面單)。寫入失敗(罕見負數 id 競態)則略過此筆記錄。
                sqlx::query_scalar::<_, i64>(
                    "INSERT INTO parcel_query_log
                       (response_id, query_no, tracking_no, shipping_provider, sort_channel, print_profile, should_print, label_key, created_at, cloud_ms, label_ms, total_ms)
                     VALUES (
                       (SELECT COALESCE(MIN(response_id), 0) - 1 FROM parcel_query_log WHERE response_id < 0),
                       ?, ?, ?, ?, ?, 0, ?, datetime('now','localtime'), ?, ?, ?)
                     RETURNING response_id",
                )
                .bind(&query_no)
                .bind(&info.shipping_no)
                .bind(&info.shipping_provider)
                .bind(&channel_code)
                .bind(&print_profile)
                // 純分揀不下載面單:label_key 寫 NULL,否則「請求記錄」頁會對每筆顯示
                // 「檢視面單」按鈕、點擊指向不存在的 /images/{key} 而 404。
                .bind(None::<String>)
                .bind(cloud_ms)
                .bind(label_ms)
                .bind(total_ms)
                .fetch_one(&state.db)
                .await
                .map_err(|e| tracing::warn!(?e, query_no = %query_no, "純分揀查詢記錄寫入失敗"))
                .ok()
            } else {
                None
            };

            if let Some(rid) = logged_rid {
                
                let _ = crate::event_bridge::emit(&state.app, "parcel-query-logged", ());

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
                            let _ = crate::event_bridge::emit(&app, "parcel-query-logged", ());
                        }
                    });
                }
            }

            // 記一筆 daily request 統計(成功:request +1、success +1)
            bump_daily_stats(&state.db, true, false).await;

            if has_assigned {
              // 印單事件:source='ipc'(工控機 GET /api/parcel),sticker 由 channel_code 反查 sort_channels.job_sticker
              // 失敗不影響 API 回應(統計次要,不能干擾正常出單)。
              // 條件含 has_image:雲端回空 shipping_image 時實體無任何面單印出,不可記 print_event(同「未指派通道」原則)。
              // 純分揀模式(is_sort_only):完全不出面單 → 一律不記印單統計,但下方件核對仍照常(包裹實體仍過機分揀)。
              if !is_sort_only && has_image {
                // package_sn(袋號)由雲端 v2 回應帶出:記入 print_event 讓印單統計的「袋數」
                // 反映工控機分揀的分袋量。與 bag_check 同規則正規化:散單(空 / "0")存 NULL,
                // 否則空字串會被 COUNT(DISTINCT package_sn) 當成一個假袋、灌高袋數。
                let package_sn = info
                    .package_sn
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty() && *s != "0")
                    .map(str::to_string);
                let insert_res = sqlx::query(
                    "INSERT INTO print_event (source, shipping_no, provider_code, sticker_user, channel_code, package_sn)
                     VALUES ('ipc', ?, ?, ?, ?, ?)",
                )
                .bind(&info.shipping_no)
                .bind(&info.shipping_provider)
                .bind(&sticker_user)
                .bind(&channel_code)
                .bind(&package_sn)
                .execute(&state.db)
                .await;
                if insert_res.is_ok() {
                    crate::commands::print_stats_commands::emit_print_stats_updated(
                        &state.app,
                        "ipc",
                        &info.shipping_no,
                    );
                }
              } // if !is_sort_only && has_image(印單統計)

              // 分揀袋件核對:新袋背景 examine 取整袋清單,舊袋就地更新列印時間(非阻塞,不讓工控機等雲端)。
              // 一般模式需有面單印出(has_image)才標已印;純分揀模式無面單,只要分到通道即代表過機分揀 → 照標。
              if is_sort_only || has_image {
                state.bag_check.on_parcel(
                    info.package_sn.clone(),
                    &info.order_sn,
                    &info.shipping_no,
                    &info.shipping_provider,
                );
              }
            } else {
                // 未指派通道(!has_assigned):此件**沒有面單被印出**(DirectPrint 未入列、其他模式 label_path=None;
                // 純分揀模式本就不出面單),且無格口可分揀 → 不記 print_event、不做件核對,統計必須與實物一致
                // (否則儀表板全綠、現場卻累積一批無面單包裹)。event_log 節流告警(同 provider 20s 一次,防洪),
                // 提醒到「指派物流」頁補設定;工控機端仍收 200 + fallback 通道碼可分揀。
                if should_log_throttled(&format!("unassigned|{}", info.shipping_provider)) {
                    event_log::log_bg(state.db.clone(), "warn", "server", "未指派通道",
                        format!("物流商 {} 未指派分揀通道,面單未產出(shipping_no={};請至「指派物流」頁設定)",
                            info.shipping_provider, info.shipping_no));
                }
            }

            // 純分揀:一律回 response_id=null,工控機因而不會 POST /api/report。
            // 防呆:雲端已升級時本就回 None;若雲端未同步 / 回滾仍回正數 id(代表雲端已記了一筆印單),
            // 這裡主動吞掉不轉給工控機,避免工控機再回報觸發雲端二次記印單,並節流告警提醒兩端同步部署。
            let response_id = if is_sort_only {
                if info.response_id.is_some()
                    && should_log_throttled(&format!("sortonly_cloud_recorded|{}", info.shipping_provider))
                {
                    event_log::log_bg(state.db.clone(), "warn", "server", "純分揀雲端未同步",
                        format!("純分揀模式但雲端仍回 response_id(代表雲端已記印單);請確認雲端已部署 sort_only 支援(shipping_no={})",
                            info.shipping_no));
                }
                None
            } else {
                info.response_id
            };

            // 推一則給分揀看板:亮該格口的燈、中央顯示單號與物流名。
            // 未指派通道(has_assigned=false)時不亮燈、單號轉黃字,提醒現場這件沒有格口可去。
            publish_board(
                &state,
                BoardEvent {
                    position: match (has_assigned, channel_code.as_deref()) {
                        (true, Some(cc)) => fetch_position_by_code(&state.db, cc).await,
                        _ => None,
                    },
                    no: info.shipping_no.clone(),
                    provider: fetch_provider_name(&state.db, &info.shipping_provider).await,
                    status: if has_assigned { "ok" } else { "unassigned" },
                    message: None,
                    at: board_now(),
                    seq: BOARD_SEQ.fetch_add(1, Ordering::Relaxed),
                },
            );

            Ok(Json(DataEnvelope::new(ParcelData {
                channel_code,
                print_profile,
                label_path,
                response_id,
                is_error_label: false,
                error_code: None,
                message: None,
            })))
        }
        Err(AppError::Unauthorized) => {
            // 未登入:只計 request(非成功、非 NoRead)
            bump_daily_stats(&state.db, false, false).await;
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
            // 其他錯誤:只計 request(非成功、非 NoRead)
            bump_daily_stats(&state.db, false, false).await;
            // 依雲端錯誤訊息分類,讓桌面前端播放對應提示音(門市關轉 / 未確認 / 找不到 / 一般失敗)
            let (kind, msg, err_code, err_provider, err_shipping_no, err_package_sn, err_order_sn) = match &e {
                AppError::Cloud { code, message, shipping_provider, shipping_no, package_sn, order_sn } => (
                    classify_parcel_alert(code),
                    message.clone(),
                    code.clone(),
                    shipping_provider.clone(),
                    shipping_no.clone(),
                    package_sn.clone(),
                    order_sn.clone(),
                ),
                other => ("error", other.to_string(), "ERROR".to_string(), None, None, None, None),
            };
            emit_parcel_alert(&state.app, kind, &msg, &query_no);

            // 錯誤面單總開關(設定頁熱切換,預設關)。關閉時工控機只拿得到 error_code:
            // 不出提示面單、不回分揀通道,異常包裹由工控機自行走預設落格。
            let error_label_on = state.label_resolver.is_error_label_enabled();

            // 開啟時:雲端帶出物流商代碼(查得到訂單的業務錯誤,如 STORE_CLOSED / UNCONFIRMED)
            // 就照正常面單流程解析分揀通道與 print_profile;查不到物流商(NOT_FOUND / 雲端連線失敗)
            // 統一退回「未指派通道代碼」,讓所有錯誤面單只要有設 fallback 就一定有格口可分揀。
            // 關閉時一律不回通道 —— 連帶不查 print_profile(沒有面單就沒有列印參數)。
            let (channel_code, print_profile) = if !error_label_on {
                (None, None)
            } else {
                match err_provider.as_deref() {
                    Some(p) => {
                        let (cc, _) =
                            resolve_channel_code(&state.db, &state.routing, p, err_shipping_no.as_deref())
                                .await;
                        (cc, fetch_print_profile(&state.db, p).await)
                    }
                    None => (fetch_unassigned_channel_code(&state.db).await, None),
                }
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

            // 有跑 recordNotOutboundPrint 的業務錯誤(雲端已記 is_outbound=0)→ 件數核對也標記已印,
            // 對齊成功列印的 on_parcel;僅這 3 種 code 雲端會回 package_sn / order_sn。
            if matches!(err_code.as_str(), "UNCONFIRMED" | "STORE_CLOSED" | "STATUS_ABNORMAL") {
                if let Some(sn) = err_shipping_no.as_deref() {
                    // package_sn 為 Option;散單(空 / "0")由 on_parcel 內部自行略過
                    state.bag_check.on_parcel(
                        err_package_sn.clone(),
                        err_order_sn.as_deref().unwrap_or(""),
                        sn,
                        err_provider.as_deref().unwrap_or(""),
                    );
                }
            }

            // 錯誤面單(取向 A):產生提示圖,依面單路徑模式決定出口 —
            //   direct_print : 中介 PC 本機直接列印(label_path 回 null)
            //   local/share/http : 寫入 cache 後回 label_path,讓工控機如同一般面單自行列印
            // 這樣 http 模式(工控機跨機、本機無印表機)也能在分揀線印出錯誤面單。
            // 錯誤面單:即使在純分揀模式也照常產出/列印 —— 現場需靠它辨識、揀出異常包裹,
            // 屬「必須處理」的例外,不受純分揀「不出面單」規則抑制。
            let t_label = std::time::Instant::now();
            let (label_path, err_label_key) = if !error_label_on {
                (None, None)
            } else {
                let label_bytes = crate::error_label::generate(
                    &query_no,
                    &err_code,
                    crate::error_label::LabelHeight::H100mm,
                );
                if state.label_resolver.current_mode() == LabelPathMode::DirectPrint {
                    spawn_print_error_label_bytes(
                        state.db.clone(),
                        state.app.clone(),
                        query_no.clone(),
                        label_bytes,
                        channel_code.clone(),
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
                            // 寫檔失敗 → 改在本機嘗試列印 + 回 502,不讓工控機誤以為有面單
                            emit_error_label_failed(&state.app, &query_no, "cache_write_failed");
                            spawn_print_error_label_bytes(
                                state.db.clone(),
                                state.app.clone(),
                                query_no.clone(),
                                label_bytes,
                                channel_code.clone(),
                            );
                            return Err(err_resp(StatusCode::BAD_GATEWAY, e.to_string()));
                        }
                    }
                }
            };
            let label_ms = t_label.elapsed().as_millis() as i64;

            // 雲端錯誤一律寫入查詢記錄(不論錯誤面單開關):response_id 用本地負數遞減(與雲端正數 ID 區隔),
            // 工控機照正常流程 POST /api/report 才反查得到(負數回報只記錄、不推雲端 webhook);
            // 查詢記錄頁同時也看得到錯誤查詢,不留診斷盲區。
            // 開關關閉時 should_print=0(沒有面單可印),與正常路徑純分揀模式的記法一致。
            // 寫入失敗時退回 response_id=null(工控機不回報),不影響出單
            let total_ms = t_start.elapsed().as_millis() as i64;
            let response_id: Option<i64> = match sqlx::query_scalar::<_, i64>(
                "INSERT INTO parcel_query_log
                   (response_id, query_no, tracking_no, shipping_provider, sort_channel, print_profile, should_print, label_key, created_at, cloud_ms, label_ms, total_ms)
                 VALUES (
                   (SELECT COALESCE(MIN(response_id), 0) - 1 FROM parcel_query_log WHERE response_id < 0),
                   ?, '', ?, ?, ?, ?, ?, datetime('now','localtime'), ?, ?, ?)
                 RETURNING response_id",
            )
            .bind(&query_no)
            .bind(&err_provider)
            .bind(&channel_code)
            .bind(&print_profile)
            .bind(if error_label_on { 1 } else { 0 })
            .bind(&err_label_key)
            .bind(cloud_ms)
            .bind(label_ms)
            .bind(total_ms)
            .fetch_one(&state.db)
            .await
            {
                Ok(rid) => {
                    
                    let _ = crate::event_bridge::emit(&state.app, "parcel-query-logged", ());
                    Some(rid)
                }
                Err(e) => {
                    tracing::warn!(?e, query_no = %query_no, "錯誤面單查詢記錄寫入失敗");
                    None
                }
            };

            // 開關關閉時不回 response_id:沒有面單可印,工控機不必也不該 POST /api/report
            //(對齊 NoRead 的回應形態);查詢記錄仍留在本機供回看。
            // 查件異常也要上看板(紅字):現場才知道這件為什麼沒面單、要不要撿出來處理。
            // 提示面單開關關閉時不回通道,燈自然不亮。
            publish_board(
                &state,
                BoardEvent {
                    position: match channel_code.as_deref() {
                        Some(cc) => fetch_position_by_code(&state.db, cc).await,
                        None => None,
                    },
                    no: err_shipping_no
                        .clone()
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or_else(|| query_no.clone()),
                    provider: match err_provider.as_deref() {
                        Some(p) => fetch_provider_name(&state.db, p).await,
                        None => None,
                    },
                    status: "error",
                    message: Some(msg.clone()),
                    at: board_now(),
                    seq: BOARD_SEQ.fetch_add(1, Ordering::Relaxed),
                },
            );

            Ok(Json(DataEnvelope::new(ParcelData {
                channel_code,
                print_profile,
                label_path,
                response_id: if error_label_on { response_id } else { None },
                is_error_label: error_label_on,
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

    // 用 channel_code 反查通道設定上的貼標人員(與印單統計、直印自補共用同一查詢)
    let job_sticker = fetch_channel_sticker(&state.db, channel_code.as_deref()).await;

    // 寫入本機 queue (status=pending,含分流通道 + 貼標人員),
    // 背景 worker 會推送 logistic-cat webhook 並追蹤 status/retry/last_error。
    // DirectPrint 模式下該筆可能已由中介機自補建立 —— 由 UPSERT 原子地併入同一列
    // (兩列會被推兩次、雲端記兩筆印單),併入時順帶把自補的寬限等待清掉改為立即送出。
    if let Err(e) = state
        .queue
        .enqueue_ipc_report(
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

/// device-alert 後端 event_log 去洪窗:同一 (type|message) 在此窗內只寫一次事件記錄。
/// 持續性異常工控機會狂丟同一訊號,不節流會把 event_log 灌爆、淹掉其他事件並撐大 DB。
/// (前端另有 20s 廣播去抖;emit 仍每筆送出,只節流「寫 log」。)
const DEVICE_ALERT_LOG_THROTTLE: std::time::Duration = std::time::Duration::from_secs(20);

/// 通用 event_log 去洪:回傳此 key 現在是否該寫 log(距上次 >= [`DEVICE_ALERT_LOG_THROTTLE`] 窗、
/// 或首次),並更新時間戳。共用一張 static 表,呼叫端以 key 前綴區分用途
///(`{type}|{message}` 設備異常、`unassigned|{provider}` 未指派通道)。
fn should_log_throttled(key: &str) -> bool {
    use std::sync::{Mutex, OnceLock};
    use std::time::Instant;
    static GATE: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    let mut gate = GATE.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap();
    let now = Instant::now();
    // 順手清過期:key 可能含變動內容(message 夾通道/計數/時間戳)每筆都是新 key,
    // 不清的話常駐數週的進程會無界累積
    gate.retain(|_, last| now.duration_since(*last) < DEVICE_ALERT_LOG_THROTTLE);
    match gate.get(key) {
        Some(&last) if now.duration_since(last) < DEVICE_ALERT_LOG_THROTTLE => false,
        _ => {
            gate.insert(key.to_string(), now);
            true
        }
    }
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

    // event_log 去洪:同一 (type|message) 20s 內只記一次,避免持續性異常灌爆事件記錄。
    // emit 仍每筆送(前端自行去抖顯示),此處只節流「寫入 event_log」。
    if should_log_throttled(&format!("{alert_type}|{message}")) {
        let detail = if message.trim().is_empty() {
            format!("工控機設備異常 type={alert_type}")
        } else {
            format!("工控機設備異常 type={alert_type} message={message}")
        };
        event_log::log_bg(state.db.clone(), "warn", "server", "設備異常", detail);
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({ "message": "OK" })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noread_normalization_matches_variants() {
        // 相機讀不到單號的各種寫法都應觸發 NoRead 短路(正規化:去非英數、小寫)
        for q in ["NoRead", "noread", "NOREAD", "NO_READ", "no read", "NO-READ", " NoRead "] {
            assert!(is_noread(q), "應判定為 NoRead: {q:?}");
        }
    }

    #[test]
    fn normal_query_no_is_not_noread() {
        // 正常條碼不可誤判成 NoRead(否則會漏查雲端)
        for q in ["SF0220862051573", "noread123", "read", "0STTJX9B1694", ""] {
            assert!(!is_noread(q), "不應判定為 NoRead: {q:?}");
        }
    }

    #[test]
    fn tail2_takes_last_two_chars_case_insensitively() {
        assert_eq!(tail2("SF0220862051573").as_deref(), Some("73"));
        assert_eq!(tail2("  0STTJX9B169a  ").as_deref(), Some("9A"));
        // 不足兩碼比不出後兩碼,一律不套用尾碼迴避
        assert_eq!(tail2("7"), None);
        assert_eq!(tail2(""), None);
    }

    // ── 尾碼迴避的分配測試 ──────────────────────────────────
    // 作業員貼單靠單號後兩碼認包裹,同一格口連著兩張尾碼相同的單會分不出誰是誰。
    // 以下用 in-memory SQLite 建與 migration 等價的三張表,直接跑 resolve_channel_code。

    use sqlx::sqlite::SqlitePoolOptions;

    /// 建三張路由會用到的表(sort_channels / sort_channel_dispatch / print_event / settings)
    async fn routing_db(positions: &[(&str, &str)], provider: &str) -> DbPool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        for sql in [
            "CREATE TABLE sort_channels (
                position TEXT PRIMARY KEY,
                channel_code TEXT,
                job_sticker TEXT,
                printer_name TEXT,
                enabled INTEGER NOT NULL DEFAULT 1,
                skip_count INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL DEFAULT (datetime('now','localtime'))
            )",
            "CREATE TABLE sort_channel_dispatch (
                position TEXT NOT NULL,
                dispatch_code TEXT NOT NULL,
                PRIMARY KEY (position, dispatch_code)
            )",
            "CREATE TABLE print_event (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source TEXT NOT NULL,
                shipping_no TEXT NOT NULL,
                channel_code TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now','localtime'))
            )",
            "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
        ] {
            sqlx::query(sql).execute(&pool).await.unwrap();
        }
        for (pos, code) in positions {
            sqlx::query("INSERT INTO sort_channels (position, channel_code) VALUES (?, ?)")
                .bind(pos)
                .bind(code)
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query("INSERT INTO sort_channel_dispatch (position, dispatch_code) VALUES (?, ?)")
                .bind(pos)
                .bind(provider)
                .execute(&pool)
                .await
                .unwrap();
        }
        pool
    }

    fn routing_state() -> SortRoutingState {
        Arc::new(tokio::sync::Mutex::new(SortRouting::default()))
    }

    #[tokio::test]
    async fn same_tail_goes_to_the_next_channel() {
        let db = routing_db(&[("L1", "A01"), ("L2", "A02")], "C").await;
        let routing = routing_state();

        let (first, _) = resolve_channel_code(&db, &routing, "C", Some("SF00000000047")).await;
        assert_eq!(first.as_deref(), Some("A01"));
        let (second, _) = resolve_channel_code(&db, &routing, "C", Some("SF99999999988")).await;
        assert_eq!(second.as_deref(), Some("A02"));
        // 第三件又是 47:輪替指標指回 A01,但 A01 上一件就是 47 → 改給尾碼不撞的 A02
        let (third, _) = resolve_channel_code(&db, &routing, "C", Some("SF12312312347")).await;
        assert_eq!(third.as_deref(), Some("A02"), "撞尾碼的格口不該再收一件同尾碼");
        // 換回不撞的尾碼:輪替回到 A01,不該因為前一件被改道就一直卡在 A02
        let (fourth, _) = resolve_channel_code(&db, &routing, "C", Some("SF45645645612")).await;
        assert_eq!(fourth.as_deref(), Some("A01"));
    }

    #[tokio::test]
    async fn single_channel_ignores_tail_rule() {
        // 只有一個可用通道時無處可換,尾碼相同也照給 —— 否則這件會無格口可去
        let db = routing_db(&[("L1", "A01")], "C").await;
        let routing = routing_state();

        let (first, _) = resolve_channel_code(&db, &routing, "C", Some("SF00000000047")).await;
        assert_eq!(first.as_deref(), Some("A01"));
        let (second, _) = resolve_channel_code(&db, &routing, "C", Some("SF99999999947")).await;
        assert_eq!(second.as_deref(), Some("A01"));
    }

    #[tokio::test]
    async fn tail_rule_runs_only_one_lap() {
        // 兩個格口上一件都是 47,再來一件 47:繞完一圈仍找不到不撞的,
        // 就照輪替順序給出去,不能因此不分配
        let db = routing_db(&[("L1", "A01"), ("L2", "A02")], "C").await;
        let routing = routing_state();
        resolve_channel_code(&db, &routing, "C", Some("SF00000000047")).await;
        resolve_channel_code(&db, &routing, "C", Some("SF11111111147")).await;

        let (third, has_assigned) = resolve_channel_code(&db, &routing, "C", Some("SF22222222247")).await;
        assert_eq!(third.as_deref(), Some("A01"), "全部撞尾碼時應照輪替順序分配");
        assert!(has_assigned);
    }

    #[tokio::test]
    async fn cold_start_reads_last_number_from_print_events() {
        // 進程剛啟動、記憶體是空的:要回查列印記錄,否則重開 App 後尾碼迴避形同失效
        let db = routing_db(&[("L1", "A01"), ("L2", "A02")], "C").await;
        sqlx::query(
            "INSERT INTO print_event (source, shipping_no, channel_code) VALUES ('ipc', 'SF00000000047', 'A01')",
        )
        .execute(&db)
        .await
        .unwrap();
        let routing = routing_state();

        let (picked, _) = resolve_channel_code(&db, &routing, "C", Some("SF98765432147")).await;
        assert_eq!(picked.as_deref(), Some("A02"), "A01 的歷史尾碼是 47,應改給 A02");
    }

    #[tokio::test]
    async fn tail_skip_does_not_consume_skip_quota() {
        // 「跳過本輪」是操作員按的額度,只該花在真的輪到它的那次;
        // 因撞尾碼而讓過的那次不能偷扣,否則手機按的一次跳過會平白消失
        let db = routing_db(&[("L1", "A01"), ("L2", "A02")], "C").await;
        sqlx::query("UPDATE sort_channels SET skip_count = 1 WHERE position = 'L1'")
            .execute(&db)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO print_event (source, shipping_no, channel_code) VALUES ('ipc', 'SF00000000047', 'A01')",
        )
        .execute(&db)
        .await
        .unwrap();
        let routing = routing_state();

        let (picked, _) = resolve_channel_code(&db, &routing, "C", Some("SF55555555547")).await;
        assert_eq!(picked.as_deref(), Some("A02"));
        let left: i64 = sqlx::query("SELECT skip_count FROM sort_channels WHERE position = 'L1'")
            .fetch_one(&db)
            .await
            .unwrap()
            .try_get("skip_count")
            .unwrap();
        assert_eq!(left, 1, "撞尾碼讓過不該消耗跳過額度");
    }

    /// 左1=7-11、左2=7-11+全家、左3=7-11 —— 共用格口的情境
    async fn shared_channel_db() -> DbPool {
        let db = routing_db(&[("L1", "L1"), ("L2", "L2"), ("L3", "L3")], "7").await;
        sqlx::query("INSERT INTO sort_channel_dispatch (position, dispatch_code) VALUES ('L2','F')")
            .execute(&db)
            .await
            .unwrap();
        db
    }

    #[tokio::test]
    async fn shared_channel_yields_to_the_idlest_one() {
        // 包裹順序 7-11 → 全家 → 7-11。中間那件全家進了左2,第三件 7-11 就該讓給
        // 一直沒收件的左3 —— 排序看的是「格口實際收了什麼」,不是各家物流自己的順序。
        let db = shared_channel_db().await;
        let routing = routing_state();

        let (a, _) = resolve_channel_code(&db, &routing, "7", Some("SF00000000011")).await;
        let (b, _) = resolve_channel_code(&db, &routing, "F", Some("SF00000000022")).await;
        let (c, _) = resolve_channel_code(&db, &routing, "7", Some("SF00000000033")).await;
        assert_eq!(
            (a.as_deref(), b.as_deref(), c.as_deref()),
            (Some("L1"), Some("L2"), Some("L3")),
            "左2 剛收過全家的件,第三件應讓給最久沒收的左3"
        );
    }

    #[tokio::test]
    async fn tail_rule_looks_at_the_channel_not_the_provider() {
        // 格口的「上一件」不分物流:左2 上一件是全家的單,接著輪到左2 的 7-11 件
        // 若尾碼與它相同,一樣要讓開 —— 作業員站在格口前看的是單號,不是哪家物流。
        let db = shared_channel_db().await;
        let routing = routing_state();

        resolve_channel_code(&db, &routing, "F", Some("SF00000000044")).await; // 左2 收全家,尾碼 44
        resolve_channel_code(&db, &routing, "7", Some("SF00000000011")).await; // 左1
        resolve_channel_code(&db, &routing, "7", Some("SF00000000022")).await; // 左3
        // 此時最久沒收的是左2,但它上一件尾碼正是 44 → 讓給次久的左1
        let (d, _) = resolve_channel_code(&db, &routing, "7", Some("SF99999999944")).await;
        assert_eq!(d.as_deref(), Some("L1"), "撞到別家物流留下的尾碼一樣要讓開");
    }

    #[tokio::test]
    async fn no_shipping_no_still_alternates_channels() {
        // 錯誤面單等拿不到配送單號的情況:沒有尾碼可比,仍照「最久沒收件」輪流
        let db = routing_db(&[("L1", "A01"), ("L2", "A02")], "C").await;
        let routing = routing_state();

        let (a, _) = resolve_channel_code(&db, &routing, "C", None).await;
        let (b, _) = resolve_channel_code(&db, &routing, "C", None).await;
        let (c, _) = resolve_channel_code(&db, &routing, "C", None).await;
        assert_eq!(
            (a.as_deref(), b.as_deref(), c.as_deref()),
            (Some("A01"), Some("A02"), Some("A01"))
        );
    }
}
