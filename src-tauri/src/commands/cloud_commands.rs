use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::cache::derive_label_key;
use crate::event_log;
use crate::cloud::LabelFetchMode;
use crate::commands::print_stats_commands::emit_print_stats_updated;
use crate::models::{
    ClearanceDispatchResult, ClearanceOptions, ClearanceStoreResult, CloudPrintResult,
    CloudSession, ExaminePackageResult, PackageOrdersResult, PrintViewResult,
};
use crate::watermark::derive_repeat_key;
use crate::{AppError, AppResult, SharedState};

/// 把掃描列印 print_view_status / 自動印單 respond_code 對應到錯誤面單代碼
fn to_error_label_code(code: &str) -> &'static str {
    match code {
        "STORE_CLOSED" | "STORE-CLOSED"                      => "STORE_CLOSED",
        "UNCONFIRMED" | "UNCONFIRMED-SHIPMENT"               => "UNCONFIRMED",
        "NOT_FOUND" | "NO-DATA"                              => "NOT_FOUND",
        "NOT_PROXY"                                          => "NOT_PROXY",
        "NOT_FORWARD"                                        => "NOT_FORWARD",
        "ABNORMAL-SHIPMENT" | "ORDER-UNUSUAL" | "SHIPPING-UNUSUAL" => "ABNORMAL",
        _                                                    => "ERROR",
    }
}

/// 把雲端面單 URL 處理成前端 UI 可用的 middleware 本機 URL:
///   1. 下載原圖到 cache(若未命中)
///   2. 若 `print_num > 1` 且字型可用,生成 `@repeat/W{provider}-{basename}.png`
///   3. 回 `http://127.0.0.1:{port}/images/{effective_key}`
///
/// 任何步驟失敗都退回原雲端 URL,讓前端 fallback 顯示。
async fn process_label_for_ui(
    state: &SharedState,
    raw_url: &str,
    print_num: Option<u32>,
    provider: Option<&str>,
) -> String {
    let label_key = derive_label_key(raw_url);

    // 1. 確保原圖已在 cache
    if !state.cache.has_local(&label_key) {
        if let Err(e) = state.cache.fetch_now(&label_key, raw_url).await {
            tracing::warn!(label_key = %label_key, ?e, "面單下載到 cache 失敗,回原雲端 URL");
            return raw_url.to_string();
        }
    }

    // 2. print_num > 1 → 套用浮水印
    let effective_key = match (print_num, provider) {
        (Some(n), Some(p)) if n > 1 => {
            let repeat_key = derive_repeat_key(&label_key, p);
            let cache_base = state.cache.base_dir();
            let src = state.cache.local_path_for_key(&label_key);
            let dst = cache_base.join(&repeat_key);
            match state.watermark.apply(&src, &dst, n, p) {
                Ok(()) => repeat_key,
                Err(e) => {
                    tracing::warn!(label_key = %label_key, print_num = n, %e, "浮水印生成失敗,回原圖");
                    label_key
                }
            }
        }
        _ => label_key,
    };

    let port = state.config.read().await.server.port;
    format!("http://127.0.0.1:{port}/images/{effective_key}")
}

/// 面單預產(背景排程用):Download 模式抓單張面單並下載快取,
/// 不記印單事件、不產錯誤面單(純預熱快取)。
/// 回傳 Ok(true)=成功快取 / Ok(false)=雲端回成功但無檔路徑(略過) / Err=連線或下載失敗。
pub(crate) async fn pregen_label_to_cache(state: &SharedState, order_sn: &str) -> AppResult<bool> {
    let result = state
        .cloud
        .fetch_label_for_print(
            order_sn,
            &["ALL".to_string()],
            false,
            LabelFetchMode::Download,
            None,
            None,
        )
        .await?;
    let Some(url) = result.print_file_path.clone() else {
        return Ok(false); // 雲端回成功但無檔路徑
    };
    let label_key = derive_label_key(&url);
    if state.cache.has_local(&label_key) {
        return Ok(true); // 已在 cache,免重抓
    }
    // 直接下載到 cache 並傳播真實成敗:不經 process_label_for_ui(那是 UI 路徑,
    // 下載失敗會「退回原雲端 URL」回 String,會讓 pregen 把沒快取成功的也算成功)
    state.cache.fetch_now(&label_key, &url).await?;
    Ok(true)
}

