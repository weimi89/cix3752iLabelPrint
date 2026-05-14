use serde::Serialize;
use sqlx::Row;
use tauri::State;

use crate::{AppResult, SharedState};

#[derive(Debug, Clone, Serialize, Default)]
pub struct CacheStats {
    pub file_count: i64,
    pub total_bytes: i64,
    pub hit_count: i64,
    pub miss_count: i64,
    pub hit_rate: f64,
}

#[tauri::command]
pub async fn cache_stats(state: State<'_, SharedState>) -> AppResult<CacheStats> {
    // 從 cache_meta 取數量與總容量
    let meta_row = sqlx::query(
        "SELECT COUNT(*) AS file_count, COALESCE(SUM(size_bytes), 0) AS total_bytes
         FROM cache_meta",
    )
    .fetch_one(&state.db)
    .await?;

    let file_count: i64 = meta_row.try_get("file_count").unwrap_or(0);
    let total_bytes: i64 = meta_row.try_get("total_bytes").unwrap_or(0);

    // 從 daily_stats 取本日 hit / miss
    let today_row = sqlx::query(
        "SELECT COALESCE(cache_hit, 0) AS h, COALESCE(cache_miss, 0) AS m
         FROM daily_stats
         WHERE date = date('now')",
    )
    .fetch_optional(&state.db)
    .await?;

    let (hit, miss) = match today_row {
        Some(row) => (
            row.try_get::<i64, _>("h").unwrap_or(0),
            row.try_get::<i64, _>("m").unwrap_or(0),
        ),
        None => (0, 0),
    };

    let total = (hit + miss).max(1);
    let hit_rate = (hit as f64) / (total as f64);

    Ok(CacheStats {
        file_count,
        total_bytes,
        hit_count: hit,
        miss_count: miss,
        hit_rate,
    })
}

/// 清空快取(刪除目錄下所有檔案 + 清 cache_meta)
#[tauri::command]
pub async fn cache_clear(state: State<'_, SharedState>) -> AppResult<u64> {
    let base = state.cache.base_dir();
    // 先刪檔
    if base.exists() {
        let mut stack = vec![base.clone()];
        while let Some(dir) = stack.pop() {
            let mut entries = tokio::fs::read_dir(&dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                let meta = entry.metadata().await?;
                let path = entry.path();
                if meta.is_dir() {
                    stack.push(path);
                } else {
                    let _ = tokio::fs::remove_file(&path).await;
                }
            }
        }
    }
    // 再清 cache_meta
    let result = sqlx::query("DELETE FROM cache_meta")
        .execute(&state.db)
        .await?;
    Ok(result.rows_affected())
}
