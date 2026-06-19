use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqliteRow;
use sqlx::Row;

use crate::cloud::CloudClient;
use crate::db::DbPool;
use crate::event_log;
use crate::models::ReportPayload;
use crate::{AppError, AppResult};

/// 推送最大重試次數,達到後放棄(status=failed 並寫 last_error,以 error 級事件留痕,不靜默)
const MAX_RETRY: i64 = 10;

/// 工控機回報佇列管理 — 寫入即回 200 給工控機,背景 worker 嘗試推送雲端
#[derive(Clone)]
pub struct QueueManager {
    inner: Arc<Inner>,
}

struct Inner {
    db: DbPool,
    cloud: CloudClient,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueStats {
    pub pending: i64,
    pub sending: i64,
    pub success: i64,
    pub failed: i64,
}

impl QueueManager {
    pub fn new(db: DbPool, cloud: CloudClient) -> Self {
        Self {
            inner: Arc::new(Inner { db, cloud }),
        }
    }

    /// 寫入一筆工控機回報歷史；status 寫 pending,等 worker 推送雲端
    /// sort_channel / job_sticker 由 server 端反查後傳入,方便 QueueLogPage 直接顯示
    pub async fn enqueue(
        &self,
        payload: &ReportPayload,
        tracking_no: &str,
        sort_channel: Option<&str>,
        job_sticker: Option<&str>,
    ) -> AppResult<i64> {
        let json = serde_json::to_string(payload)?;
        let row = sqlx::query(
            "INSERT INTO report_queue
                 (tracking_no, payload_json, response_id, sort_channel, job_sticker, status, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, 'pending', datetime('now','localtime'), datetime('now','localtime'))",
        )
        .bind(tracking_no)
        .bind(&json)
        .bind(payload.response_id)
        .bind(sort_channel)
        .bind(job_sticker)
        .execute(&self.inner.db)
        .await?;
        Ok(row.last_insert_rowid())
    }

    /// 透過 queue_id 反查當初工控機回報時帶的 response_id
    pub async fn lookup_response_id(&self, queue_id: i64) -> AppResult<Option<i64>> {
        let row = sqlx::query("SELECT response_id FROM report_queue WHERE id = ?")
            .bind(queue_id)
            .fetch_optional(&self.inner.db)
            .await?;
        Ok(row.and_then(|r| r.try_get::<Option<i64>, _>("response_id").ok().flatten()))
    }

    /// 取得佇列統計
    pub async fn stats(&self) -> AppResult<QueueStats> {
        let row = sqlx::query(
            "SELECT
                 COALESCE(SUM(CASE WHEN status='pending' THEN 1 ELSE 0 END), 0) AS pending,
                 COALESCE(SUM(CASE WHEN status='sending' THEN 1 ELSE 0 END), 0) AS sending,
                 COALESCE(SUM(CASE WHEN status='success' THEN 1 ELSE 0 END), 0) AS success,
                 COALESCE(SUM(CASE WHEN status='failed'  THEN 1 ELSE 0 END), 0) AS failed
             FROM report_queue",
        )
        .fetch_one(&self.inner.db)
        .await?;

        Ok(QueueStats {
            pending: row.try_get("pending").unwrap_or(0),
            sending: row.try_get("sending").unwrap_or(0),
            success: row.try_get("success").unwrap_or(0),
            failed: row.try_get("failed").unwrap_or(0),
        })
    }

    /// 啟動背景 worker:每 5 秒掃一次到期(next_attempt_at <= now)的 pending / failed 推送雲端
    pub fn start_worker(&self) {
        let inner = self.inner.clone();
        tokio::spawn(async move {
            // 啟動回收:上次程序結束前殘留在 sending 的項目(App 中途關閉/崩潰)重設為 pending,
            // 否則它們因不在 worker 查詢條件內會永久卡死(殭屍佇列項)
            if let Err(e) = recover_stale_sending(&inner.db).await {
                tracing::warn!(?e, "啟動回收殘留 sending 失敗");
            }
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                if let Err(e) = process_once(&inner).await {
                    tracing::warn!(?e, "Queue worker 推送發生錯誤");
                }
            }
        });
    }
}

/// 回收殘留 sending:把卡在 sending 的項目重設回 pending(清退避立即可送)。
/// 啟動時呼叫一次,救回上次崩潰/關閉遺留的殭屍項。
async fn recover_stale_sending(db: &DbPool) -> AppResult<()> {
    let res = sqlx::query(
        "UPDATE report_queue
         SET status='pending', next_attempt_at=NULL, updated_at=datetime('now','localtime')
         WHERE status='sending'",
    )
    .execute(db)
    .await?;
    if res.rows_affected() > 0 {
        event_log::log_bg(db.clone(), "warn", "queue", "回收殘留",
            format!("啟動回收 {} 筆殘留 sending 為 pending", res.rows_affected()));
    }
    Ok(())
}

async fn process_once(inner: &Inner) -> AppResult<()> {
    let rows = sqlx::query(
        "SELECT id, response_id, job_sticker, retry_count
         FROM report_queue
         WHERE retry_count < ?
           AND (
                status IN ('pending', 'failed')
                -- in-session 安全網:卡在 sending 超過 60 秒(中途 DB 錯誤未即時回收)也重撿,
                -- 與啟動回收互補,確保任何路徑下的 sending 都不會永久殭屍
                OR (status = 'sending' AND updated_at < datetime('now','localtime','-60 seconds'))
           )
           -- 退避閘:只撿到期項目(NULL=立即可送)
           AND (next_attempt_at IS NULL OR next_attempt_at <= datetime('now','localtime'))
         ORDER BY id ASC
         LIMIT 20",
    )
    .bind(MAX_RETRY)
    .fetch_all(&inner.db)
    .await?;

    // 逐筆隔離:單筆 DB 錯誤只記 warn 並跳下一筆,不讓整批 early-return
    // (避免「已標 sending 卻因後續錯誤中斷」造成殭屍;殘留也有 60s 安全網兜底)
    for row in rows {
        let id = row.try_get::<i64, _>("id").unwrap_or(-1);
        if let Err(e) = process_row(inner, row).await {
            tracing::warn!(queue_id = id, ?e, "處理佇列項目失敗,跳過續處理下一筆");
        }
    }
    Ok(())
}

