use tauri::{AppHandle, State};

use crate::config::AppConfig;
use crate::{AppResult, SharedState};

/// 取得目前設定（不含敏感 token）
#[tauri::command]
pub async fn get_config(state: State<'_, SharedState>) -> AppResult<AppConfig> {
    Ok(state.config.read().await.clone())
}

/// 更新設定並持久化
#[tauri::command]
pub async fn update_config(
    handle: AppHandle,
    state: State<'_, SharedState>,
    new_config: AppConfig,
) -> AppResult<AppConfig> {
    // server.listen_ip / port 不是熱套用欄位(要重綁 socket)。先比對是否變更,
    // 變更則用新設定重啟 server —— start 會驗證新 addr 可綁,失敗就整個 update 中止、
    // 不持久化也不動其他設定,避免「設定存了卻沒生效」的斷鏈(舊版這裡完全沒處理 server)。
    let server_changed = {
        let cur = state.config.read().await;
        cur.server.listen_ip != new_config.server.listen_ip
            || cur.server.port != new_config.server.port
    };
    if server_changed {
        crate::commands::server_commands::restart_server(state.inner(), &new_config, handle.clone())
            .await?;
    }

    new_config.save(&handle).await?;

    state.cloud.apply_config(&new_config);
    state.cache.apply_config(&handle, &new_config)?;
    state.health.apply_config(&new_config);
    state.label_resolver.apply_config(&new_config);

    *state.config.write().await = new_config.clone();
    Ok(new_config)
}

/// 取得面單預產自動排程的可觀測狀態(排程啟動時間、上次執行結果)。
/// 前端據此顯示「上次執行 / 排程啟動」常駐狀態,確認排程是否正常運作。
#[tauri::command]
pub async fn get_pregen_status(
    state: State<'_, SharedState>,
) -> AppResult<crate::pregen::PregenStatus> {
    Ok(state.pregen_status.read().await.clone())
}
