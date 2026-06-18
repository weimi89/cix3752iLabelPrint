use tauri::State;

use crate::{AppResult, SharedState};

/// 桌面回看用:最近雲端查件異常(門市關轉 / 未確認 / 找不到 …)
#[tauri::command]
pub async fn recent_parcel_alerts(
    state: State<'_, SharedState>,
) -> AppResult<Vec<serde_json::Value>> {
    Ok(crate::server::fetch_recent_alerts(&state.db, 100).await)
}