/// 處理單一佇列項目:標 sending → 推送 → 依結果改 success / pending(未登入)/ failed(退避)
async fn process_row(inner: &Inner, row: SqliteRow) -> AppResult<()> {
    let id: i64 = row.try_get("id")?;
    let response_id: Option<i64> = row.try_get("response_id").ok().flatten();
    let job_sticker: Option<String> = row.try_get("job_sticker").ok().flatten();
    let retry_count: i64 = row.try_get("retry_count")?;

    sqlx::query("UPDATE report_queue SET status='sending', updated_at=datetime('now','localtime') WHERE id=?")
        .bind(id)
        .execute(&inner.db)
        .await?;

    // 構造 logistic-cat webhook payload (job_user 由 cloud client 內部從設定注入)
    let mut payload = serde_json::json!({
        "job_id": response_id,
        "job_sticker": job_sticker,
    });

    match inner.cloud.notify_logistic_cat(&mut payload).await {
        Ok(()) => {
            sqlx::query(
                "UPDATE report_queue
                 SET status='success', sent_at=datetime('now','localtime'), updated_at=datetime('now','localtime')
                 WHERE id=?",
            )
            .bind(id)
            .execute(&inner.db)
            .await?;
            event_log::log_bg(inner.db.clone(), "info", "queue", "推送成功",
                format!("回報推送成功 queue_id={id}"));
        }
        Err(AppError::Unauthorized) => {
            // 未登入:不累加 retry,清退避下一輪立即繼續
            sqlx::query(
                "UPDATE report_queue
                 SET status='pending', next_attempt_at=NULL, updated_at=datetime('now','localtime')
                 WHERE id=?",
            )
            .bind(id)
            .execute(&inner.db)
            .await?;
        }
        Err(e) => {
            tracing::warn!(queue_id = id, ?e, "webhook 推送失敗");
            mark_failed(&inner.db, id, retry_count, &e.to_string()).await?;
        }
    }
    Ok(())
}

/// capped 指數退避秒數:5 * 2^(n-1),上限 300 秒。
/// n=1→5, 2→10, 3→20, 4→40, 5→80, 6→160, 7→300(本應 320,封頂), 8+→300。
/// `(n-1).min(6)` 鎖位移上限避免溢位,封頂用 .min(300)。
fn backoff_seconds(new_count: i64) -> i64 {
    (5_i64 * (1_i64 << (new_count - 1).clamp(0, 6))).min(300)
}

/// 標記推送失敗:寫 last_error、累加 retry_count;
/// 未達上限 → capped 指數退避設下次可送時間;達上限 → 放棄並以 error 級事件留痕(不靜默)。
async fn mark_failed(db: &DbPool, id: i64, retry_count: i64, last_error: &str) -> AppResult<()> {
    let new_count = retry_count + 1;

    if new_count >= MAX_RETRY {
        sqlx::query(
            "UPDATE report_queue
             SET status='failed', retry_count=?, last_error=?, updated_at=datetime('now','localtime')
             WHERE id=?",
        )
        .bind(new_count)
        .bind(last_error)
        .bind(id)
        .execute(db)
        .await?;
        // 達上限放棄改為可觀測:error 級事件 + last_error 落庫,不再靜默丟件
        event_log::log_bg(db.clone(), "error", "queue", "推送放棄",
            format!("回報達重試上限 {MAX_RETRY} 次放棄 queue_id={id}:{last_error}"));
        return Ok(());
    }

    // capped 指數退避:下次可送 = now + backoff_seconds(n) 秒
    let backoff = backoff_seconds(new_count);
    sqlx::query(
        "UPDATE report_queue
         SET status='failed', retry_count=?, last_error=?,
             next_attempt_at=datetime('now','localtime', ?),
             updated_at=datetime('now','localtime')
         WHERE id=?",
    )
    .bind(new_count)
    .bind(last_error)
    .bind(format!("+{backoff} seconds"))
    .bind(id)
    .execute(db)
    .await?;
    event_log::log_bg(db.clone(), "warn", "queue", "推送失敗",
        format!("回報推送失敗 queue_id={id} retry={new_count},{backoff}s 後重試"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{backoff_seconds, MAX_RETRY};

    #[test]
    fn backoff_is_capped_exponential() {
        // 5 * 2^(n-1),上限 300
        assert_eq!(backoff_seconds(1), 5);
        assert_eq!(backoff_seconds(2), 10);
        assert_eq!(backoff_seconds(3), 20);
        assert_eq!(backoff_seconds(4), 40);
        assert_eq!(backoff_seconds(5), 80);
        assert_eq!(backoff_seconds(6), 160);
        // n=7 本應 320,封頂 300
        assert_eq!(backoff_seconds(7), 300);
    }

    #[test]
    fn backoff_never_exceeds_cap_until_give_up() {
        // 放棄前最後一次重試(MAX_RETRY-1=9)仍封頂 300,不溢位
        for n in 1..MAX_RETRY {
            let b = backoff_seconds(n);
            assert!((5..=300).contains(&b), "n={n} backoff={b} 超出 [5,300]");
        }
    }
}
