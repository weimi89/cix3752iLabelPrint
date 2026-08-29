use serde::{Deserialize, Serialize};
use sqlx::Row;
use tauri::State;

use super::search_terms::{in_clause, split_nos};
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
    /// 關鍵字(對 tracking_no / sort_channel / job_sticker 做 LIKE 模糊比對)
    #[serde(default)]
    pub keyword: Option<String>,
    /// 物流單號:可一次多組(逗號 / 空白 / 換行分隔),精確比對
    #[serde(default)]
    pub tracking_no: Option<String>,
    /// 只列出「工控機從未回報」的項目(直印模式下用來抓工控機回報鏈是否斷掉)
    #[serde(default)]
    pub unreported_only: bool,
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
    pub response_id: Option<i64>,
    pub status: String,
    pub retry_count: i64,
    pub created_at: String,
    pub updated_at: String,
    pub sent_at: Option<String>,
    pub payload_json: String,
    pub sort_channel: Option<String>,
    pub job_sticker: Option<String>,
    /// 這筆佇列是誰建立的:`ipc`(工控機回報)/ `direct_print`(中介機直印後自補)
    pub source: String,
    /// 推送失敗原因、或被攔截時的原因(直印失敗時寫入),供佇列歷史頁直接顯示
    pub last_error: Option<String>,
    /// 工控機實際回報的時間;None = 從未回報(直印模式下代表只有面單印出、沒有工控機確認)
    pub ipc_reported_at: Option<String>,
}

#[tauri::command]
pub async fn queue_list(
    state: State<'_, SharedState>,
    req: QueueListReq,
) -> AppResult<Vec<QueueItem>> {
    let limit = req.limit.clamp(1, 1000);
    let offset = req.offset.max(0);
    let keyword = req
        .keyword
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());

    let tracking_nos = split_nos(req.tracking_no.as_deref());

    // 動態組 WHERE(status / keyword / 單號清單可任意組合)
    let mut where_clauses: Vec<String> = Vec::new();
    if req.status.is_some() {
        where_clauses.push("status = ?".into());
    }
    if keyword.is_some() {
        where_clauses.push("(tracking_no LIKE ? OR sort_channel LIKE ? OR job_sticker LIKE ?)".into());
    }
    if !tracking_nos.is_empty() {
        where_clauses.push(in_clause("tracking_no", tracking_nos.len()));
    }
    if req.unreported_only {
        where_clauses.push("ipc_reported_at IS NULL".into());
    }

    let where_sql = if where_clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", where_clauses.join(" AND "))
    };

    let sql = format!(
        "SELECT id, tracking_no, response_id, status, retry_count,
                created_at, updated_at, sent_at, payload_json,
                sort_channel, job_sticker, source, ipc_reported_at, last_error
         FROM report_queue
         {where_sql}
         ORDER BY id DESC
         LIMIT ? OFFSET ?"
    );

    let like = keyword.map(|kw| format!("%{kw}%"));
    let mut query = sqlx::query(&sql);
    if let Some(status) = req.status.as_deref() {
        query = query.bind(status);
    }
    if let Some(like) = like.as_deref() {
        query = query.bind(like).bind(like).bind(like);
    }
    for v in &tracking_nos { query = query.bind(v.as_str()); }
    let rows = query.bind(limit).bind(offset).fetch_all(&state.db).await?;

    Ok(rows
        .into_iter()
        .map(|row| QueueItem {
            id: row.try_get("id").unwrap_or(0),
            tracking_no: row.try_get("tracking_no").unwrap_or_default(),
            response_id: row.try_get("response_id").ok(),
            status: row.try_get("status").unwrap_or_default(),
            retry_count: row.try_get("retry_count").unwrap_or(0),
            created_at: row.try_get("created_at").unwrap_or_default(),
            updated_at: row.try_get("updated_at").unwrap_or_default(),
            sent_at: row.try_get("sent_at").ok(),
            payload_json: row.try_get("payload_json").unwrap_or_default(),
            sort_channel: row.try_get("sort_channel").ok(),
            job_sticker: row.try_get("job_sticker").ok(),
            source: row.try_get("source").unwrap_or_else(|_| "ipc".to_string()),
            ipc_reported_at: row.try_get("ipc_reported_at").ok(),
            last_error: row.try_get("last_error").ok(),
        })
        .collect())
}

/// 把 failed(與殘留 sending)全部重設為 pending,清退避讓 worker 下一輪立即重試。
///
/// **被攔截的件(直印失敗)一律不動**:它們刻意不推送雲端,若跟著轉回待送,
/// 畫面上會顯示成「已重新排隊」但 worker 永遠不撿 —— 看起來在跑、實際永久卡死,
/// 比留在原狀更難察覺。
#[tauri::command]
pub async fn queue_retry_failed(state: State<'_, SharedState>) -> AppResult<u64> {
    let result = sqlx::query(
        "UPDATE report_queue
         SET status='pending', retry_count=0, next_attempt_at=NULL, updated_at=datetime('now','localtime')
         WHERE status IN ('failed', 'sending') AND cancel_requested=0",
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
    // created_at 以 localtime 寫入,比對基準也要 localtime,否則 +8 時區會有 8 小時偏差
    let result = sqlx::query(
        "DELETE FROM report_queue
         WHERE status = ?
           AND created_at < datetime('now','localtime', ?)",
    )
    .bind(&req.status)
    .bind(&cutoff)
    .execute(&state.db)
    .await?;
    Ok(result.rows_affected())
}
