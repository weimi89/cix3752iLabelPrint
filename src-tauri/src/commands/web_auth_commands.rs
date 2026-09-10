use serde::{Deserialize, Serialize};
use tauri::State;

use crate::server::auth;
use crate::{AppResult, SharedState};

#[derive(Debug, Serialize)]
pub struct WebAuthStatus {
    /// 是否已設定共用密碼。未設定時外網一律進不來,即使對外開關已打開。
    pub password_set: bool,
}

/// 查詢網頁存取密碼是否已設定(不回傳密碼本身,也不回雜湊)
#[tauri::command]
pub async fn web_auth_status(state: State<'_, SharedState>) -> AppResult<WebAuthStatus> {
    Ok(WebAuthStatus {
        password_set: auth::stored_password_hash(&state.db).await?.is_some(),
    })
}

#[derive(Debug, Deserialize)]
pub struct SetPasswordReq {
    /// 新密碼;傳空字串等於清除密碼(清除後外網無法登入)
    pub password: String,
}

/// 設定或更換網頁存取密碼。
///
/// 換密碼會一併踢掉所有已登入的連線 —— 否則舊密碼流出後,
/// 對方手上的 session 仍然可以繼續用到過期為止。
#[tauri::command]
pub async fn web_auth_set_password(
    state: State<'_, SharedState>,
    req: SetPasswordReq,
) -> AppResult<()> {
    let pw = req.password.trim().to_string();

    // 對外只有這一組密碼,短密碼等於沒有
    if !pw.is_empty() && pw.chars().count() < 8 {
        return Err(crate::AppError::Config(
            "網頁存取密碼至少需要 8 個字元".into(),
        ));
    }

    auth::set_password(&state.db, &pw).await
}
