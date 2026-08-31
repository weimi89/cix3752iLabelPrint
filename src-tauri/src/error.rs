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
        /// 雲端錯誤 body 帶出的袋號(有跑 recordNotOutboundPrint 的業務錯誤才有:UNCONFIRMED / STORE_CLOSED / STATUS_ABNORMAL)。
        /// 供設備端件數核對把此件標記為已印(對齊成功列印的 on_parcel)。
        package_sn: Option<String>,
        /// 雲端錯誤 body 帶出的訂單編號(同 package_sn)。
        order_sn: Option<String>,
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

impl AppError {
    /// 給前端判讀的錯誤分類。前端據此決定要顯示哪一句使用者看得懂的話 ——
    /// 少了它,前端只拿得到 Display 出來的技術字串,只能整串貼上畫面
    /// (操作員會看到 reqwest 的英文訊息與完整雲端網址)。
    ///
    /// - `network`：連不上雲端 / 雲端沒回應。使用者只需知道「連線有問題」,不必看網址與狀態碼。
    /// - `unauthorized`：雲端未登入或 token 失效,使用者要去重新登入。
    /// - `cloud`：雲端回的業務錯誤(門市關轉、未確認…),`message` 本身就是給人看的,原樣顯示。
    /// - `input`：本機自己擋下的輸入 / 設定問題,`message` 是我們寫的中文說明,原樣顯示。
    /// - `internal`：本機 IO / DB / 序列化等內部故障,使用者無從處理,只提示並留紀錄。
    pub fn kind(&self) -> &'static str {
        match self {
            AppError::Http(_) => "network",
            AppError::Unauthorized => "unauthorized",
            AppError::Cloud { .. } => "cloud",
            AppError::Config(_)
            | AppError::Printer(_)
            | AppError::Server(_)
            | AppError::NotFound(_)
            | AppError::Other(_) => "input",
            AppError::Io(_)
            | AppError::Database(_)
            | AppError::Migration(_)
            | AppError::Serde(_)
            | AppError::Toml(_)
            | AppError::TomlSerialize(_)
            | AppError::Tauri(_)
            | AppError::Keyring(_) => "internal",
        }
    }
}

/// Tauri command 回傳到前端時要可被 serde 序列化。
///
/// 序列化成物件而非單一字串:前端要靠 `kind` 才翻得出使用者看得懂的話。
/// `message` 欄位刻意保留原本的完整技術訊息 —— 供 console 與問題回報用,
/// 且舊的 `e?.message` 取法照樣拿得到值,不會因為改成物件而變成 undefined。
impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut st = serializer.serialize_struct("AppError", 2)?;
        st.serialize_field("kind", self.kind())?;
        st.serialize_field("message", &self.to_string())?;
        st.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 前端全靠 `kind` 決定要對操作員說哪一句話。若這裡漏了分類或分錯類,
    /// 畫面就會退回顯示 Display 出來的技術訊息(reqwest 英文 + 完整雲端網址)。
    #[test]
    fn 序列化帶得出分類與訊息() {
        let e = AppError::Server("通道代碼格式不符".into());
        let v: serde_json::Value = serde_json::to_value(&e).unwrap();
        assert_eq!(v["kind"], "input");
        // input / cloud 的訊息是寫給人看的,前端會原樣顯示
        assert!(v["message"].as_str().unwrap().contains("通道代碼格式不符"));

        let e = AppError::Unauthorized;
        assert_eq!(serde_json::to_value(&e).unwrap()["kind"], "unauthorized");

        let e = AppError::Cloud {
            code: "STORE_CLOSED".into(),
            message: "門市關轉".into(),
            shipping_provider: None,
            shipping_no: None,
            package_sn: None,
            order_sn: None,
        };
        assert_eq!(serde_json::to_value(&e).unwrap()["kind"], "cloud");

        let e = AppError::Config("設定檔壞了".into());
        assert_eq!(serde_json::to_value(&e).unwrap()["kind"], "input");
    }

    /// 本機內部故障不該把 IO / SQL 細節丟到畫面上,一律歸 internal 由前端統一措辭
    #[test]
    fn 內部故障歸為_internal() {
        let e = AppError::Io(std::io::Error::other("boom"));
        assert_eq!(e.kind(), "internal");
        let e = AppError::Database(sqlx::Error::RowNotFound);
        assert_eq!(e.kind(), "internal");
    }
}
