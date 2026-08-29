use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use reqwest::Client;
use serde_json::json;

use crate::config::AppConfig;
use crate::models::{
    ClearanceDispatchResult, ClearanceOptions, ClearanceStoreResult, CloudOrderResponse,
    CloudPrintResult, CloudSession, ExaminePackageResult, PackageOrdersResult, ParcelInfo,
    PrintViewResult,
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
    clearance_options_path: String,
    clearance_progress_path: String,
    clearance_store_path: String,
    clearance_dispatch_path: String,
    field_operation_monitor_path: String,
    warehouse_scanner_path: String,
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
            clearance_options_path: config.cloud.clearance_options_path.clone(),
            clearance_progress_path: config.cloud.clearance_progress_path.clone(),
            clearance_store_path: config.cloud.clearance_store_path.clone(),
            clearance_dispatch_path: config.cloud.clearance_dispatch_path.clone(),
            field_operation_monitor_path: config.cloud.field_operation_monitor_path.clone(),
            warehouse_scanner_path: config.cloud.warehouse_scanner_path.clone(),
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
        s.clearance_options_path = config.cloud.clearance_options_path.clone();
        s.clearance_progress_path = config.cloud.clearance_progress_path.clone();
        s.clearance_store_path = config.cloud.clearance_store_path.clone();
        s.clearance_dispatch_path = config.cloud.clearance_dispatch_path.clone();
        s.field_operation_monitor_path = config.cloud.field_operation_monitor_path.clone();
        s.warehouse_scanner_path = config.cloud.warehouse_scanner_path.clone();
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
    ///
    /// `sort_only`：純分揀模式。帶 `?sort_only=1` 告知雲端「本支請求不記任何印單」
    /// (不寫 order_print_log、不更新 shipping_print_num/_time、不廣播 ParcelPrinted),
    /// 成功回應因而不含 response_id。雲端仍照常回物流商 / shipping_no / package_sn 供分揀。
    pub async fn fetch_parcel(&self, query_no: &str, sort_only: bool) -> AppResult<ParcelInfo> {
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
        // 工控機同步熱路徑:只重試「連線層」瞬時錯誤(連線被拒/重置,near-instant),
        // 不重試 timeout(避免雲端 hang 時把工控機等待放大 retry 倍)。retry 取自設定(預設 3)。
        let retry = self.inner.state.read().retry;
        let mut attempt = 0u32;
        let resp = loop {
            // sort_only 用 reqwest .query() 附加(自動 URL 編碼),不手動拼字串,
            // 避免含 `?`/`#` 的條碼把參數推到錯位置導致雲端讀不到 sort_only。
            let mut req = http.get(&url);
            if sort_only {
                req = req.query(&[("sort_only", "1")]);
            }
            match req.send().await {
                Ok(r) => break r,
                Err(e) if e.is_connect() && attempt < retry => {
                    attempt += 1;
                    tracing::warn!(%url, attempt, ?e, "雲端 parcel 查詢連線失敗,重試");
                    continue;
                }
                Err(e) => return Err(e.into()),
            }
        };
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
            let shipping_no = parse_error_shipping_no(json.as_ref());
            let package_sn = parse_error_package_sn(json.as_ref());
            let order_sn = parse_error_order_sn(json.as_ref());
            return Err(AppError::Cloud { code, message, shipping_provider, shipping_no, package_sn, order_sn });
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
            let shipping_no = parse_error_shipping_no(json.as_ref());
            let package_sn = parse_error_package_sn(json.as_ref());
            let order_sn = parse_error_order_sn(json.as_ref());
            return Err(AppError::Cloud { code, message, shipping_provider, shipping_no, package_sn, order_sn });
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

    /// 帶 Bearer 的 GET,回傳已 error_for_status 的回應內文。HTTP 樣板集中一處
    /// (snapshot → join_url → http clone → get+query → bearer → error_for_status → text)。
    async fn authed_get(&self, path: &str, query: &[(&str, &str)]) -> AppResult<String> {
        let (base, token) = self.snapshot()?;
        let url = join_url(&base, path);
        let http = self.inner.http.read().clone();
        let resp = http
            .get(&url)
            .query(query)
            .bearer_auth(&token)
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.text().await?)
    }

    /// 帶 Bearer 的 POST(JSON body),回傳已 error_for_status 的回應內文。
    async fn authed_post(&self, path: &str, body: serde_json::Value) -> AppResult<String> {
        let (base, token) = self.snapshot()?;
        let url = join_url(&base, path);
        let http = self.inner.http.read().clone();
        let resp = http
            .post(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.text().await?)
    }

    /// 面單預產用:袋號反查整袋訂單編號(對齊雲端 label/package-orders)
    pub async fn fetch_package_orders(&self, package_sn: &str) -> AppResult<PackageOrdersResult> {
        let path = self.inner.state.read().package_orders_path.clone();
        let text = self
            .authed_post(&path, json!({ "package_sn": package_sn }))
            .await?;
        parse_cloud_json(&text, "package-orders")
    }

    /// 面單預產用:依日期反查整批訂單編號(對齊雲端 label/orders-by-date;source: clearance/transfer)
    pub async fn fetch_orders_by_date(
        &self,
        date: &str,
        source: &str,
    ) -> AppResult<PackageOrdersResult> {
        let path = self.inner.state.read().orders_by_date_path.clone();
        let text = self
            .authed_post(&path, json!({ "date": date, "source": source }))
            .await?;
        parse_cloud_json(&text, "orders-by-date")
    }

    /// 清關作業:取得選項(倉庫 / 清關公司 / 司機 歷史清單),對齊雲端 clearance/options
    pub async fn fetch_clearance_options(&self) -> AppResult<ClearanceOptions> {
        let path = self.inner.state.read().clearance_options_path.clone();
        let text = self.authed_get(&path, &[]).await?;
        parse_cloud_json(&text, "clearance/options")
    }

    /// 清關進度浮動框:GET clearance/progress?from&to,回傳雲端原始 JSON
    /// (`{from,to,bag_total,parcel_total,printed,remaining,parcels:[...]}`),直接透傳給前端。
    pub async fn fetch_clearance_progress(
        &self,
        from: &str,
        to: &str,
    ) -> AppResult<serde_json::Value> {
        let path = self.inner.state.read().clearance_progress_path.clone();
        let text = self
            .authed_get(&path, &[("from", from), ("to", to)])
            .await?;
        parse_cloud_json(&text, "clearance/progress")
    }

    /// 現場作業監控:清關/轉寄進度看板 + 每日貼單作業人員統計(透傳雲端 JSON,與網頁版同一份資料)
    pub async fn fetch_field_operation_monitor(
        &self,
        from: &str,
        to: &str,
    ) -> AppResult<serde_json::Value> {
        let path = self.inner.state.read().field_operation_monitor_path.clone();
        let text = self
            .authed_get(&path, &[("from", from), ("to", to)])
            .await?;
        parse_cloud_json(&text, "field-operation-monitor")
    }

    /// 清關作業:新增清關包裹,對齊雲端 clearance/store
    /// `package_sn` 以逗號 / 空白 / 換行分隔的多筆袋號字串(雲端會再去重、轉大寫)
    pub async fn store_clearance_packages(
        &self,
        transport_package_sn: &str,
        clearance_company: &str,
        clearance_date: &str,
        storage_code: &str,
    ) -> AppResult<ClearanceStoreResult> {
        let path = self.inner.state.read().clearance_store_path.clone();
        let text = self
            .authed_post(
                &path,
                json!({
                    "transport_package_sn": transport_package_sn,
                    "clearance_company": clearance_company,
                    "clearance_date": clearance_date,
                    "storage_code": storage_code,
                }),
            )
            .await?;
        parse_cloud_json(&text, "clearance/store")
    }

    /// 清關作業:司機派工,對齊雲端 clearance/dispatch
    pub async fn dispatch_clearance_packages(
        &self,
        transport_package_sn: &str,
        driver_name: &str,
        shipping_date: &str,
        storage_code: &str,
    ) -> AppResult<ClearanceDispatchResult> {
        let path = self.inner.state.read().clearance_dispatch_path.clone();
        let text = self
            .authed_post(
                &path,
                json!({
                    "transport_package_sn": transport_package_sn,
                    "driver_name": driver_name,
                    "shipping_date": shipping_date,
                    "storage_code": storage_code,
                }),
            )
            .await?;
        parse_cloud_json(&text, "clearance/dispatch")
    }

    // ===== 入倉驗單(warehouse-scanner)=====
    // 邏輯全在雲端 WarehouseScannerService,中介端僅透傳 JSON;base path 下接子路由。

    /// warehouse-scanner 子路由 GET(透傳雲端 JSON)
    async fn warehouse_get(
        &self,
        suffix: &str,
        query: &[(&str, &str)],
    ) -> AppResult<serde_json::Value> {
        let path = {
            let p = self.inner.state.read().warehouse_scanner_path.clone();
            format!("{p}{suffix}")
        };
        let text = self.authed_get(&path, query).await?;
        parse_cloud_json(&text, &format!("warehouse-scanner{suffix}"))
    }

    /// warehouse-scanner 子路由 POST(透傳雲端 JSON)
    async fn warehouse_post(
        &self,
        suffix: &str,
        body: serde_json::Value,
    ) -> AppResult<serde_json::Value> {
        let path = {
            let p = self.inner.state.read().warehouse_scanner_path.clone();
            format!("{p}{suffix}")
        };
        let text = self.authed_post(&path, body).await?;
        parse_cloud_json(&text, &format!("warehouse-scanner{suffix}"))
    }

    /// 入倉驗單:下拉選項(倉庫 / 物流商)
    pub async fn warehouse_options(&self) -> AppResult<serde_json::Value> {
        self.warehouse_get("/options", &[]).await
    }

    /// 入倉驗單:建立 / 載入箱號
    pub async fn warehouse_create_package(&self, body: serde_json::Value) -> AppResult<serde_json::Value> {
        self.warehouse_post("/create-package", body).await
    }

    /// 入倉驗單:驗單入倉
    pub async fn warehouse_examine(&self, body: serde_json::Value) -> AppResult<serde_json::Value> {
        self.warehouse_post("/examine", body).await
    }

    /// 入倉驗單:移除單一商品
    pub async fn warehouse_remove_goods(&self, shipment_no: &str) -> AppResult<serde_json::Value> {
        self.warehouse_post("/remove-goods", json!({ "shipment_no": shipment_no })).await
    }

    /// 入倉驗單:刪除整個箱號
    pub async fn warehouse_remove_package(&self, storage_warehouse: &str, package_sn: &str) -> AppResult<serde_json::Value> {
        self.warehouse_post(
            "/remove-package",
            json!({ "storage_warehouse": storage_warehouse, "package_sn": package_sn }),
        )
        .await
    }

    /// 入倉驗單:取箱標列印資料(中介端拿到後走本地印表機列印)
    pub async fn warehouse_label_data(
        &self,
        storage_warehouse: &str,
        package_sn: &str,
        end_num: u32,
        continuous: bool,
    ) -> AppResult<serde_json::Value> {
        let end_num = end_num.to_string();
        let continuous = if continuous { "1" } else { "0" };
        self.warehouse_get(
            "/label-data",
            &[
                ("storage_warehouse", storage_warehouse),
                ("package_sn", package_sn),
                ("end_num", &end_num),
                ("continuous", continuous),
            ],
        )
        .await
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

        // 對齊雲端 controller：print_type 是 array,enforce 用 0/1 numeric
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
            .await?;

        // 非 2xx:解析 dingo errorFormat 錯誤 body 成 AppError::Cloud(對齊 fetch_label_for_print)。
        // 不可用 error_for_status()(那會變 AppError::Http):cloud_commands 依賴 AppError::Cloud
        // 的機器碼合成失敗結果(記 print_failure_event + 產錯誤面單),Http 錯誤只會直接回 Err
        // —— 否則自動印單的 4xx 業務錯誤永遠走不進失敗流程(該 match arm 形同死碼)。
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
            let shipping_no = parse_error_shipping_no(json.as_ref());
            let package_sn = parse_error_package_sn(json.as_ref());
            let order_sn = parse_error_order_sn(json.as_ref());
            return Err(AppError::Cloud { code, message, shipping_provider, shipping_no, package_sn, order_sn });
        }

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
    parse_error_str_field(json, "shipping_provider")
}

