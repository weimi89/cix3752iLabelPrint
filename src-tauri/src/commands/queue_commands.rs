use serde::{Deserialize, Serialize};
use sqlx::Row;
use tauri::State;

use crate::queue::QueueStats;
use crate::{AppResult, SharedState};

#[tauri::command]
pub async fn queue_stats(state: State<'_, SharedState>) -> AppResult<QueueStats> {
    state.queue.stats().await
}

#[derive(Debug, Deserialize)]
pub struct QueueListReq {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    100
}

#[derive(Debug, Serialize)]
pub struct QueueItem {
    pub id: i64,
    pub tracking_no: String,
    pub status: String,
    pub retry_count: i64,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub sent_at: Option<String>,
    pub payload_json: String,
}

#[tauri::command]
pub async fn queue_list(
    state: State<'_, SharedState>,
    req: QueueListReq,
) -> AppResult<Vec<QueueItem>> {
    let limit = req.limit.clamp(1, 1000);
    let offset = req.offset.max(0);

    let rows = if let Some(status) = req.status.as_deref() {
        sqlx::query(
            "SELECT id, tracking_no, status, retry_count, last_error,
                    created_at, updated_at, sent_at, payload_json
             FROM report_queue
             WHERE status = ?
             ORDER BY id DESC
             LIMIT ? OFFSET ?",
        )
        .bind(status)
        .bind(limit)
        .bind(offset)
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query(
            "SELECT id, tracking_no, status, retry_count, last_error,
                    created_at, updated_at, sent_at, payload_json
             FROM report_queue
             ORDER BY id DESC
             LIMIT ? OFFSET ?",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&state.db)
        .await?
    };

    Ok(rows
        .into_iter()
        .map(|row| QueueItem {
            id: row.try_get("id").unwrap_or(0),
            tracking_no: row.try_get("tracking_no").unwrap_or_default(),
            status: row.try_get("status").unwrap_or_default(),
            retry_count: row.try_get("retry_count").unwrap_or(0),
            last_error: row.try_get("last_error").ok(),
            created_at: row.try_get("created_at").unwrap_or_default(),
            updated_at: row.try_get("updated_at").unwrap_or_default(),
            sent_at: row.try_get("sent_at").ok(),
            payload_json: row.try_get("payload_json").unwrap_or_default(),
        })
        .collect())
}

/// 把 failed 全部重設為 pending,讓 worker 下一輪重試
#[tauri::command]
pub async fn queue_retry_failed(state: State<'_, SharedState>) -> AppResult<u64> {
    let result = sqlx::query(
        "UPDATE report_queue
         SET status='pending', retry_count=0, last_error=NULL, updated_at=datetime('now')
         WHERE status='failed'",
    )
    .execute(&state.db)
    .await?;
    Ok(result.rows_affected())
}

#[derive(Debug, Deserialize)]
pub struct QueuePurgeReq {
    #[serde(default = "default_purge_status")]
    pub status: String,
    #[serde(default = "default_older_than_days")]
    pub older_than_days: i64,
}

fn default_purge_status() -> String {
    "success".to_string()
}
fn default_older_than_days() -> i64 {
    7
}

/// 清除指定狀態、超過 N 天的舊紀錄
#[tauri::command]
pub async fn queue_purge(
    state: State<'_, SharedState>,
    req: QueuePurgeReq,
) -> AppResult<u64> {
    let days = req.older_than_days.max(0);
    let cutoff = format!("-{days} days");
    let result = sqlx::query(
        "DELETE FROM report_queue
         WHERE status = ?
           AND created_at < datetime('now', ?)",
    )
    .bind(&req.status)
    .bind(&cutoff)
    .execute(&state.db)
    .await?;
    Ok(result.rows_affected())
}
