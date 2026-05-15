use serde::{Deserialize, Serialize};
use tauri::State;

use crate::cache::derive_label_key;
use crate::cloud::LabelFetchMode;
use crate::models::{CloudPrintResult, CloudSession, ExaminePackageResult, PrintViewResult};
use crate::{AppResult, SharedState};

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
        .fetch_label_for_print(&req.order_sn, &req.print_type, req.enforce, mode)
        .await?;

    // Download / WebPrint 模式：把面單同步下載到本地快取，回給前端本地 server 路徑
    // → 後續列印直接讀 cache,不重打雲端
    // CloudPrint 模式由雲端直接送印,本地不需要 cache
    if matches!(mode, LabelFetchMode::Download | LabelFetchMode::WebPrint) {
        if let Some(url) = result.print_file_path.clone() {
            // 保留雲端 URL 的子資料夾結構 (labels/{provider}/{date}/{hash}.png)
            let label_key = derive_label_key(&url);
            match state.cache.fetch_now(&label_key, &url).await {
                Ok(()) => {
                    let port = state.config.read().await.server.port;
                    result.print_file_path =
                        Some(format!("http://127.0.0.1:{port}/images/{label_key}"));
                }
                Err(e) => {
                    tracing::warn!(label_key = %label_key, ?e, "面單下載到 cache 失敗，回原雲端 URL");
                    // result.print_file_path 維持原雲端 URL，前端仍可顯示
                }
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

/// 自動印單專用：cloud-print 端點回應 schema 與 PrintViewResult 不同,需獨立 command
#[tauri::command]
pub async fn cloud_fetch_cloud_print(
    state: State<'_, SharedState>,
    req: FetchCloudPrintRequest,
) -> AppResult<CloudPrintResult> {
    state
        .cloud
        .fetch_cloud_print_label(
            &req.order_sn,
            &req.print_type,
            req.enforce,
            req.package_sn.as_deref(),
            req.scanner_user.as_deref(),
            req.sticker_user.as_deref(),
        )
        .await
}
