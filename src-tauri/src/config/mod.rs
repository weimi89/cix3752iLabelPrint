use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tokio::fs;

use crate::{AppError, AppResult};

/// 應用設定 — 持久化於 `~/Library/Application Support/<appId>/config.toml`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub cloud: CloudConfig,
    #[serde(default)]
    pub cache: CacheConfig,
    #[serde(default)]
    pub network: NetworkConfig,
    #[serde(default)]
    pub label_path: LabelPathConfig,
    #[serde(default)]
    pub pre_gen_schedule: PreGenScheduleConfig,
    #[serde(default)]
    pub camera: CameraConfig,
    #[serde(default)]
    pub sync: SyncConfig,
}

/// 件數核對跨機同步設定 — 訂閱 Reverb(Pusher 相容)的 `bag.{package_sn}` 廣播,
/// 讓「同一袋包裹拿到別台處理」時本機核對清單也即時更新。預設關閉。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    /// 總開關(預設關;填好 host/key 並確認雲端同環境廣播後再開)
    #[serde(default)]
    pub enabled: bool,
    /// Reverb 主機位址(須與該分揀場雲端廣播伺服器同環境;實際值由部署設定填入)
    #[serde(default)]
    pub reverb_host: String,
    /// Reverb 連接埠(預設 443)
    #[serde(default = "default_reverb_port")]
    pub reverb_port: u16,
    /// 連線 scheme:`wss`(TLS,預設)或 `ws`
    #[serde(default = "default_reverb_scheme")]
    pub reverb_scheme: String,
    /// Reverb 廣播 app key(由部署設定填入;public 頻道訂閱免 secret)
    #[serde(default)]
    pub reverb_app_key: String,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            reverb_host: String::new(),
            reverb_port: default_reverb_port(),
            reverb_scheme: default_reverb_scheme(),
            reverb_app_key: String::new(),
        }
    }
}

fn default_reverb_port() -> u16 { 443 }
fn default_reverb_scheme() -> String { "wss".to_string() }

/// 讀碼站快照相機設定 — 收到工控機 GET /api/parcel 時抓 USB 相機當下一幀存證
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraConfig {
    /// 總開關(預設關;接好相機、選對 device_index 再開)
    #[serde(default)]
    pub enabled: bool,
    /// USB 相機裝置索引(0 = 第一台;多台時依序試)
    #[serde(default)]
    pub device_index: u32,
    /// JPEG 壓縮品質 1-100(預設 80,存證夠用又省空間)
    #[serde(default = "default_camera_quality")]
    pub jpeg_quality: u8,
    /// 數位變焦 1.0–4.0(1.0=原畫面;>1 裁切畫面中央 1/zoom 區域=「拉近」框景)。
    /// USB 相機多半無法用軟體控制光學變焦/對焦,故以數位裁切達成「拉近」,讓相機擺不到理想位置時仍能對準讀碼站。
    #[serde(default = "default_camera_zoom")]
    pub zoom: f32,
    /// 存證快照存放目錄(留白=預設 `{app_data}/captures`)。
    /// **刻意與面單快取目錄分離** —— 存證是爭議佐證,不該被面單快取的 keep_days(可能僅數天)清掉。
    #[serde(default)]
    pub captures_dir: String,
    /// 存證快照保留天數(0 = 永久保留)。與面單快取的 cache.keep_days 完全獨立。
    #[serde(default = "default_captures_keep_days")]
    pub keep_days: u32,
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            device_index: 0,
            jpeg_quality: default_camera_quality(),
            zoom: default_camera_zoom(),
            captures_dir: String::new(),
            keep_days: default_captures_keep_days(),
        }
    }
}

fn default_camera_quality() -> u8 { 80 }
fn default_camera_zoom() -> f32 { 1.0 }
fn default_captures_keep_days() -> u32 { 90 }

/// 面單預產自動排程設定 — 每天到指定時間點自動反查當日整批訂單並預下載快取(headless)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreGenScheduleConfig {
    /// 總開關(預設關;設好時間並開啟後才運作)
    #[serde(default)]
    pub enabled: bool,
    /// 每天執行的時間點清單,格式 "HH:MM"(24 小時制),可多個
    #[serde(default)]
    pub times: Vec<String>,
    /// 預產來源:"clearance"(清關日期) / "transfer"(轉寄出貨),可多選
    #[serde(default = "default_pregen_sources")]
    pub sources: Vec<String>,
    /// 預產「哪一天」的訂單:相對執行日的天數位移。0 = 當天、+1 = 隔天、-1 = 前一天
    #[serde(default)]
    pub date_offset_days: i64,
    /// App 啟動時,若今日已過的排程時間點尚未跑過,是否補跑一次(預設關)。
    /// 因排程為進程內常駐 task,只有 App 開著才會到點觸發;開啟此項可在開機/重啟後補上錯過的時段。
    #[serde(default)]
    pub catch_up_on_start: bool,
}

