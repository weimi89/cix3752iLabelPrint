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
}

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
}

impl Default for PreGenScheduleConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            times: Vec::new(),
            sources: default_pregen_sources(),
            date_offset_days: 0,
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
}

fn config_path(handle: &AppHandle) -> AppResult<PathBuf> {
    let app_data = handle
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Config(format!("無法取得 app_data 目錄: {e}")))?;
    Ok(app_data.join("config.toml"))
}
