use std::path::PathBuf;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Pool, Sqlite};
use tauri::{AppHandle, Manager};
use tokio::fs;

use crate::{AppError, AppResult};

pub type DbPool = Pool<Sqlite>;

/// 開啟 SQLite 連線池並執行 migration
pub async fn init(handle: &AppHandle) -> AppResult<DbPool> {
    let db_path = db_path(handle)?;

    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent).await?;
    }

    let options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);

    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    tracing::info!(path = %db_path.display(), "SQLite 初始化完成");
    Ok(pool)
}

fn db_path(handle: &AppHandle) -> AppResult<PathBuf> {
    let app_data = handle
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Config(format!("無法取得 app_data 目錄: {e}")))?;
    Ok(app_data.join("data").join("app.sqlite"))
}
