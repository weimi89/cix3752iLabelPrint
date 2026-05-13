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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CloudConfig {
    /// 雲端 API base URL，例如 https://example.com
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// 圖片快取目錄；空字串代表使用 app_data/cache/labels
    #[serde(default)]
    pub dir: String,
    /// 保留天數，0 代表永久保留
    #[serde(default = "default_keep_days")]
    pub keep_days: u32,
    /// 最大容量 (MB)，0 代表不限
    #[serde(default)]
    pub max_size_mb: u64,
    /// 是否啟用背景補下載
    #[serde(default = "default_true")]
    pub background_prefetch: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            cloud: CloudConfig::default(),
            cache: CacheConfig::default(),
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
            background_prefetch: true,
        }
    }
}

fn default_listen_ip() -> String { "0.0.0.0".to_string() }
fn default_port() -> u16 { 18080 }
fn default_timeout() -> u64 { 30 }
fn default_retry() -> u32 { 3 }
fn default_keep_days() -> u32 { 14 }
fn default_true() -> bool { true }

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

    /// 解析實際快取目錄；空字串會 fallback 到 app_data/cache/labels
    pub fn resolved_cache_dir(&self, handle: &AppHandle) -> AppResult<PathBuf> {
        if !self.cache.dir.is_empty() {
            return Ok(PathBuf::from(&self.cache.dir));
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
