use serde::Serialize;
use tauri::State;

use crate::server;
use crate::{AppResult, SharedState};

#[derive(Debug, Serialize)]
pub struct ServerStatus {
    pub running: bool,
    pub bind_addr: String,
}

#[tauri::command]
pub async fn server_status(state: State<'_, SharedState>) -> AppResult<ServerStatus> {
    let guard = state.server.read().await;
    Ok(ServerStatus {
        running: true,
        bind_addr: guard.bind_addr.clone(),
    })
}

/// 重啟 server（套用新設定後呼叫）
#[tauri::command]
pub async fn server_restart(state: State<'_, SharedState>) -> AppResult<ServerStatus> {
    let config = state.config.read().await.clone();

    // 先取出舊 handle 並關閉
    let new_handle = {
        let mut guard = state.server.write().await;
        // 直接用 take pattern：先換一個 placeholder 也行；簡化做法是先 shutdown 再 start
        // 因為 ServerHandle 沒實作 Default，我們用 swap 不便；改成「先 start 新的、再關舊的」
        let new = server::start(
            &config,
            state.db.clone(),
            state.cloud.clone(),
            state.cache.clone(),
            state.queue.clone(),
            state.label_resolver.clone(),
            state.watermark.clone(),
        )
        .await?;
        let old = std::mem::replace(&mut *guard, new);
        old
    };

    new_handle.shutdown().await;

    let guard = state.server.read().await;
    Ok(ServerStatus {
        running: true,
        bind_addr: guard.bind_addr.clone(),
    })
}
