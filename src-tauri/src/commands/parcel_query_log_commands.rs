use serde::{Deserialize, Serialize};
use sqlx::Row;
use tauri::State;

use crate::{AppResult, SharedState};

#[derive(Debug, Deserialize)]
pub struct ParcelQueryLogListReq {
    /// 關鍵字(在 query_no / tracking_no / label_key 做 LIKE 模糊)
    #[serde(default)]
    pub keyword: Option<String>,
    /// 耗時篩選欄位:"cloud" / "label" / "total";省略或其他值 = 任一欄位超過即列出
    #[serde(default)]
    pub ms_field: Option<String>,
    /// 耗時門檻(毫秒),只列出 >= 此值的記錄;省略或 <=0 不篩選
    #[serde(default)]
    pub min_ms: Option<i64>,
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

    let like = req
        .keyword
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|kw| format!("%{kw}%"));
    let min_ms = req.min_ms.filter(|v| *v > 0);
    // 耗時欄位白名單(避免拼 SQL 注入);非三者之一視為「任一欄位」
    let ms_single = matches!(req.ms_field.as_deref(), Some("cloud") | Some("label") | Some("total"));

    // 動態組 WHERE(關鍵字 / 耗時門檻可任意組合)
    // 注意:不可用 `?1` 編號參數混搭匿名 `?` —— sqlx 依 bind 順序做位置綁定,
    // 與 SQLite 的參數編號規則對不上,LIKE 字串會被綁進 LIMIT 造成
    // datatype mismatch(見 tests/parcel_query_log_keyword.rs)
    let mut where_clauses: Vec<&str> = Vec::new();
    if like.is_some() {
        where_clauses.push("(query_no LIKE ? OR tracking_no LIKE ? OR label_key LIKE ?)");
    }
    if min_ms.is_some() {
        where_clauses.push(match req.ms_field.as_deref() {
            Some("cloud") => "cloud_ms >= ?",
            Some("label") => "label_ms >= ?",
            Some("total") => "total_ms >= ?",
            _ => "(cloud_ms >= ? OR label_ms >= ? OR total_ms >= ?)",
        });
    }
    let where_sql = if where_clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", where_clauses.join(" AND "))
    };

    let sql = format!(
        "SELECT response_id, query_no, tracking_no, shipping_provider, sort_channel, print_profile,
                should_print, label_key, created_at, cloud_ms, label_ms, total_ms
         FROM parcel_query_log
         {where_sql}
         ORDER BY created_at DESC
         LIMIT ? OFFSET ?"
    );
    let count_sql = format!("SELECT COUNT(*) AS n FROM parcel_query_log {where_sql}");

    let mut query = sqlx::query(&sql);
    if let Some(like) = like.as_deref() {
        query = query.bind(like).bind(like).bind(like);
    }
    if let Some(ms) = min_ms {
        query = query.bind(ms);
        if !ms_single {
            query = query.bind(ms).bind(ms);
        }
    }
    let rows = query.bind(limit).bind(offset).fetch_all(&state.db).await?;

    let mut count_query = sqlx::query(&count_sql);
    if let Some(like) = like.as_deref() {
        count_query = count_query.bind(like).bind(like).bind(like);
    }
    if let Some(ms) = min_ms {
        count_query = count_query.bind(ms);
        if !ms_single {
            count_query = count_query.bind(ms).bind(ms);
        }
    }
    let total: i64 = count_query
        .fetch_one(&state.db)
        .await?
        .try_get("n")
        .unwrap_or(0);

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
