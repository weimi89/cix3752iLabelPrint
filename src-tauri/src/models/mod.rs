use serde::{Deserialize, Serialize};

/// 雲端 `/api/v2/order-forward-print/{queryNo}` 成功回應的外層
#[derive(Debug, Clone, Deserialize)]
pub struct CloudOrderResponse {
    pub data: ParcelInfo,
}

/// 雲端回應的 data 內容（對應雲端 API 真實格式）
#[derive(Debug, Clone, Deserialize)]
pub struct ParcelInfo {
    pub order_sn: String,
    pub shipping_no: String,
    /// 物流商代碼：7/F/O/C/H/P/S/A/J/E
    pub shipping_provider: String,
    /// v2: CDN URL；v1: base64 PNG（目前只支援 v2）
    pub shipping_image: String,
    /// 列印記錄 ID（debug 模式不回，所以是 Option）
    #[serde(default)]
    pub response_id: Option<i64>,
}

/// 統一的成功回應包裝：`{ "message": "OK", "data": {...} }`
/// 目前僅 `/healthz` 使用；業務 API（parcel、report）只用 `DataEnvelope` 包 `data`
#[derive(Debug, Clone, Serialize)]
pub struct SuccessEnvelope<T: Serialize> {
    pub message: String,
    pub data: T,
}

impl<T: Serialize> SuccessEnvelope<T> {
    pub fn ok(data: T) -> Self {
        Self {
            message: "OK".to_string(),
            data,
        }
    }
}

/// 業務 API 的成功回應包裝：`{ "data": {...} }`（無 message）
#[derive(Debug, Clone, Serialize)]
pub struct DataEnvelope<T: Serialize> {
    pub data: T,
}

impl<T: Serialize> DataEnvelope<T> {
    pub fn new(data: T) -> Self {
        Self { data }
    }
}

/// 工控機面向的包裹資料
/// channel_code / print_profile 來自本地 sort_channels & printer_profile 配置
#[derive(Debug, Clone, Serialize)]
pub struct ParcelData {
    pub channel_code: Option<String>,
    pub print_profile: Option<String>,
    pub label_path: Option<String>,
    pub response_id: Option<i64>,
}

/// 工控機看到的錯誤回應：`{ "message": "...", "status_code": 404 }`
#[derive(Debug, Clone, Serialize)]
pub struct ApiErrorBody {
    pub message: String,
    pub status_code: u16,
}

/// 工控機 POST /api/report 提交的內容（只含參考鍵）
/// 同時也是推給雲端的 payload —— 雲端用 response_id 自行對應原查詢
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportPayload {
    pub response_id: i64,
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

/// 包裹查詢結果（自動印單第一步：掃包裹條碼取訂單清單）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExaminePackageResult {
    pub respond_code: String,
    #[serde(default)]
    pub respond_message: Option<String>,
    #[serde(default)]
    pub package_sn: Option<String>,
    #[serde(default)]
    pub orders: Vec<ExaminePackageOrder>,
}

/// 包裹內單筆訂單摘要（webhook 端不在乎詳細結構,字段全選擇性）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExaminePackageOrder {
    #[serde(default)]
    pub order_sn: Option<String>,
    #[serde(default)]
    pub shipping_no: Option<String>,
    #[serde(default)]
    pub shipping_provider: Option<String>,
    #[serde(default)]
    pub last_print_time: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    /// 其他額外欄位由前端依需要使用
    #[serde(flatten)]
    pub extras: serde_json::Map<String, serde_json::Value>,
}

/// 雲端列印單筆結果（自動印單 cloud-print 端點專用，回應 schema 與 PrintViewResult 不同）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudPrintResult {
    pub respond_code: String,
    #[serde(default)]
    pub respond_message: Option<String>,
    #[serde(default)]
    pub shipment_no: Option<String>,
    #[serde(default)]
    pub package_sn: Option<String>,
    #[serde(default)]
    pub provider_code: Option<String>,
    #[serde(default)]
    pub image_path: Option<String>,
}
