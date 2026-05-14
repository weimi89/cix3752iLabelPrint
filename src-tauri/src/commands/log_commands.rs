use serde::{Deserialize, Serialize};
use sqlx::Row;
use tauri::State;

use crate::{AppResult, SharedState};

#[derive(Debug, Deserialize)]
pub struct EventLogReq {
    #[serde(default)]
    pub level: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    200
}

#[derive(Debug, Serialize)]
pub struct EventLogItem {
    pub id: i64,
    pub level: String,
    pub category: String,
    pub action: String,
    pub message: String,
    pub context_json: Option<String>,
    pub created_at: String,
}

#[tauri::command]
pub async fn event_log_list(
    state: State<'_, SharedState>,
    req: EventLogReq,
) -> AppResult<Vec<EventLogItem>> {
    let limit = req.limit.clamp(1, 1000);
    let offset = req.offset.max(0);

    // 動態組 WHERE
    let mut where_clauses: Vec<&str> = Vec::new();
    if req.level.is_some() {
        where_clauses.push("level = ?");
    }
    if req.category.is_some() {
        where_clauses.push("category = ?");
    }

    let where_sql = if where_clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", where_clauses.join(" AND "))
    };

    let sql = format!(
        "SELECT id, level, category, action, message, context_json, created_at
         FROM event_log
         {where_sql}
         ORDER BY id DESC
         LIMIT ? OFFSET ?"
    );

    let mut query = sqlx::query(&sql);
    if let Some(l) = req.level.as_deref() {
        query = query.bind(l);
    }
    if let Some(c) = req.category.as_deref() {
        query = query.bind(c);
    }
    let rows = query.bind(limit).bind(offset).fetch_all(&state.db).await?;

    Ok(rows
        .into_iter()
        .map(|row| EventLogItem {
            id: row.try_get("id").unwrap_or(0),
            level: row.try_get("level").unwrap_or_default(),
            category: row.try_get("category").unwrap_or_default(),
            action: row.try_get("action").unwrap_or_default(),
            message: row.try_get("message").unwrap_or_default(),
            context_json: row.try_get("context_json").ok(),
            created_at: row.try_get("created_at").unwrap_or_default(),
        })
        .collect())
}

#[derive(Debug, Deserialize)]
pub struct DailyStatsReq {
    #[serde(default = "default_days")]
    pub days: i64,
}

fn default_days() -> i64 {
    7
}

#[derive(Debug, Serialize)]
pub struct DailyStatsItem {
    pub date: String,
    pub request_count: i64,
    pub success_count: i64,
    pub cache_hit: i64,
    pub cache_miss: i64,
}

#[tauri::command]
pub async fn daily_stats(
    state: State<'_, SharedState>,
    req: DailyStatsReq,
) -> AppResult<Vec<DailyStatsItem>> {
    let days = req.days.clamp(1, 365);
    let cutoff = format!("-{} days", days - 1);
    let rows = sqlx::query(
        "SELECT date, request_count, success_count, cache_hit, cache_miss
         FROM daily_stats
         WHERE date >= date('now', ?)
         ORDER BY date DESC",
    )
    .bind(&cutoff)
    .fetch_all(&state.db)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| DailyStatsItem {
            date: row.try_get("date").unwrap_or_default(),
            request_count: row.try_get("request_count").unwrap_or(0),
            success_count: row.try_get("success_count").unwrap_or(0),
            cache_hit: row.try_get("cache_hit").unwrap_or(0),
            cache_miss: row.try_get("cache_miss").unwrap_or(0),
        })
        .collect())
}