/// 產生錯誤面單、寫入 cache,回傳本機 middleware URL(`http://127.0.0.1:{port}/images/@error/...`)。
/// 給 GUI 掃描 / 自動印單用:前端拿到後以「該物流商在印表機設定的同一台印表機」印出,
/// 與正常面單同出口 —— 避免後端 find_any_printer(dispatch_provider DB / 系統預設)與
/// 前端 printerMap(localStorage)印表機來源不一致,導致正常面單印得出、錯誤面單卻印不出或印錯台。
/// 寫檔失敗回 None(前端略過,後端不再 fallback 列印,與正常面單失敗時行為一致)。
async fn build_error_label_url(
    state: &SharedState,
    query_no: &str,
    error_code: &str,
) -> Option<String> {
    let bytes = crate::error_label::generate(
        query_no,
        error_code,
        crate::error_label::LabelHeight::H100mm,
    );
    let cache_base = state.cache.base_dir();
    let key = crate::server::write_error_label_to_cache(&cache_base, query_no, error_code, &bytes)
        .await?;
    let port = state.config.read().await.server.port;
    Some(format!("http://127.0.0.1:{port}/images/{key}"))
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub api_base: String,
    pub token: String,
}

#[derive(Debug, Deserialize)]
pub struct FetchLabelRequest {
    pub order_sn: String,
    #[serde(default = "default_print_type_vec")]
    pub print_type: Vec<String>,
    #[serde(default)]
    pub enforce: bool,
    /// "web_print" / "download" / "cloud_print"
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default)]
    pub scanner_user: Option<String>,
    #[serde(default)]
    pub sticker_user: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct FetchCloudPrintRequest {
    pub order_sn: String,
    #[serde(default = "default_print_type_vec")]
    pub print_type: Vec<String>,
    #[serde(default)]
    pub enforce: bool,
    #[serde(default)]
    pub package_sn: Option<String>,
    #[serde(default)]
    pub scanner_user: Option<String>,
    #[serde(default)]
    pub sticker_user: Option<String>,
}

fn default_print_type_vec() -> Vec<String> { vec!["ALL".to_string()] }
fn default_mode() -> String { "web_print".to_string() }

#[derive(Debug, Serialize)]
pub struct PingResp {
    pub ok: bool,
}

/// 確認雲端可達（不需登入也能 ping，例如打 /healthz）
#[tauri::command]
pub async fn cloud_ping(state: State<'_, SharedState>) -> AppResult<PingResp> {
    // 簡單做：用目前 session 拉 session 端點；若還沒登入則靠 base 解析失敗判斷
    let session = state.cloud.session();
    Ok(PingResp { ok: session.logged_in })
}

#[tauri::command]
pub async fn cloud_login(
    state: State<'_, SharedState>,
    req: LoginRequest,
) -> AppResult<CloudSession> {
    let result = state.cloud.login(&req.api_base, &req.token).await?;
    event_log::log_bg(state.db.clone(), "info", "cloud", "登入",
        format!("登入成功 {}", req.api_base));
    Ok(result)
}

#[tauri::command]
pub async fn cloud_logout(state: State<'_, SharedState>) -> AppResult<()> {
    let result = state.cloud.logout();
    if result.is_ok() {
        event_log::log_bg(state.db.clone(), "info", "cloud", "登出", "已登出".to_string());
    }
    result
}

#[tauri::command]
pub async fn cloud_session(state: State<'_, SharedState>) -> AppResult<CloudSession> {
    Ok(state.cloud.session())
}