/// 從雲端錯誤 body 解析 `shipping_no`(failResp 在 400 帶出,查得到訂單才有)。
fn parse_error_shipping_no(json: Option<&serde_json::Value>) -> Option<String> {
    parse_error_str_field(json, "shipping_no")
}

/// 從雲端錯誤 body 解析 `package_sn`(有跑 recordNotOutboundPrint 的業務錯誤才有,供件數核對標記已印)。
fn parse_error_package_sn(json: Option<&serde_json::Value>) -> Option<String> {
    parse_error_str_field(json, "package_sn")
}

/// 從雲端錯誤 body 解析 `order_sn`(同 package_sn)。
fn parse_error_order_sn(json: Option<&serde_json::Value>) -> Option<String> {
    parse_error_str_field(json, "order_sn")
}

/// 從錯誤 body 取指定欄位並轉成非空字串;數字型也容忍(雲端常數可能是 int)。
fn parse_error_str_field(json: Option<&serde_json::Value>, key: &str) -> Option<String> {
    let v = json?.get(key)?;
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

/// 統一把雲端回應內文 serde 成目標型別;失敗時帶上 endpoint 標籤與截斷 body 方便診斷。
fn parse_cloud_json<T: serde::de::DeserializeOwned>(text: &str, label: &str) -> AppResult<T> {
    serde_json::from_str(text).map_err(|e| {
        AppError::Server(format!(
            "雲端 {label} 回應解析失敗: {e}; body: {}",
            text.chars().take(500).collect::<String>()
        ))
    })
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
