use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::cloud::CloudClient;
use crate::db::DbPool;
use crate::event_log;
use crate::models::ReportPayload;
use crate::{AppError, AppResult};

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

    /// 啟動背景 worker:每 5 秒掃一次 pending / failed 的紀錄推送雲端
    pub fn start_worker(&self) {
        let inner = self.inner.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                if let Err(e) = process_once(&inner).await {
                    tracing::warn!(?e, "Queue worker 推送發生錯誤");
                }
            }
        });
    }
}

async fn process_once(inner: &Inner) -> AppResult<()> {
    let rows = sqlx::query(
        "SELECT id, response_id, job_sticker, retry_count
         FROM report_queue
         WHERE status IN ('pending', 'failed')
           AND retry_count < 10
         ORDER BY id ASC
         LIMIT 20",
    )
    .fetch_all(&inner.db)
    .await?;

    for row in rows {
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
                // 未登入:不累加 retry,下一輪繼續
                sqlx::query(
                    "UPDATE report_queue
                     SET status='pending', updated_at=datetime('now','localtime')
                     WHERE id=?",
                )
                .bind(id)
                .execute(&inner.db)
                .await?;
            }
            Err(e) => {
                tracing::warn!(queue_id = id, ?e, "webhook 推送失敗");
                mark_failed(&inner.db, id, retry_count).await?;
            }
        }
    }
    Ok(())
}

async fn mark_failed(db: &DbPool, id: i64, retry_count: i64) -> AppResult<()> {
    sqlx::query(
        "UPDATE report_queue
         SET status='failed', retry_count=?, updated_at=datetime('now','localtime')
         WHERE id=?",
    )
    .bind(retry_count + 1)
    .bind(id)
    .execute(db)
    .await?;
    event_log::log_bg(db.clone(), "warn", "queue", "推送失敗",
        format!("回報推送失敗 queue_id={id} retry={}", retry_count + 1));
    Ok(())
}
