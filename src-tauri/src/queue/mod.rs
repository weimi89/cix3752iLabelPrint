use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::cloud::CloudClient;
use crate::db::DbPool;
use crate::models::ReportPayload;
use crate::{AppError, AppResult};

/// 工控機回報佇列管理 — 寫入即回 200，背景 worker 推送雲端
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

    /// 寫入一筆回報到佇列；只要寫得進 DB 就立即回 OK 給工控機
    pub async fn enqueue(&self, payload: &ReportPayload) -> AppResult<i64> {
        let json = serde_json::to_string(payload)?;
        let row = sqlx::query(
            "INSERT INTO report_queue (tracking_no, payload_json, status) VALUES (?, ?, 'pending')",
        )
        .bind(&payload.tracking_no)
        .bind(&json)
        .execute(&self.inner.db)
        .await?;
        Ok(row.last_insert_rowid())
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

    /// 啟動背景 worker
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
        "SELECT id, payload_json, retry_count
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
        let payload_json: String = row.try_get("payload_json")?;
        let retry_count: i64 = row.try_get("retry_count")?;

        sqlx::query("UPDATE report_queue SET status='sending', updated_at=datetime('now') WHERE id=?")
            .bind(id)
            .execute(&inner.db)
            .await?;

        let payload: ReportPayload = match serde_json::from_str(&payload_json) {
            Ok(p) => p,
            Err(e) => {
                mark_failed(&inner.db, id, &format!("payload parse error: {e}"), retry_count).await?;
                continue;
            }
        };

        match inner.cloud.push_report(&payload).await {
            Ok(()) => {
                sqlx::query(
                    "UPDATE report_queue
                     SET status='success', sent_at=datetime('now'), updated_at=datetime('now')
                     WHERE id=?",
                )
                .bind(id)
                .execute(&inner.db)
                .await?;
            }
            Err(AppError::Unauthorized) => {
                // 未登入：別累加 retry，下一輪繼續
                sqlx::query(
                    "UPDATE report_queue
                     SET status='pending', last_error='unauthorized', updated_at=datetime('now')
                     WHERE id=?",
                )
                .bind(id)
                .execute(&inner.db)
                .await?;
            }
            Err(e) => {
                mark_failed(&inner.db, id, &e.to_string(), retry_count).await?;
            }
        }
    }
    Ok(())
}

async fn mark_failed(db: &DbPool, id: i64, err: &str, retry_count: i64) -> AppResult<()> {
    sqlx::query(
        "UPDATE report_queue
         SET status='failed', retry_count=?, last_error=?, updated_at=datetime('now')
         WHERE id=?",
    )
    .bind(retry_count + 1)
    .bind(err)
    .bind(id)
    .execute(db)
    .await?;
    Ok(())
}
