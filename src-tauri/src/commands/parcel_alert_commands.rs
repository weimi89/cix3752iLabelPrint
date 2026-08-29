use serde::{Deserialize, Serialize};
use sqlx::Row;
use tauri::State;

use crate::db::DbPool;
use crate::{AppResult, SharedState};

#[derive(Debug, Deserialize)]
pub struct ParcelAlertListReq {
    /// 關鍵字(在 query_no / shipping_no / message 做 LIKE 模糊)
    #[serde(default)]
    pub keyword: Option<String>,
    /// 異常類別(store_closed / unconfirmed / not_found …);省略或空字串 = 全部
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 { 25 }

#[derive(Debug, Serialize)]
pub struct ParcelAlertItem {
    pub id: i64,
    pub kind: String,
    pub code: Option<String>,
    pub query_no: Option<String>,
    pub shipping_no: Option<String>,
    pub message: Option<String>,
    pub channel_code: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct ParcelAlertListResp {
    pub items: Vec<ParcelAlertItem>,
    pub total: i64,
}

/// 桌面回看用:雲端查件異常清單(門市關轉 / 未確認 / 找不到 …),依 id DESC 分頁
/// - keyword 對 query_no / shipping_no / message 做 LIKE
/// - kind 精確比對類別
///
/// 注意:不可用 `?1` 編號參數混搭匿名 `?`,sqlx 依 bind 順序做位置綁定,
/// 與 SQLite 的編號規則對不上(見 tests/parcel_query_log_keyword.rs)
#[tauri::command]
pub async fn parcel_alert_list(
    state: State<'_, SharedState>,
    req: ParcelAlertListReq,
) -> AppResult<ParcelAlertListResp> {
    Ok(list_parcel_alerts(&state.db, &req).await?)
}

/// 查詢本體(與 Tauri State 解耦,供整合測試直接餵 in-memory pool)
pub async fn list_parcel_alerts(
    db: &DbPool,
    req: &ParcelAlertListReq,
) -> Result<ParcelAlertListResp, sqlx::Error> {
    let limit = req.limit.clamp(1, 1000);
    let offset = req.offset.max(0);

    let like = req
        .keyword
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|kw| format!("%{kw}%"));
    let kind = req
        .kind
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    let mut where_clauses: Vec<&str> = Vec::new();
    if like.is_some() {
        where_clauses.push("(query_no LIKE ? OR shipping_no LIKE ? OR message LIKE ?)");
    }
    if kind.is_some() {
        where_clauses.push("kind = ?");
    }
    let where_sql = if where_clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", where_clauses.join(" AND "))
    };

    let sql = format!(
        "SELECT id, kind, code, query_no, shipping_no, message, channel_code, created_at
         FROM parcel_alert
         {where_sql}
         ORDER BY id DESC
         LIMIT ? OFFSET ?"
    );
    let count_sql = format!("SELECT COUNT(*) AS n FROM parcel_alert {where_sql}");

    let mut query = sqlx::query(&sql);
    if let Some(like) = like.as_deref() {
        query = query.bind(like).bind(like).bind(like);
    }
    if let Some(k) = kind.as_deref() {
        query = query.bind(k);
    }
    let rows = query.bind(limit).bind(offset).fetch_all(db).await?;

    let mut count_query = sqlx::query(&count_sql);
    if let Some(like) = like.as_deref() {
        count_query = count_query.bind(like).bind(like).bind(like);
    }
    if let Some(k) = kind.as_deref() {
        count_query = count_query.bind(k);
    }
    let total: i64 = count_query
        .fetch_one(db)
        .await?
        .try_get("n")
        .unwrap_or(0);

    let items = rows
        .into_iter()
        .map(|r| ParcelAlertItem {
            id: r.try_get("id").unwrap_or_default(),
            kind: r.try_get("kind").unwrap_or_default(),
            code: r.try_get("code").ok().flatten(),
            query_no: r.try_get("query_no").ok().flatten(),
            shipping_no: r.try_get("shipping_no").ok().flatten(),
            message: r.try_get("message").ok().flatten(),
            channel_code: r.try_get("channel_code").ok().flatten(),
            created_at: r.try_get("created_at").unwrap_or_default(),
        })
        .collect();

    Ok(ParcelAlertListResp { items, total })
}