impl Default for PreGenScheduleConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            times: Vec::new(),
            sources: default_pregen_sources(),
            date_offset_days: 0,
            catch_up_on_start: false,
        }
    }
}

fn default_pregen_sources() -> Vec<String> {
    vec!["clearance".to_string(), "transfer".to_string()]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// HTTP server 監聽 IP（預設 0.0.0.0 對外開放給工控機）
    #[serde(default = "default_listen_ip")]
    pub listen_ip: String,
    /// HTTP server 監聽 port
    #[serde(default = "default_port")]
    pub port: u16,
    /// 是否開機自動啟動 server
    #[serde(default = "default_true")]
    pub auto_start: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudConfig {
    /// 雲端 API base URL（網域），例如 https://ship.cix3752i.com
    #[serde(default)]
    pub api_base: String,
    /// 連線逾時秒數
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// 失敗重試次數
    #[serde(default = "default_retry")]
    pub retry: u32,
    /// 跳過 SSL 憑證驗證（給內網/開發環境用）
    #[serde(default)]
    pub allow_invalid_certs: bool,
    /// 發送 logistic-cat webhook 時帶的 job_user 值（識別這台機器/操作員）
    #[serde(default = "default_job_user")]
    pub job_user: String,
    /// 包裹查詢模式：`forward`（轉寄）或 `proxy`（代寄）
    #[serde(default = "default_parcel_mode")]
    pub parcel_mode: String,
    /// 轉寄訂單列印 path prefix（會自動拼上 /{queryNo}）
    #[serde(default = "default_parcel_forward_path")]
    pub parcel_forward_path: String,
    /// 代寄訂單列印 path prefix
    #[serde(default = "default_parcel_proxy_path")]
    pub parcel_proxy_path: String,
    /// 登入 / Session 驗證 path
    #[serde(default = "default_session_path")]
    pub session_path: String,
    /// 掃描列印 (操作員 UI 用) path
    #[serde(default = "default_scan_print_path")]
    pub scan_print_path: String,
    /// 面單預產 path
    #[serde(default = "default_pre_generate_path")]
    pub pre_generate_path: String,
    /// 袋號反查整袋訂單編號 (面單預產用) path
    #[serde(default = "default_package_orders_path")]
    pub package_orders_path: String,
    /// 依日期反查整批訂單編號 (面單預產用;清關/轉寄) path
    #[serde(default = "default_orders_by_date_path")]
    pub orders_by_date_path: String,
    /// 雲端列印 path
    #[serde(default = "default_cloud_print_path")]
    pub cloud_print_path: String,
    /// 自動印單包裹查詢 (examine-package) path
    #[serde(default = "default_examine_package_path")]
    pub examine_package_path: String,
    /// 清關作業 — 取得選項 (倉庫 / 清關公司 / 司機 歷史清單) path
    #[serde(default = "default_clearance_options_path")]
    pub clearance_options_path: String,
    /// 清關作業 — 新增清關包裹 path
    #[serde(default = "default_clearance_store_path")]
    pub clearance_store_path: String,
    /// 清關作業 — 司機派工 path
    #[serde(default = "default_clearance_dispatch_path")]
    pub clearance_dispatch_path: String,
    /// 清關進度浮動框 — 指定報關日區間聚合(袋數/件數/已印/剩餘)path
    #[serde(default = "default_clearance_progress_path")]
    pub clearance_progress_path: String,
    /// 入倉驗單 — 資源基底 path(子路由 /options /examine /create-package /remove-goods /remove-package /label-data)
    #[serde(default = "default_warehouse_scanner_path")]
    pub warehouse_scanner_path: String,
    /// 分揀完成 webhook path
    #[serde(default = "default_webhook_path")]
    pub webhook_path: String,
}

impl Default for CloudConfig {
    fn default() -> Self {
        Self {
            api_base: String::new(),
            timeout_secs: default_timeout(),
            retry: default_retry(),
            allow_invalid_certs: false,
            job_user: default_job_user(),
            parcel_mode: default_parcel_mode(),
            parcel_forward_path: default_parcel_forward_path(),
            parcel_proxy_path: default_parcel_proxy_path(),
            session_path: default_session_path(),
            scan_print_path: default_scan_print_path(),
            pre_generate_path: default_pre_generate_path(),
            package_orders_path: default_package_orders_path(),
            orders_by_date_path: default_orders_by_date_path(),
            cloud_print_path: default_cloud_print_path(),
            examine_package_path: default_examine_package_path(),
            clearance_options_path: default_clearance_options_path(),
            clearance_progress_path: default_clearance_progress_path(),
            clearance_store_path: default_clearance_store_path(),
            clearance_dispatch_path: default_clearance_dispatch_path(),
            warehouse_scanner_path: default_warehouse_scanner_path(),
            webhook_path: default_webhook_path(),
        }
    }
}

/// 網路健康偵測設定
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// 正常巡檢間隔(秒);至少 5
    #[serde(default = "default_net_interval")]
    pub interval_secs: u64,
    /// 連續失敗達 fail_threshold 後拉長到的間隔(秒);至少 interval_secs
    #[serde(default = "default_net_degrade_interval")]
    pub degrade_interval_secs: u64,
    /// 公網 anchor host:port(TCP connect 試水);例:1.1.1.1:443 / 8.8.8.8:443
    #[serde(default = "default_net_anchor")]
    pub anchor_addr: String,
    /// 公網 TCP connect timeout(毫秒)
    #[serde(default = "default_net_anchor_timeout_ms")]
    pub anchor_timeout_ms: u64,
    /// 雲端 API HEAD timeout(秒)
    #[serde(default = "default_net_cloud_timeout")]
    pub cloud_timeout_secs: u64,
    /// 雲端 API 巡檢節流(秒);anchor 仍每 interval_secs 跑,
    /// 雲端只有距上次檢查 >= cloud_interval_secs 才會真打,期間沿用上次結果
    #[serde(default = "default_net_cloud_interval")]
    pub cloud_interval_secs: u64,
    /// 抖動緩衝:連續失敗達此次數才標 down,否則保持 effective_ok=true
    #[serde(default = "default_net_fail_threshold")]
    pub fail_threshold: u32,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            interval_secs: default_net_interval(),
            degrade_interval_secs: default_net_degrade_interval(),
            anchor_addr: default_net_anchor(),
            anchor_timeout_ms: default_net_anchor_timeout_ms(),
            cloud_timeout_secs: default_net_cloud_timeout(),
            cloud_interval_secs: default_net_cloud_interval(),
            fail_threshold: default_net_fail_threshold(),
        }
    }
}

