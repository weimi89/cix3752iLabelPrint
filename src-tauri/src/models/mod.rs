use serde::{Deserialize, Serialize};

/// 雲端 `/api/v2/order-forward-print/{queryNo}` 成功回應的外層
#[derive(Debug, Clone, Deserialize)]
pub struct CloudOrderResponse {
    pub data: ParcelInfo,
}

/// 雲端回應的 data 內容(對應雲端 API 真實格式)
#[derive(Debug, Clone, Deserialize)]
pub struct ParcelInfo {
    pub order_sn: String,
    pub shipping_no: String,
    /// 物流商代碼:7/F/O/C/H/P/S/A/J/E
    pub shipping_provider: String,
    /// v2: CDN URL;v1: base64 PNG(目前只支援 v2)
    pub shipping_image: String,
    /// 累計列印次數(雲端維護,本次列印應顯示在面單上)
    #[serde(default)]
    pub print_num: Option<u32>,
    /// 列印記錄 ID(debug 模式不回,所以是 Option)
    #[serde(default)]
    pub response_id: Option<i64>,
    /// 集包單號(袋號);雲端 v2 回傳新增,舊版雲端未回時為 None → 視為散單不納入袋件核對
    #[serde(default)]
    pub package_sn: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_path: Option<String>,
    pub response_id: Option<i64>,
    /// 此筆為「錯誤面單」(雲端業務錯誤時自動產生的提示圖)。
    /// 工控機收到 true 時應把 label_path 當一般面單印出,但**不要** POST /api/report
    /// (錯誤面單無 response_id,僅供現場辨識撿出異常包裹)。
    /// 正常面單時此欄省略(預設 false)。
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_error_label: bool,
    /// 錯誤面單對應的機器可讀 code(STORE_CLOSED / NOT_FOUND / …),正常面單省略。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    /// 錯誤面單的人類可讀訊息(雲端回的原始 message),正常面單省略。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// serde skip helper：bool 為 false 時不序列化
fn is_false(b: &bool) -> bool {
    !*b
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

/// 訂單列印單筆結果(給操作員 UI 的「掃描列印」用,對齊既有 Web 回傳)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrintViewResult {
    pub print_view_status: String,
    pub print_shipping_no: Option<String>,
    pub print_shipping_provider: Option<String>,
    pub print_file_path: Option<String>,
    #[serde(default)]
    pub print_time: Vec<String>,
    /// 累計列印次數(雲端 addRepeatWatermark=false,middleware 自行依此值疊加浮水印)
    #[serde(default)]
    pub print_num: Option<u32>,
    /// 查詢失敗時 middleware 產生的「錯誤面單」URL(http://127.0.0.1:port/images/@error/...);
    /// 前端用該物流商在「印表機設定」的同一台印表機印出,與正常面單同出口(避免印表機來源不一致)。
    /// 成功或不需錯誤面單時為 None。由 middleware 填,雲端不回此欄位故需 default。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_label_path: Option<String>,
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

/// 雲端列印單筆結果(自動印單 cloud-print 端點專用,回應 schema 與 PrintViewResult 不同)
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
    /// 雲端相對路徑(供 middleware 推 cache key 用)
    #[serde(default)]
    pub file_path: Option<String>,
    /// 累計列印次數(PRINT-SUCCESS 時才帶)
    #[serde(default)]
    pub print_num: Option<u32>,
    /// 印單失敗時 middleware 產生的「錯誤面單」URL(同 PrintViewResult.error_label_path 語意);
    /// 前端用 provider_code 對應的印表機印出。成功或不需錯誤面單時為 None。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_label_path: Option<String>,
}

/// 袋號反查整袋訂單編號結果(面單預產用)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageOrdersResult {
    pub respond_code: String,
    #[serde(default)]
    pub respond_message: Option<String>,
    #[serde(default)]
    pub package_sn: Option<String>,
    #[serde(default)]
    pub order_sns: Vec<String>,
    /// 依清關日期查詢時,當日袋號數(袋號反查單袋時為 None)
    #[serde(default)]
    pub package_count: Option<i64>,
}
