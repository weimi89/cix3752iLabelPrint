use serde::{Serialize, Serializer};
use thiserror::Error;

/// 應用通用錯誤
#[derive(Debug, Error)]
pub enum AppError {
    #[error("IO 錯誤: {0}")]
    Io(#[from] std::io::Error),

    #[error("資料庫錯誤: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Migration 錯誤: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

    #[error("HTTP 錯誤: {0}")]
    Http(#[from] reqwest::Error),

    #[error("序列化錯誤: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("Toml 解析錯誤: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("Toml 寫入錯誤: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error("Tauri 錯誤: {0}")]
    Tauri(#[from] tauri::Error),

    #[error("Keyring 錯誤: {0}")]
    Keyring(#[from] keyring::Error),

    #[error("設定錯誤: {0}")]
    Config(String),

    #[error("雲端未登入或 token 失效")]
    Unauthorized,

    #[error("雲端錯誤 [{code}]: {message}")]
    Cloud {
        code: String,
        message: String,
        /// 雲端錯誤 body 帶出的物流商代碼(查得到訂單的業務錯誤才有,如 STORE_CLOSED / UNCONFIRMED)。
        /// 工控機錯誤面單據此解析分揀通道,讓異常包裹進對應格口列印。
        shipping_provider: Option<String>,
        /// 雲端錯誤 body 帶出的物流單號(查得到訂單的業務錯誤才有)。
        /// 工控機掃的 query_no 是條碼原值(可能經正規化),這裡記錄雲端精確的物流單號供異常清單對單。
        shipping_no: Option<String>,
    },

    #[error("印表機錯誤: {0}")]
    Printer(String),

    #[error("Server 錯誤: {0}")]
    Server(String),

    #[error("資料不存在: {0}")]
    NotFound(String),

    #[error("{0}")]
    Other(String),
}

pub type AppResult<T> = Result<T, AppError>;

/// Tauri command 回傳到前端時要可被 serde 序列化
impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
