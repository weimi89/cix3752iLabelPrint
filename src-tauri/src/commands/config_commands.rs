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
    new_config.save(&handle).await?;

    state.cloud.apply_config(&new_config);
    state.cache.apply_config(&handle, &new_config)?;
    state.health.apply_config(&new_config);
    state.label_resolver.apply_config(&new_config);

    *state.config.write().await = new_config.clone();
    Ok(new_config)
}
