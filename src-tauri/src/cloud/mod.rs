use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use reqwest::Client;
use serde_json::json;

use crate::config::AppConfig;
use crate::models::{CloudSession, ParcelInfo, PrintViewResult, ReportPayload};
use crate::{AppError, AppResult};

const KEYRING_SERVICE: &str = "com.weimi.cix3752i.labelprint";
const KEYRING_USER: &str = "cloud_api_token";

/// 雲端 API client — 封裝 Bearer Token 認證、retry、timeout
#[derive(Clone)]
pub struct CloudClient {
    inner: Arc<Inner>,
}

struct Inner {
    http: Client,
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
}

impl CloudClient {
    pub fn new(config: &AppConfig) -> Self {
        let http = build_http_client(config.cloud.timeout_secs, config.cloud.allow_invalid_certs);

        // 啟動時嘗試從 keyring 還原 token；失敗就視為未登入
        let token = load_token_from_keyring().unwrap_or_default();

        let state = CloudState {
            api_base: trim_base(&config.cloud.api_base),
            token,
            timeout: config.cloud.timeout_secs,
            retry: config.cloud.retry,
            logged_in: false,
            user_label: None,
        };

        Self {
            inner: Arc::new(Inner {
                http,
                state: RwLock::new(state),
            }),
        }
    }

    /// 套用新的設定（API base / timeout / SSL 旗標等）
    pub fn apply_config(&self, config: &AppConfig) {
        let mut s = self.inner.state.write();
        s.api_base = trim_base(&config.cloud.api_base);
        s.timeout = config.cloud.timeout_secs;
        s.retry = config.cloud.retry;
        // HTTP client 不重建 — timeout 由 request 層級控制更靈活
    }

    /// 登入：驗證 API base + token，成功則持久化 token 到 keyring
    pub async fn login(&self, api_base: &str, token: &str) -> AppResult<CloudSession> {
        let base = trim_base(api_base);
        let url = format!("{}/api/v1/local-middleware/session", base);
        let timeout_secs = { self.inner.state.read().timeout };

        let resp = self
            .inner
            .http
            .get(&url)
            .bearer_auth(token)
            .timeout(Duration::from_secs(timeout_secs))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Cloud {
                code: status.as_u16().to_string(),
                message: format!("登入失敗：{body}"),
            });
        }

        let body: serde_json::Value = resp.json().await?;
        let user_label = body
            .get("user")
            .and_then(|u| u.get("name"))
            .and_then(|n| n.as_str())
            .map(|s| s.to_string());

        save_token_to_keyring(token)?;

        {
            let mut s = self.inner.state.write();
            s.api_base = base.clone();
            s.token = token.to_string();
            s.logged_in = true;
            s.user_label = user_label.clone();
        }

        Ok(CloudSession {
            api_base: base,
            logged_in: true,
            user_label,
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

    /// 依 tracking_no 查包裹（給 HTTP server / 內部 worker 用）
    pub async fn fetch_parcel(&self, tracking_no: &str) -> AppResult<ParcelInfo> {
        let (base, token) = self.snapshot()?;
        let url = format!("{}/api/v1/local-middleware/parcel/{}", base, tracking_no);

        let resp = self
            .inner
            .http
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await?
            .error_for_status()?;

        let info: ParcelInfo = resp.json().await?;
        Ok(info)
    }

    /// 推送單筆回報結果
    pub async fn push_report(&self, payload: &ReportPayload) -> AppResult<()> {
        let (base, token) = self.snapshot()?;
        let url = format!("{}/api/v1/local-middleware/report", base);

        self.inner
            .http
            .post(&url)
            .bearer_auth(&token)
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
        print_type: &str,
        enforce: bool,
        mode: LabelFetchMode,
    ) -> AppResult<PrintViewResult> {
        let (base, token) = self.snapshot()?;

        let endpoint = match mode {
            LabelFetchMode::WebPrint => "label/scan-print",
            LabelFetchMode::Download => "label/pre-generate",
            LabelFetchMode::CloudPrint => "label/cloud-print",
        };
        let url = format!("{}/api/v1/local-middleware/{endpoint}", base);

        let body = json!({
            "order_sn": order_sn,
            "print_type": print_type,
            "enforce": enforce,
        });

        let resp = self
            .inner
            .http
            .post(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;

        let result: PrintViewResult = resp.json().await?;
        Ok(result)
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
