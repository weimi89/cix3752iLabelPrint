use serde::{Deserialize, Serialize};
use sqlx::Row;
use tauri::State;

use crate::{AppResult, SharedState};

#[derive(Debug, Deserialize)]
pub struct ParcelQueryLogListReq {
    /// 關鍵字(在 query_no / tracking_no / label_key 做 LIKE 模糊)
    #[serde(default)]
    pub keyword: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 { 25 }

#[derive(Debug, Serialize)]
pub struct ParcelQueryLogItem {
    pub response_id: i64,
    pub query_no: String,
    pub tracking_no: String,
    pub shipping_provider: Option<String>,
    pub sort_channel: Option<String>,
    pub print_profile: Option<String>,
    pub should_print: i64,
    pub label_key: Option<String>,
    pub created_at: String,
    pub cloud_ms: Option<i64>,
    pub label_ms: Option<i64>,
    pub total_ms: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ParcelQueryLogListResp {
    pub items: Vec<ParcelQueryLogItem>,
    pub total: i64,
}

/// 列出 GET /api/parcel 的查詢歷史
/// - 預設依 created_at DESC 排序
/// - keyword 對 query_no / tracking_no / label_key 做 LIKE
#[tauri::command]
pub async fn parcel_query_log_list(
    state: State<'_, SharedState>,
    req: ParcelQueryLogListReq,
) -> AppResult<ParcelQueryLogListResp> {
    let limit = req.limit.clamp(1, 1000);
    let offset = req.offset.max(0);

    let kw = req.keyword.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty());

    let (rows, total) = if let Some(kw) = kw {
        let like = format!("%{kw}%");
        let rows = sqlx::query(
            "SELECT response_id, query_no, tracking_no, shipping_provider, sort_channel, print_profile,
                    should_print, label_key, created_at, cloud_ms, label_ms, total_ms
             FROM parcel_query_log
             WHERE query_no LIKE ?1 OR tracking_no LIKE ?1 OR label_key LIKE ?1
             ORDER BY created_at DESC
             LIMIT ? OFFSET ?",
        )
        .bind(&like)
        .bind(limit)
        .bind(offset)
        .fetch_all(&state.db)
        .await?;
        let total: i64 = sqlx::query(
            "SELECT COUNT(*) AS n
             FROM parcel_query_log
             WHERE query_no LIKE ?1 OR tracking_no LIKE ?1 OR label_key LIKE ?1",
        )
        .bind(&like)
        .fetch_one(&state.db)
        .await?
        .try_get("n")
        .unwrap_or(0);
        (rows, total)
    } else {
        let rows = sqlx::query(
            "SELECT response_id, query_no, tracking_no, shipping_provider, sort_channel, print_profile,
                    should_print, label_key, created_at, cloud_ms, label_ms, total_ms
             FROM parcel_query_log
             ORDER BY created_at DESC
             LIMIT ? OFFSET ?",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&state.db)
        .await?;
        let total: i64 = sqlx::query("SELECT COUNT(*) AS n FROM parcel_query_log")
            .fetch_one(&state.db)
            .await?
            .try_get("n")
            .unwrap_or(0);
        (rows, total)
    };

    let items = rows
        .into_iter()
        .map(|row| ParcelQueryLogItem {
            response_id: row.try_get("response_id").unwrap_or(0),
            query_no: row.try_get("query_no").unwrap_or_default(),
            tracking_no: row.try_get("tracking_no").unwrap_or_default(),
            shipping_provider: row.try_get("shipping_provider").ok(),
            sort_channel: row.try_get("sort_channel").ok(),
            print_profile: row.try_get("print_profile").ok(),
            should_print: row.try_get("should_print").unwrap_or(0),
            label_key: row.try_get("label_key").ok(),
            created_at: row.try_get("created_at").unwrap_or_default(),
            cloud_ms: row.try_get("cloud_ms").ok(),
            label_ms: row.try_get("label_ms").ok(),
            total_ms: row.try_get("total_ms").ok(),
        })
        .collect();

    Ok(ParcelQueryLogListResp { items, total })
}
