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

/// 清空快取(刪除目錄下所有檔案 + 清 cache_meta + 連帶清面單預產去重)
#[tauri::command]
pub async fn cache_clear(state: State<'_, SharedState>) -> AppResult<u64> {
    // 刪檔走 CacheManager(帶 marker 安全鎖:目錄未經初始化=可能誤設使用者資料夾 → 拒絕)
    state.cache.clear_all_files().await?;
    // 清 cache_meta
    let result = sqlx::query("DELETE FROM cache_meta")
        .execute(&state.db)
        .await?;
    // 「清快取必須連帶清 pregen_done」是後端不變量,在此維護(不靠 UI 記得補第二個 IPC):
    // 否則檔案已刪、pregen_done 仍標已預產 → 預產整批誤判略過、無法重跑。
    // 失敗只 warn 不整體失敗(前端 clearProcessed 仍會再清一次 + 重置前端鏡像)。
    if let Err(e) = state.pregen_done.clear(&state.db).await {
        tracing::warn!(?e, "cache_clear 連帶清 pregen_done 失敗");
    }
    Ok(result.rows_affected())
}