#[tauri::command]
pub async fn cloud_fetch_label(
    state: State<'_, SharedState>,
    app: AppHandle,
    req: FetchLabelRequest,
) -> AppResult<PrintViewResult> {
    let mode = match req.mode.as_str() {
        "download" => LabelFetchMode::Download,
        "cloud_print" => LabelFetchMode::CloudPrint,
        _ => LabelFetchMode::WebPrint,
    };
    let mut result = match state
        .cloud
        .fetch_label_for_print(
            &req.order_sn,
            &req.print_type,
            req.enforce,
            mode,
            req.scanner_user.as_deref(),
            req.sticker_user.as_deref(),
        )
        .await
    {
        Ok(r) => r,
        Err(e) => {
            if let AppError::Cloud { code, shipping_provider, .. } = &e {
                crate::server::spawn_error_label_print(
                    state.db.clone(),
                    app.clone(),
                    req.order_sn.clone(),
                    to_error_label_code(code).to_string(),
                    shipping_provider.clone(),
                );
            }
            return Err(e);
        }
    };

    // Download / WebPrint 模式:下載到本地快取 + 套用列印次數浮水印,
    // 回給前端本地 server 路徑(後續列印直接讀 cache,不重打雲端)
    // CloudPrint 模式由雲端直接送印,本地不需要 cache
    if matches!(mode, LabelFetchMode::Download | LabelFetchMode::WebPrint) {
        if let Some(url) = result.print_file_path.clone() {
            let provider = result.print_shipping_provider.as_deref();
            let local_url =
                process_label_for_ui(state.inner(), &url, result.print_num, provider).await;
            result.print_file_path = Some(local_url);
        }
    }

    // 面單預產(Download 模式)只是下載快取,不是出單,不記印單事件
    if !matches!(mode, LabelFetchMode::Download) {
        // 印單事件 — LABEL-PROCESS = 成功記 print_event;其他狀態記 print_failure_event
        let shipping_no_opt = result.print_shipping_no.as_deref().filter(|s| !s.is_empty());
        if result.print_view_status == "LABEL-PROCESS" {
            if let Some(no) = shipping_no_opt {
                let insert_res = sqlx::query(
                    "INSERT INTO print_event (source, shipping_no, provider_code, sticker_user, scanner_user)
                     VALUES ('scan', ?, ?, ?, ?)",
                )
                .bind(no)
                .bind(result.print_shipping_provider.as_deref())
                .bind(req.sticker_user.as_deref())
                .bind(req.scanner_user.as_deref())
                .execute(&state.db)
                .await;
                if insert_res.is_ok() {
                    emit_print_stats_updated(&app, "scan", no);
                }
            }
        } else {
            let _ = sqlx::query(
                "INSERT INTO print_failure_event (source, respond_code, order_sn, shipping_no, scanner_user, sticker_user)
                 VALUES ('scan', ?, ?, ?, ?, ?)",
            )
            .bind(&result.print_view_status)
            .bind(&req.order_sn)
            .bind(shipping_no_opt)
            .bind(req.scanner_user.as_deref())
            .bind(req.sticker_user.as_deref())
            .execute(&state.db)
            .await;
            event_log::log_bg(state.db.clone(), "warn", "cloud", "面單查詢失敗",
                format!("面單查詢失敗 status={} order={}", result.print_view_status, req.order_sn));
            if !matches!(result.print_view_status.as_str(), "SHIPPING-REPEAT" | "NO-PRINT-TYPE") {
                // 產生錯誤面單 URL 回前端,由前端用 printerMap[provider] 印(同正常面單出口)
                result.error_label_path = build_error_label_url(
                    state.inner(),
                    &req.order_sn,
                    to_error_label_code(&result.print_view_status),
                )
                .await;
            }
        }
    }

    Ok(result)
}

#[derive(Debug, Deserialize)]
pub struct ExaminePackageRequest {
    pub shipment_no: String,
}

/// 自動印單第一步：掃包裹條碼取訂單清單
#[tauri::command]
pub async fn cloud_examine_package(
    state: State<'_, SharedState>,
    req: ExaminePackageRequest,
) -> AppResult<ExaminePackageResult> {
    state.cloud.examine_package(&req.shipment_no).await
}

#[derive(Debug, Deserialize)]
pub struct PackageOrdersRequest {
    pub package_sn: String,
}

/// 面單預產:袋號反查整袋訂單編號
#[tauri::command]
pub async fn cloud_package_orders(
    state: State<'_, SharedState>,
    req: PackageOrdersRequest,
) -> AppResult<PackageOrdersResult> {
    state.cloud.fetch_package_orders(&req.package_sn).await
}

#[derive(Debug, Deserialize)]
pub struct OrdersByDateRequest {
    pub date: String,
    #[serde(default)]
    pub source: Option<String>,
}

/// 面單預產:依日期反查整批訂單編號(source: clearance 清關 / transfer 轉寄出貨)
#[tauri::command]
pub async fn cloud_orders_by_date(
    state: State<'_, SharedState>,
    req: OrdersByDateRequest,
) -> AppResult<PackageOrdersResult> {
    let source = req.source.as_deref().unwrap_or("clearance");
    state.cloud.fetch_orders_by_date(&req.date, source).await
}

/// 清關作業:取得選項(倉庫 / 清關公司 / 司機 歷史清單)
#[tauri::command]
pub async fn cloud_clearance_options(
    state: State<'_, SharedState>,
) -> AppResult<ClearanceOptions> {
    state.cloud.fetch_clearance_options().await
}

#[derive(Debug, Deserialize)]
pub struct ClearanceStoreRequest {
    pub transport_package_sn: String,
    #[serde(default)]
    pub clearance_company: Option<String>,
    #[serde(default)]
    pub clearance_date: Option<String>,
    #[serde(default)]
    pub storage_code: Option<String>,
}

