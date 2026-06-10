use crate::db::DbPool;

/// 非同步背景寫入 event_log，fire-and-forget，不阻塞呼叫方。
/// level: "info" / "warn" / "error"
/// category: "server" / "cloud" / "cache" / "queue" / "printer"
pub fn log_bg(
    db: DbPool,
    level: &'static str,
    category: &'static str,
    action: &'static str,
    message: String,
) {
    tokio::spawn(async move {
        let _ = sqlx::query(
            "INSERT INTO event_log (level, category, action, message, created_at)
             VALUES (?, ?, ?, ?, datetime('now','localtime'))",
        )
        .bind(level)
        .bind(category)
        .bind(action)
        .bind(message)
        .execute(&db)
        .await;
    });
}
