use serde::{Deserialize, Serialize};
use tauri::State;

use crate::cloud::LabelFetchMode;
use crate::models::{CloudSession, PrintViewResult};
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
    state
        .cloud
        .fetch_label_for_print(&req.order_sn, &req.print_type, req.enforce, mode)
        .await
}