/// 清關作業:新增清關包裹
#[tauri::command]
pub async fn cloud_clearance_store(
    state: State<'_, SharedState>,
    req: ClearanceStoreRequest,
) -> AppResult<ClearanceStoreResult> {
    state
        .cloud
        .store_clearance_packages(
            &req.transport_package_sn,
            req.clearance_company.as_deref().unwrap_or(""),
            req.clearance_date.as_deref().unwrap_or(""),
            req.storage_code.as_deref().unwrap_or("33843"),
        )
        .await
}

#[derive(Debug, Deserialize)]
pub struct ClearanceDispatchRequest {
    pub transport_package_sn: String,
    pub driver_name: String,
    #[serde(default)]
    pub shipping_date: Option<String>,
    #[serde(default)]
    pub storage_code: Option<String>,
}

/// 清關作業:司機派工
#[tauri::command]
pub async fn cloud_clearance_dispatch(
    state: State<'_, SharedState>,
    req: ClearanceDispatchRequest,
) -> AppResult<ClearanceDispatchResult> {
    state
        .cloud
        .dispatch_clearance_packages(
            &req.transport_package_sn,
            &req.driver_name,
            req.shipping_date.as_deref().unwrap_or(""),
            req.storage_code.as_deref().unwrap_or("33843"),
        )
        .await
}

/// 自動印單專用:cloud-print 端點回應 schema 與 PrintViewResult 不同,需獨立 command
#[tauri::command]
pub async fn cloud_fetch_cloud_print(
    state: State<'_, SharedState>,
    app: AppHandle,
    req: FetchCloudPrintRequest,
) -> AppResult<CloudPrintResult> {
    let mut result = state
        .cloud
        .fetch_cloud_print_label(
            &req.order_sn,
            &req.print_type,
            req.enforce,
            req.package_sn.as_deref(),
            req.scanner_user.as_deref(),
            req.sticker_user.as_deref(),
        )
        .await?;

    // PRINT-SUCCESS 才有 image_path / print_num,套用浮水印並改寫成 middleware URL
    if let Some(url) = result.image_path.clone() {
        let provider = result.provider_code.as_deref();
        let local_url =
            process_label_for_ui(state.inner(), &url, result.print_num, provider).await;
        result.image_path = Some(local_url);
    }

    // 印單事件:source='auto'(自動印單).僅 PRINT-SUCCESS 計入 print_event;
    // 其他 respond_code(NO-DATA / PRINT-ERROR / UNCONFIRMED-SHIPMENT / ERROR-SHIPMENT /
    // ABNORMAL-* / WRAPPER-ERROR / STORE-CLOSED)記 print_failure_event
    // package_sn 寫入:雲端回應為主、前端傳入為輔(雲端值 None / 空字串時取 req)
    // scan / ipc 寫入點不帶此欄位,保持 NULL — 那兩個來源本來就沒有「袋」概念
    if result.respond_code == "PRINT-SUCCESS" {
        if let Some(no) = result.shipment_no.as_deref() {
            if !no.is_empty() {
                let package_sn_for_db: Option<&str> = result
                    .package_sn
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .or_else(|| req.package_sn.as_deref().filter(|s| !s.is_empty()));
                let insert_res = sqlx::query(
                    "INSERT INTO print_event (source, shipping_no, provider_code, sticker_user, scanner_user, package_sn)
                     VALUES ('auto', ?, ?, ?, ?, ?)",
                )
                .bind(no)
                .bind(result.provider_code.as_deref())
                .bind(req.sticker_user.as_deref())
                .bind(req.scanner_user.as_deref())
                .bind(package_sn_for_db)
                .execute(&state.db)
                .await;
                if insert_res.is_ok() {
                    emit_print_stats_updated(&app, "auto", no);
                }
            }
        }
    } else {
        let _ = sqlx::query(
            "INSERT INTO print_failure_event (source, respond_code, order_sn, shipping_no, respond_message, scanner_user, sticker_user)
             VALUES ('auto', ?, ?, ?, ?, ?, ?)",
        )
        .bind(&result.respond_code)
        .bind(&req.order_sn)
        .bind(result.shipment_no.as_deref())
        .bind(result.respond_message.as_deref())
        .bind(req.scanner_user.as_deref())
        .bind(req.sticker_user.as_deref())
        .execute(&state.db)
        .await;
        if !matches!(result.respond_code.as_str(), "ABNORMAL-PACKAGE" | "WRAPPER-ERROR") {
            // 產生錯誤面單 URL 回前端,由前端用 printerMap[provider_code] 印(同正常面單出口)
            result.error_label_path = build_error_label_url(
                state.inner(),
                &req.order_sn,
                to_error_label_code(&result.respond_code),
            )
            .await;
        }
    }

    Ok(result)
}
