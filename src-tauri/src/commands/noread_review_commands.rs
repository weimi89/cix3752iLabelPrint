//! 讀碼失敗照片回顧:列某一天的 NoRead 存證照與標記、標記原因、各原因件數。

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::{AppError, AppResult, SharedState};

/// 可標的原因代碼(顯示文字在前端 i18n);不在這裡的一律拒收
pub const TAGS: [&str; 7] = ["label_back", "glare", "damaged", "small", "position", "no_label", "other"];

#[derive(Debug, Deserialize)]
pub struct NoreadReviewListReq {
    /// 'YYYY-MM-DD'(本機日期)
    pub day: String,
    /// 只列某個原因;空 = 全部
    #[serde(default)]
    pub tag: Option<String>,
    /// 只列還沒標的
    #[serde(default)]
    pub untagged_only: bool,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}
fn default_limit() -> i64 {
    60
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct NoreadReviewItem {
    pub response_id: i64,
    pub created_at: String,
    /// 存證照相對 key(`/captures/{photo_path}`);相機沒抓到幀時為 None
    pub photo_path: Option<String>,
    pub tag: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TagCount {
    pub tag: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
pub struct NoreadReviewListResp {
    pub items: Vec<NoreadReviewItem>,
    /// 符合篩選的件數(分頁用)
    pub total: i64,
    /// 當天 NoRead 總件數、已標件數、各原因件數(不受 tag／untagged 篩選影響)
    pub day_total: i64,
    pub tagged: i64,
    pub tag_counts: Vec<TagCount>,
}

fn valid_day(day: &str) -> bool {
    chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").is_ok()
}

#[tauri::command]
pub async fn noread_review_list(state: State<'_, SharedState>, req: NoreadReviewListReq) -> AppResult<NoreadReviewListResp> {
    if !valid_day(&req.day) {
        return Err(AppError::Other(format!("日期格式錯誤: {}", req.day)));
    }
    if let Some(t) = req.tag.as_deref().filter(|t| !t.is_empty()) {
        if !TAGS.contains(&t) {
            return Err(AppError::Other(format!("不認識的原因代碼: {t}")));
        }
    }
    let (from, to) = (format!("{} 00:00:00", req.day), format!("{} 23:59:59.999", req.day));
    let limit = req.limit.clamp(1, 500);
    let offset = req.offset.max(0);
    let tag = req.tag.filter(|t| !t.is_empty());

    // 篩選只有三種組合(全部／某原因／未標),SQL 各自寫死,值全部走 bind
    let (where_extra, bind_tag): (&str, Option<&str>) = match (&tag, req.untagged_only) {
        (Some(t), _) => ("AND r.tag = ?", Some(t.as_str())),
        (None, true) => ("AND r.tag IS NULL", None),
        (None, false) => ("", None),
    };
    let sql = format!(
        "SELECT q.response_id, q.created_at, q.photo_path, r.tag, r.note
           FROM parcel_query_log q LEFT JOIN noread_review r ON r.response_id = q.response_id
          WHERE q.query_no = 'NoRead' AND q.created_at >= ? AND q.created_at <= ? {where_extra}
          ORDER BY q.created_at DESC LIMIT ? OFFSET ?"
    );
    let count_sql = format!(
        "SELECT COUNT(*) FROM parcel_query_log q LEFT JOIN noread_review r ON r.response_id = q.response_id
          WHERE q.query_no = 'NoRead' AND q.created_at >= ? AND q.created_at <= ? {where_extra}"
    );
    let mut q1 = sqlx::query_as::<_, NoreadReviewItem>(sqlx::AssertSqlSafe(sql)).bind(&from).bind(&to);
    let mut q2 = sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(count_sql)).bind(&from).bind(&to);
    if let Some(t) = bind_tag {
        q1 = q1.bind(t.to_string());
        q2 = q2.bind(t.to_string());
    }
    let items = q1.bind(limit).bind(offset).fetch_all(&state.db).await?;
    let total = q2.fetch_one(&state.db).await?;

    let (day_total, tagged): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(r.tag IS NOT NULL), 0)
           FROM parcel_query_log q LEFT JOIN noread_review r ON r.response_id = q.response_id
          WHERE q.query_no = 'NoRead' AND q.created_at >= ? AND q.created_at <= ?",
    )
    .bind(&from)
    .bind(&to)
    .fetch_one(&state.db)
    .await?;
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT r.tag, COUNT(*) FROM parcel_query_log q JOIN noread_review r ON r.response_id = q.response_id
          WHERE q.query_no = 'NoRead' AND q.created_at >= ? AND q.created_at <= ? GROUP BY r.tag",
    )
    .bind(&from)
    .bind(&to)
    .fetch_all(&state.db)
    .await?;
    // 固定順序、沒標過的原因也列 0,前端不用自己補
    let tag_counts = TAGS
        .iter()
        .map(|t| TagCount { tag: t.to_string(), count: rows.iter().find(|(k, _)| k == t).map(|(_, n)| *n).unwrap_or(0) })
        .collect();
    Ok(NoreadReviewListResp { items, total, day_total, tagged, tag_counts })
}

/// 標記(或清除)一張存證照的原因;`tag` 空 = 清除
#[tauri::command]
pub async fn noread_review_tag(state: State<'_, SharedState>, response_id: i64, tag: Option<String>, note: Option<String>) -> AppResult<()> {
    let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM parcel_query_log WHERE response_id = ? AND query_no = 'NoRead'")
        .bind(response_id)
        .fetch_one(&state.db)
        .await?;
    if exists == 0 {
        return Err(AppError::Other(format!("找不到這筆讀碼失敗紀錄: {response_id}")));
    }
    match tag.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        None => {
            sqlx::query("DELETE FROM noread_review WHERE response_id = ?").bind(response_id).execute(&state.db).await?;
        }
        Some(t) if TAGS.contains(&t) => {
            sqlx::query(
                "INSERT INTO noread_review (response_id, tag, note, reviewed_at) VALUES (?, ?, ?, datetime('now','localtime'))
                 ON CONFLICT(response_id) DO UPDATE SET tag = excluded.tag, note = excluded.note, reviewed_at = excluded.reviewed_at",
            )
            .bind(response_id)
            .bind(t)
            .bind(note.as_deref().map(str::trim).filter(|n| !n.is_empty()))
            .execute(&state.db)
            .await?;
        }
        Some(t) => return Err(AppError::Other(format!("不認識的原因代碼: {t}"))),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 日期格式檢查() {
        assert!(valid_day("2026-09-18"));
        assert!(!valid_day("2026/09/18"));
        assert!(!valid_day("20260918"));
        assert!(!valid_day(""));
    }

    #[test]
    fn 原因代碼清單固定七種() {
        assert_eq!(TAGS.len(), 7);
        assert!(TAGS.contains(&"other"));
    }
}