/// `GET /api/parcel/{queryNo}` 回應的 `label_path` 呈現模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LabelPathMode {
    /// 本機絕對路徑(預設、僅同機器可讀)
    Local,
    /// 共用目錄路徑(SMB/NFS 等;以 share_root 替換 cache_root 前綴)
    Share,
    /// HTTP URL(指向 Middleware 的 /images/{label_key} 端點)
    Http,
    /// 本機直接列印(由 App 呼叫本地印表機，回給工控機 label_path=null)
    #[serde(rename = "direct_print")]
    DirectPrint,
}

impl Default for LabelPathMode {
    fn default() -> Self { LabelPathMode::Local }
}

/// 面單路徑回傳模式設定
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelPathConfig {
    /// 回傳模式:local / share / http
    #[serde(default)]
    pub mode: LabelPathMode,
    /// share 模式用:共用目錄根路徑(例:`\\\\10.0.0.1\\labels` 或 `/Volumes/labels`)
    /// 結構需與 cache_root 之下的相對路徑對齊
    #[serde(default)]
    pub share_root: String,
}

impl Default for LabelPathConfig {
    fn default() -> Self {
        Self {
            mode: LabelPathMode::default(),
            share_root: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// 圖片快取目錄；空字串代表使用系統「圖片」資料夾（~/Pictures）
    #[serde(default)]
    pub dir: String,
    /// 保留天數，0 代表永久保留
    #[serde(default = "default_keep_days")]
    pub keep_days: u32,
    /// 最大容量 (MB)，0 代表不限
    #[serde(default)]
    pub max_size_mb: u64,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            cloud: CloudConfig::default(),
            cache: CacheConfig::default(),
            network: NetworkConfig::default(),
            label_path: LabelPathConfig::default(),
            pre_gen_schedule: PreGenScheduleConfig::default(),
            camera: CameraConfig::default(),
            sync: SyncConfig::default(),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen_ip: default_listen_ip(),
            port: default_port(),
            auto_start: true,
        }
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            dir: String::new(),
            keep_days: default_keep_days(),
            max_size_mb: 0,
        }
    }
}

