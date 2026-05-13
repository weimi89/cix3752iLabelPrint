use serde::{Deserialize, Serialize};

/// 雲端 API 回傳的包裹查詢結果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParcelInfo {
    pub success: bool,
    pub tracking_no: String,
    pub status: String,
    pub sort_channel: Option<String>,
    #[serde(default)]
    pub should_print: bool,
    pub print_profile: Option<String>,
    pub label_source: String,
    pub label_url: Option<String>,
    pub label_path: Option<String>,
    pub label_key: Option<String>,
}

/// 工控機回報的執行結果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportPayload {
    pub tracking_no: String,
    pub sort_channel: Option<String>,
    pub print_profile: Option<String>,
    pub print_result: String,
    pub sort_result: String,
    pub error_message: Option<String>,
    pub executed_at: String,
}

/// 雲端登入 session 資訊（不含 token，token 從 keyring 取）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CloudSession {
    pub api_base: String,
    pub logged_in: bool,
    pub user_label: Option<String>,
}

/// 訂單列印單筆結果（給操作員 UI 的「掃描列印」用，對齊既有 Web 回傳）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrintViewResult {
    pub print_view_status: String,
    pub print_shipping_no: Option<String>,
    pub print_shipping_provider: Option<String>,
    pub print_file_path: Option<String>,
    #[serde(default)]
    pub print_time: Vec<String>,
}
