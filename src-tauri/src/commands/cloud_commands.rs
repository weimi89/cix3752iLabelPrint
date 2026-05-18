use serde::{Deserialize, Serialize};
use tauri::State;

use crate::cache::derive_label_key;
use crate::cloud::LabelFetchMode;
use crate::models::{CloudPrintResult, CloudSession, ExaminePackageResult, PrintViewResult};
use crate::watermark::derive_repeat_key;
use crate::{AppResult, SharedState};

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

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub api_base: String,
    pub token: String,
}

#[derive(Debug, Deserialize)]
pub struct FetchLabelRequest {
    pub order_sn: String,
    #[serde(default = "default_print_type")]
    pub print_type: String,
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
    #[serde(default = "default_print_type")]
    pub print_type: String,
    #[serde(default)]
    pub enforce: bool,
    #[serde(default)]
    pub package_sn: Option<String>,
    #[serde(default)]
    pub scanner_user: Option<String>,
    #[serde(default)]
    pub sticker_user: Option<String>,
}

fn default_print_type() -> String { "ALL".to_string() }
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
    state.cloud.login(&req.api_base, &req.token).await
}

#[tauri::command]
pub async fn cloud_logout(state: State<'_, SharedState>) -> AppResult<()> {
    state.cloud.logout()
}

#[tauri::command]
pub async fn cloud_session(state: State<'_, SharedState>) -> AppResult<CloudSession> {
    Ok(state.cloud.session())
}

#[tauri::command]
pub async fn cloud_fetch_label(
    state: State<'_, SharedState>,
    req: FetchLabelRequest,
) -> AppResult<PrintViewResult> {
    let mode = match req.mode.as_str() {
        "download" => LabelFetchMode::Download,
        "cloud_print" => LabelFetchMode::CloudPrint,
        _ => LabelFetchMode::WebPrint,
    };
    let mut result = state
        .cloud
        .fetch_label_for_print(
            &req.order_sn,
            &req.print_type,
            req.enforce,
            mode,
            req.scanner_user.as_deref(),
            req.sticker_user.as_deref(),
        )
        .await?;

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

/// 自動印單專用:cloud-print 端點回應 schema 與 PrintViewResult 不同,需獨立 command
#[tauri::command]
pub async fn cloud_fetch_cloud_print(
    state: State<'_, SharedState>,
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

    Ok(result)
}
