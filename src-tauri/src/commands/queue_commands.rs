use tauri::State;

use crate::queue::QueueStats;
use crate::{AppResult, SharedState};

#[tauri::command]
pub async fn queue_stats(state: State<'_, SharedState>) -> AppResult<QueueStats> {
    state.queue.stats().await
}
