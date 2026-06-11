use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use reqwest::Client;
use serde_json::json;

use crate::config::AppConfig;
use crate::models::{
    CloudOrderResponse, CloudPrintResult, CloudSession, ExaminePackageResult,
    PackageOrdersResult, ParcelInfo, PrintViewResult,
};
use crate::{AppError, AppResult};

const KEYRING_SERVICE: &str = "com.weiminet.cix3752i.labelprint";
const KEYRING_USER: &str = "cloud_api_token";

/// 雲端 API client — 封裝 Bearer Token 認證、retry、timeout
#[derive(Clone)]
pub struct CloudClient {
    inner: Arc<Inner>,
}

struct Inner {
    /// http client 用 RwLock 包裝，使 apply_config 在 allow_invalid_certs / timeout
    /// 改變時可以 swap 進新 client（reqwest Client 是 Clone(Arc) 取出時 clone 即可）
    http: RwLock<Client>,
    state: RwLock<CloudState>,
}

#[derive(Default, Clone)]
struct CloudState {
    api_base: String,
    token: String,
    timeout: u64,
    retry: u32,
    logged_in: bool,
    user_label: Option<String>,
    /// 發 webhook 時帶的 job_user 值
    job_user: String,
    /// 包裹查詢模式：forward / proxy
    parcel_mode: String,
    // 各 endpoint path（相對於 api_base，使用者可在設定頁面調整）
    parcel_forward_path: String,
    parcel_proxy_path: String,
    session_path: String,
    scan_print_path: String,
    pre_generate_path: String,
    cloud_print_path: String,
    examine_package_path: String,
    package_orders_path: String,
    orders_by_date_path: String,
    webhook_path: String,
}

impl CloudClient {
    pub fn new(config: &AppConfig) -> Self {
        let http = build_http_client(config.cloud.timeout_secs, config.cloud.allow_invalid_certs);

        // 啟動時嘗試從 keyring 還原 token；有 token 即視為已登入（首次打雲端遇 401 才會降級）
        let token = load_token_from_keyring().unwrap_or_default();
        let has_token = !token.is_empty();

        let state = CloudState {
            api_base: trim_base(&config.cloud.api_base),
            token,
            timeout: config.cloud.timeout_secs,
            retry: config.cloud.retry,
            logged_in: has_token,
            user_label: None,
            job_user: config.cloud.job_user.clone(),
            parcel_mode: config.cloud.parcel_mode.clone(),
            parcel_forward_path: config.cloud.parcel_forward_path.clone(),
            parcel_proxy_path: config.cloud.parcel_proxy_path.clone(),
            session_path: config.cloud.session_path.clone(),
            scan_print_path: config.cloud.scan_print_path.clone(),
            pre_generate_path: config.cloud.pre_generate_path.clone(),
            cloud_print_path: config.cloud.cloud_print_path.clone(),
            examine_package_path: config.cloud.examine_package_path.clone(),
            package_orders_path: config.cloud.package_orders_path.clone(),
            orders_by_date_path: config.cloud.orders_by_date_path.clone(),
            webhook_path: config.cloud.webhook_path.clone(),
        };

        Self {
            inner: Arc::new(Inner {
                http: RwLock::new(http),
                state: RwLock::new(state),
            }),
        }
    }

    /// 套用新的設定（API base / timeout / SSL 旗標 / job_user / 各 endpoint path 等）
    /// allow_invalid_certs / timeout 變動會 swap 進新的 http client
    pub fn apply_config(&self, config: &AppConfig) {
        // 重建 http client 套用 SSL 旗標
        let new_http = build_http_client(config.cloud.timeout_secs, config.cloud.allow_invalid_certs);
        *self.inner.http.write() = new_http;

        let mut s = self.inner.state.write();
        s.api_base = trim_base(&config.cloud.api_base);
        s.timeout = config.cloud.timeout_secs;
        s.retry = config.cloud.retry;
        s.job_user = config.cloud.job_user.clone();
        s.parcel_mode = config.cloud.parcel_mode.clone();
        s.parcel_forward_path = config.cloud.parcel_forward_path.clone();
        s.parcel_proxy_path = config.cloud.parcel_proxy_path.clone();
        s.session_path = config.cloud.session_path.clone();
        s.scan_print_path = config.cloud.scan_print_path.clone();
        s.pre_generate_path = config.cloud.pre_generate_path.clone();
        s.cloud_print_path = config.cloud.cloud_print_path.clone();
        s.examine_package_path = config.cloud.examine_package_path.clone();
        s.package_orders_path = config.cloud.package_orders_path.clone();
        s.orders_by_date_path = config.cloud.orders_by_date_path.clone();
        s.webhook_path = config.cloud.webhook_path.clone();
    }

