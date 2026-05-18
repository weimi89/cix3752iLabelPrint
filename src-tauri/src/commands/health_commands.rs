use tauri::State;

use crate::health::NetHealthSnapshot;
use crate::{AppResult, SharedState};

/// 讀目前快照(不觸發新檢查),前端啟動時呼叫一次取初始值
#[tauri::command]
pub async fn network_health_get(state: State<'_, SharedState>) -> AppResult<NetHealthSnapshot> {
    Ok(state.health.snapshot())
}

/// 立即觸發一輪檢查並回傳結果(同時會 emit 'network-status' event)
#[tauri::command]
pub async fn network_health_check(state: State<'_, SharedState>) -> AppResult<NetHealthSnapshot> {
    Ok(state.health.check_now().await)
}
