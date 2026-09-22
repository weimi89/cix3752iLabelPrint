//! 讀碼失敗照片回顧:列某一天的 NoRead 存證照給主管翻。
//! 不做人工標原因——面單朝下、反光這類從照片分不出來,標記永遠是 0;分揀機那邊由讀碼站回應判的原因在分揀控制程式。

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::{AppError, AppResult, SharedState};

#[derive(Debug, Deserialize)]
pub struct NoreadReviewListReq {
    /// 'YYYY-MM-DD'(本機日期)
    pub day: String,
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
}

#[derive(Debug, Serialize)]
pub struct NoreadReviewListResp {
    pub items: Vec<NoreadReviewItem>,
    /// 當天 NoRead 總件數(分頁用)
    pub total: i64,
}

fn valid_day(day: &str) -> bool {
    chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").is_ok()
}

#[tauri::command]
pub async fn noread_review_list(state: State<'_, SharedState>, req: NoreadReviewListReq) -> AppResult<NoreadReviewListResp> {
    if !valid_day(&req.day) {
        return Err(AppError::Other(format!("日期格式錯誤: {}", req.day)));
    }
    let (from, to) = (format!("{} 00:00:00", req.day), format!("{} 23:59:59.999", req.day));
    let items = sqlx::query_as::<_, NoreadReviewItem>(
        "SELECT response_id, created_at, photo_path FROM parcel_query_log
          WHERE query_no = 'NoRead' AND created_at >= ? AND created_at <= ?
          ORDER BY created_at DESC LIMIT ? OFFSET ?",
    )
    .bind(&from)
    .bind(&to)
    .bind(req.limit.clamp(1, 500))
    .bind(req.offset.max(0))
    .fetch_all(&state.db)
    .await?;
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM parcel_query_log WHERE query_no = 'NoRead' AND created_at >= ? AND created_at <= ?")
        .bind(&from)
        .bind(&to)
        .fetch_one(&state.db)
        .await?;
    Ok(NoreadReviewListResp { items, total })
}