    /// 儲存 token（不打雲端做驗證）
    /// 業務 API（order-forward-print / order-proxy-print）首次呼叫時自然會驗證 token。
    /// 寫入 keyring 並更新 state。
    pub async fn login(&self, api_base: &str, token: &str) -> AppResult<CloudSession> {
        let base = trim_base(api_base);
        save_token_to_keyring(token)?;

        {
            let mut s = self.inner.state.write();
            s.api_base = base.clone();
            s.token = token.to_string();
            s.logged_in = true;
            s.user_label = None;
        }

        Ok(CloudSession {
            api_base: base,
            logged_in: true,
            user_label: None,
        })
    }

    /// 登出：清掉記憶體 token 與 keyring
    pub fn logout(&self) -> AppResult<()> {
        clear_token_from_keyring().ok();
        let mut s = self.inner.state.write();
        s.token.clear();
        s.logged_in = false;
        s.user_label = None;
        Ok(())
    }

    /// 取得目前 session 狀態（給前端顯示）
    pub fn session(&self) -> CloudSession {
        let s = self.inner.state.read();
        CloudSession {
            api_base: s.api_base.clone(),
            logged_in: s.logged_in && !s.token.is_empty(),
            user_label: s.user_label.clone(),
        }
    }

    /// 依 queryNo 查包裹（v2）
    /// 依 cloud.parcel_mode 設定走 forward 或 proxy 端點：
    ///   - forward: GET {api_base}{parcel_forward_path}/{queryNo}
    ///   - proxy:   GET {api_base}{parcel_proxy_path}/{queryNo}
    /// queryNo 可傳訂單編號、配送單號或可解析的物流條碼
    /// 注意：v2 order-*-print 端點不需要 Bearer Token
    pub async fn fetch_parcel(&self, query_no: &str) -> AppResult<ParcelInfo> {
        let base = self.snapshot_no_auth()?;
        let path = {
            let s = self.inner.state.read();
            if s.parcel_mode.eq_ignore_ascii_case("proxy") {
                s.parcel_proxy_path.clone()
            } else {
                s.parcel_forward_path.clone()
            }
        };
        let url = format!("{}/{}", join_url(&base, &path), query_no);

        let http = self.inner.http.read().clone();
        let resp = http.get(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            // 401 維持既有語意(未登入);其餘解析雲端 dingo 錯誤 body 的 message
            //(errorFormat: { "message": "..." }),保留具體狀態(門市關轉 / 未確認 / 找不到 …)
            // 供前端分類播放對應提示音
            if status.as_u16() == 401 {
                return Err(AppError::Unauthorized);
            }
            let body = resp.text().await.unwrap_or_default();
            let json: Option<serde_json::Value> = serde_json::from_str(&body).ok();
            // 優先用雲端回的機器可讀 code(STORE_CLOSED / UNCONFIRMED / NOT_FOUND …);
            // 舊版雲端未回 code 時 fallback 成 HTTP 狀態碼字串
            let code = json
                .as_ref()
                .and_then(|v| v.get("code").and_then(|c| c.as_str()))
                .map(|s| s.to_string())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| status.as_u16().to_string());
            let message = json
                .as_ref()
                .and_then(|v| v.get("message").and_then(|m| m.as_str()))
                .map(|s| s.to_string())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| format!("雲端回應 HTTP {}", status.as_u16()));
            // 查得到訂單的業務錯誤(STORE_CLOSED / UNCONFIRMED …)雲端會帶物流商代碼,
            // 供錯誤面單照正常流程解析分揀通道;NOT_FOUND 等查無訂單時為 None
            let shipping_provider = parse_error_shipping_provider(json.as_ref());
            return Err(AppError::Cloud { code, message, shipping_provider });
        }

        let envelope: CloudOrderResponse = resp.json().await?;
        Ok(envelope.data)
    }

    /// 分揀完成 webhook 通知 logistic-cat 系統
    /// URL 為 `{cloud.api_base}/webhook/logistic-cat`（共用雲端 API 設定頁的 base URL）
    /// 自動把設定中的 job_user 注入 payload；無 Bearer Auth
    pub async fn notify_logistic_cat(&self, payload: &mut serde_json::Value) -> AppResult<()> {
        let (api_base, job_user, webhook_path) = {
            let s = self.inner.state.read();
            (s.api_base.clone(), s.job_user.clone(), s.webhook_path.clone())
        };
        if api_base.is_empty() {
            return Err(AppError::Server(
                "雲端 API base URL 尚未設定，無法發送 webhook".to_string(),
            ));
        }
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("job_user".to_string(), serde_json::Value::String(job_user));
        }
        let url = join_url(&api_base, &webhook_path);
        let http = self.inner.http.read().clone();
        http.post(&url)
            .json(payload)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    /// 操作員 UI 用：取得單張面單（對齊 web 端 print_view_status 回應格式）
    pub async fn fetch_label_for_print(
        &self,
        order_sn: &str,
        print_type: &[String],
        enforce: bool,
        mode: LabelFetchMode,
        scanner_user: Option<&str>,
        sticker_user: Option<&str>,
    ) -> AppResult<PrintViewResult> {
        let (base, token) = self.snapshot()?;

        let path = {
            let s = self.inner.state.read();
            match mode {
                LabelFetchMode::WebPrint => s.scan_print_path.clone(),
                LabelFetchMode::Download => s.pre_generate_path.clone(),
                LabelFetchMode::CloudPrint => s.cloud_print_path.clone(),
            }
        };
        let url = join_url(&base, &path);

        let body = json!({
            "order_sn": order_sn,
            "print_type": print_type,
            "enforce": enforce,
            "scanner_user": scanner_user.unwrap_or(""),
            "sticker_user": sticker_user.unwrap_or(""),
        });

        let http = self.inner.http.read().clone();
        let resp = http
            .post(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            if status.as_u16() == 401 {
                return Err(AppError::Unauthorized);
            }
            let body_text = resp.text().await.unwrap_or_default();
            let json: Option<serde_json::Value> = serde_json::from_str(&body_text).ok();
            let code = json
                .as_ref()
                .and_then(|v| v.get("code").and_then(|c| c.as_str()))
                .map(|s| s.to_string())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| status.as_u16().to_string());
            let message = json
                .as_ref()
                .and_then(|v| v.get("message").and_then(|m| m.as_str()))
                .map(|s| s.to_string())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| format!("雲端回應 HTTP {}", status.as_u16()));
            let shipping_provider = parse_error_shipping_provider(json.as_ref());
            return Err(AppError::Cloud { code, message, shipping_provider });
        }

        let mut result: PrintViewResult = resp.json().await?;
        // 雲端可能回相對路徑（例 /data/labels/.../xxx.png），補上 api_base 變完整 URL，
        // 前端 <img src> 才載得到
        if let Some(path) = result.print_file_path.as_ref() {
            if path.starts_with('/') && !path.starts_with("//") {
                result.print_file_path = Some(format!("{}{}", base, path));
            }
        }
        Ok(result)
    }

    /// 自動印單第一步：掃包裹條碼取訂單清單
    pub async fn examine_package(&self, shipment_no: &str) -> AppResult<ExaminePackageResult> {
        let (base, token) = self.snapshot()?;
        let path = self.inner.state.read().examine_package_path.clone();
        let url = join_url(&base, &path);

        let body = json!({ "shipment_no": shipment_no });

        let http = self.inner.http.read().clone();
        let resp = http
            .post(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;

        let text = resp.text().await?;
        let result: ExaminePackageResult = serde_json::from_str(&text).map_err(|e| {
            AppError::Server(format!(
                "雲端 examine-package 回應解析失敗: {e}; body: {}",
                text.chars().take(500).collect::<String>()
            ))
        })?;
        Ok(result)
    }

    /// 面單預產用:袋號反查整袋訂單編號(對齊雲端 label/package-orders)
    pub async fn fetch_package_orders(&self, package_sn: &str) -> AppResult<PackageOrdersResult> {
        let (base, token) = self.snapshot()?;
        let path = self.inner.state.read().package_orders_path.clone();
        let url = join_url(&base, &path);

        let body = json!({ "package_sn": package_sn });

        let http = self.inner.http.read().clone();
        let resp = http
            .post(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;

        let text = resp.text().await?;
        let result: PackageOrdersResult = serde_json::from_str(&text).map_err(|e| {
            AppError::Server(format!(
                "雲端 package-orders 回應解析失敗: {e}; body: {}",
                text.chars().take(500).collect::<String>()
            ))
        })?;
        Ok(result)
    }

    /// 面單預產用:依日期反查整批訂單編號(對齊雲端 label/orders-by-date;source: clearance/transfer)
    pub async fn fetch_orders_by_date(
        &self,
        date: &str,
        source: &str,
    ) -> AppResult<PackageOrdersResult> {
        let (base, token) = self.snapshot()?;
        let path = self.inner.state.read().orders_by_date_path.clone();
        let url = join_url(&base, &path);

        let body = json!({ "date": date, "source": source });

        let http = self.inner.http.read().clone();
        let resp = http
            .post(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;

        let text = resp.text().await?;
        let result: PackageOrdersResult = serde_json::from_str(&text).map_err(|e| {
            AppError::Server(format!(
                "雲端 orders-by-date 回應解析失敗: {e}; body: {}",
                text.chars().take(500).collect::<String>()
            ))
        })?;
        Ok(result)
    }

    /// 自動印單用：打 cloud-print 端點，回應 schema 與 PrintViewResult 不同
    /// （respond_code / shipment_no / provider_code / image_path / respond_message）
    pub async fn fetch_cloud_print_label(
        &self,
        order_sn: &str,
        print_type: &[String],
        enforce: bool,
        package_sn: Option<&str>,
        scanner_user: Option<&str>,
        sticker_user: Option<&str>,
    ) -> AppResult<CloudPrintResult> {
        let (base, token) = self.snapshot()?;

        let path = self.inner.state.read().cloud_print_path.clone();
        let url = join_url(&base, &path);

        // 對齐雲端 controller：print_type 是 array,enforce 用 0/1 numeric
        let body = json!({
            "order_sn": order_sn,
            "print_type": print_type,
            "enforce": if enforce { 1 } else { 0 },
            "package_sn": package_sn.unwrap_or(""),
            "scanner_user": scanner_user.unwrap_or(""),
            "sticker_user": sticker_user.unwrap_or(""),
        });

        let http = self.inner.http.read().clone();
        let resp = http
            .post(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;

        // 抓 raw text → 失敗時把 body 寫進 error message,協助診斷雲端回應 schema 不符
        let text = resp.text().await?;
        let mut result: CloudPrintResult = serde_json::from_str(&text).map_err(|e| {
            AppError::Server(format!(
                "雲端 cloud-print 回應解析失敗: {e}; body: {}",
                text.chars().take(500).collect::<String>()
            ))
        })?;
        if let Some(p) = result.image_path.as_ref() {
            if p.starts_with('/') && !p.starts_with("//") {
                result.image_path = Some(format!("{}{}", base, p));
            }
        }
        Ok(result)
    }

    /// 從任意 URL 下載圖片 bytes（給 print_image 處理雲端 URL 用）
    /// reuse cloud 的 http client → 自動套用 allow_invalid_certs / timeout 設定
    pub async fn fetch_image_bytes(&self, url: &str) -> AppResult<Vec<u8>> {
        let http = self.inner.http.read().clone();
        let resp = http.get(url).send().await?.error_for_status()?;
        let bytes = resp.bytes().await?;
        Ok(bytes.to_vec())
    }

    fn snapshot(&self) -> AppResult<(String, String)> {
        let (api_base, token) = {
            let s = self.inner.state.read();
            (s.api_base.clone(), s.token.clone())
        };
        if api_base.is_empty() || token.is_empty() {
            return Err(AppError::Unauthorized);
        }
        Ok((api_base, token))
    }

    /// 給「不需要 Bearer Token 的 endpoint」用（例如 v2 order-*-print、webhook）
    /// 只檢查 api_base 是否設定
    fn snapshot_no_auth(&self) -> AppResult<String> {
        let api_base = self.inner.state.read().api_base.clone();
        if api_base.is_empty() {
            return Err(AppError::Server(
                "雲端 API base URL 尚未設定".to_string(),
            ));
        }
        Ok(api_base)
    }

    /// 給網路偵測用：對 session_path 發 HEAD,只看「端點是否可達」
    /// 200 / 401 都視為 Reachable(網路通,401 只是 token 失效);
    /// 連線失敗 / timeout / 5xx 視為 Unreachable;
    /// api_base 未設定回 NotConfigured。
    pub async fn head_session(&self, timeout_secs: u64) -> CloudReachResult {
        // 有 token 就 bearer auth(反映「業務可用性」:已登入應回 200,token 失效回 401);
        // 沒 token 就 anonymous(server 必定回 401,代表未登入)。
        let (api_base, session_path, token) = {
            let s = self.inner.state.read();
            (s.api_base.clone(), s.session_path.clone(), s.token.clone())
        };
        if api_base.is_empty() {
            return CloudReachResult::NotConfigured;
        }
        let url = join_url(&api_base, &session_path);
        let http = self.inner.http.read().clone();
        let started = std::time::Instant::now();
        let mut req = http
            .head(&url)
            .timeout(Duration::from_secs(timeout_secs.max(1)));
        if !token.is_empty() {
            req = req.bearer_auth(&token);
        }
        let result = req.send().await;
        let latency_ms = started.elapsed().as_millis() as u64;
        match result {
            Ok(resp) => {
                // 2xx/3xx/4xx 視為 Reachable(網路通,401/404 屬 application 層);
                // 5xx 視為 Unreachable — 即使 TCP/TLS 握手通,server 端真故障對工控機業務
                // (查包裹)等同雲端不可用,要降級(UI 亮黃)讓現場人員察覺。
                let status = resp.status().as_u16();
                if status >= 500 {
                    CloudReachResult::Unreachable {
                        error: format!("HTTP {status}"),
                        latency_ms,
                    }
                } else {
                    CloudReachResult::Reachable { status, latency_ms }
                }
            }
            Err(e) => CloudReachResult::Unreachable {
                error: e.to_string(),
                latency_ms,
            },
        }
    }
}

/// 雲端 API 可達性偵測結果
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CloudReachResult {
    NotConfigured,
    Reachable { status: u16, latency_ms: u64 },
    Unreachable { error: String, latency_ms: u64 },
}

#[derive(Debug, Clone, Copy)]
pub enum LabelFetchMode {
    WebPrint,
    Download,
    CloudPrint,
}

fn build_http_client(timeout_secs: u64, allow_invalid_certs: bool) -> Client {
    Client::builder()
        .timeout(Duration::from_secs(timeout_secs.max(5)))
        .danger_accept_invalid_certs(allow_invalid_certs)
        .build()
        .expect("無法建立 HTTP client")
}

fn trim_base(s: &str) -> String {
    s.trim().trim_end_matches('/').to_string()
}

/// 從雲端錯誤 body 解析 `shipping_provider`(失敗回應的 failResp 欄位,查得到訂單才有)。
/// 數字型代碼也容忍(雲端常數可能是 int),統一轉成字串。
fn parse_error_shipping_provider(json: Option<&serde_json::Value>) -> Option<String> {
    let v = json?.get("shipping_provider")?;
    let s = match v {
        serde_json::Value::String(s) => s.trim().to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => return None,
    };
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// 將 `base`（已 trim 過尾 `/`）與 `path`（可能含或不含開頭 `/`）拼成完整 URL
fn join_url(base: &str, path: &str) -> String {
    let p = path.trim();
    if p.is_empty() {
        base.to_string()
    } else if let Some(rest) = p.strip_prefix('/') {
        format!("{base}/{rest}")
    } else {
        format!("{base}/{p}")
    }
}

fn load_token_from_keyring() -> AppResult<String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)?;
    match entry.get_password() {
        Ok(t) => Ok(t),
        Err(keyring::Error::NoEntry) => Ok(String::new()),
        Err(e) => Err(AppError::Keyring(e)),
    }
}

fn save_token_to_keyring(token: &str) -> AppResult<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)?;
    entry.set_password(token)?;
    Ok(())
}

fn clear_token_from_keyring() -> AppResult<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AppError::Keyring(e)),
    }
}