fn default_listen_ip() -> String { "0.0.0.0".to_string() }
fn default_port() -> u16 { 18080 }
fn default_timeout() -> u64 { 30 }
fn default_retry() -> u32 { 3 }
fn default_keep_days() -> u32 { 5 }
fn default_true() -> bool { true }
fn default_job_user() -> String { "物流貓".to_string() }
fn default_parcel_mode() -> String { "forward".to_string() }
fn default_parcel_forward_path() -> String { "/api/v2/order-forward-print".to_string() }
fn default_parcel_proxy_path() -> String { "/api/v2/order-proxy-print".to_string() }
fn default_session_path() -> String { "/api/v1/local-middleware/session".to_string() }
fn default_scan_print_path() -> String { "/api/v1/local-middleware/label/scan-print".to_string() }
fn default_pre_generate_path() -> String { "/api/v1/local-middleware/label/pre-generate".to_string() }
fn default_cloud_print_path() -> String { "/api/v1/local-middleware/label/cloud-print".to_string() }
fn default_examine_package_path() -> String { "/api/v1/local-middleware/label/examine-package".to_string() }
fn default_package_orders_path() -> String { "/api/v1/local-middleware/label/package-orders".to_string() }
fn default_orders_by_date_path() -> String { "/api/v1/local-middleware/label/orders-by-date".to_string() }
fn default_clearance_options_path() -> String { "/api/v1/local-middleware/clearance/options".to_string() }
fn default_clearance_progress_path() -> String { "/api/v1/local-middleware/clearance/progress".to_string() }
fn default_clearance_store_path() -> String { "/api/v1/local-middleware/clearance/store".to_string() }
fn default_clearance_dispatch_path() -> String { "/api/v1/local-middleware/clearance/dispatch".to_string() }
fn default_warehouse_scanner_path() -> String { "/api/v1/local-middleware/warehouse-scanner".to_string() }
fn default_webhook_path() -> String { "/webhook/logistic-cat".to_string() }
fn default_net_interval() -> u64 { 15 }
fn default_net_degrade_interval() -> u64 { 60 }
fn default_net_anchor() -> String { "1.1.1.1:443".to_string() }
fn default_net_anchor_timeout_ms() -> u64 { 1500 }
fn default_net_cloud_timeout() -> u64 { 3 }
fn default_net_cloud_interval() -> u64 { 180 }
fn default_net_fail_threshold() -> u32 { 2 }

impl AppConfig {
    /// 載入設定；檔案不存在時建立預設值並寫入
    pub async fn load(handle: &AppHandle) -> AppResult<Self> {
        let path = config_path(handle)?;

        if !path.exists() {
            let cfg = AppConfig::default();
            cfg.write_to(&path).await?;
            return Ok(cfg);
        }

        let content = fs::read_to_string(&path).await?;
        let cfg: AppConfig = toml::from_str(&content)?;
        Ok(cfg)
    }

    /// 寫入設定到磁碟
    pub async fn save(&self, handle: &AppHandle) -> AppResult<()> {
        let path = config_path(handle)?;
        self.write_to(&path).await
    }

    async fn write_to(&self, path: &PathBuf) -> AppResult<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let content = toml::to_string_pretty(self)?;
        fs::write(path, content).await?;
        Ok(())
    }

    /// 解析實際快取目錄；空字串會 fallback 到系統「圖片」資料夾（~/Pictures）
    /// 若取不到（罕見：headless / 系統未定義 picture_dir），最後退回 app_data/cache/labels
    pub fn resolved_cache_dir(&self, handle: &AppHandle) -> AppResult<PathBuf> {
        if !self.cache.dir.is_empty() {
            return Ok(PathBuf::from(&self.cache.dir));
        }
        if let Ok(pic) = handle.path().picture_dir() {
            return Ok(pic);
        }
        let app_data = handle
            .path()
            .app_data_dir()
            .map_err(|e| AppError::Config(format!("無法取得 app_data 目錄: {e}")))?;
        Ok(app_data.join("cache").join("labels"))
    }

    /// 解析存證快照目錄 —— **與 `resolved_cache_dir` 分離**,不放在面單快取下,
    /// 避免被面單快取的 keep_days 清掉。留白時預設 `{app_data}/captures`。
    pub fn resolved_captures_dir(&self, handle: &AppHandle) -> AppResult<PathBuf> {
        if !self.camera.captures_dir.is_empty() {
            return Ok(PathBuf::from(&self.camera.captures_dir));
        }
        let app_data = handle
            .path()
            .app_data_dir()
            .map_err(|e| AppError::Config(format!("無法取得 app_data 目錄: {e}")))?;
        Ok(app_data.join("captures"))
    }
}

fn config_path(handle: &AppHandle) -> AppResult<PathBuf> {
    let app_data = handle
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Config(format!("無法取得 app_data 目錄: {e}")))?;
    Ok(app_data.join("config.toml"))
}
