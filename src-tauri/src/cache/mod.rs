use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::RwLock;
use tauri::AppHandle;
use tokio::fs;

use crate::config::AppConfig;
use crate::db::DbPool;
use crate::{AppError, AppResult};

/// 圖片快取管理 — 負責 local 判斷、補下載、清理過期
#[derive(Clone)]
pub struct CacheManager {
    inner: Arc<Inner>,
}

struct Inner {
    base_dir: RwLock<PathBuf>,
    keep_days: RwLock<u32>,
    background_prefetch: RwLock<bool>,
    db: DbPool,
}

impl CacheManager {
    pub fn new(handle: &AppHandle, config: &AppConfig, db: DbPool) -> AppResult<Self> {
        let base_dir = config.resolved_cache_dir(handle)?;
        std::fs::create_dir_all(&base_dir).map_err(AppError::from)?;

        Ok(Self {
            inner: Arc::new(Inner {
                base_dir: RwLock::new(base_dir),
                keep_days: RwLock::new(config.cache.keep_days),
                background_prefetch: RwLock::new(config.cache.background_prefetch),
                db,
            }),
        })
    }

    /// 套用新設定
    pub fn apply_config(&self, handle: &AppHandle, config: &AppConfig) -> AppResult<()> {
        let new_dir = config.resolved_cache_dir(handle)?;
        std::fs::create_dir_all(&new_dir)?;
        *self.inner.base_dir.write() = new_dir;
        *self.inner.keep_days.write() = config.cache.keep_days;
        *self.inner.background_prefetch.write() = config.cache.background_prefetch;
        Ok(())
    }

    /// 取得快取根目錄
    pub fn base_dir(&self) -> PathBuf {
        self.inner.base_dir.read().clone()
    }

    /// 用 label_key（雲端回傳的相對 key，如 `labels/2026/05/SF123.png`）算本地路徑
    pub fn local_path_for_key(&self, label_key: &str) -> PathBuf {
        let relative = label_key.trim_start_matches('/');
        self.inner.base_dir.read().join(relative)
    }

    /// 檢查 label 是否已快取
    pub fn has_local(&self, label_key: &str) -> bool {
        self.local_path_for_key(label_key).exists()
    }

    /// 啟動背景清理 task（依 keep_days 刪除過期檔）
    pub fn start_cleaner(&self) {
        let inner = self.inner.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                let (days, base) = {
                    let d = *inner.keep_days.read();
                    let b = inner.base_dir.read().clone();
                    (d, b)
                };
                if days == 0 {
                    continue;
                }
                if let Err(e) = clean_expired(&base, days).await {
                    tracing::warn!(?e, "快取清理失敗");
                }
            }
        });
    }

    pub fn db(&self) -> &DbPool {
        &self.inner.db
    }
}

async fn clean_expired(base: &Path, keep_days: u32) -> AppResult<()> {
    let threshold = std::time::SystemTime::now()
        - std::time::Duration::from_secs(keep_days as u64 * 86400);

    let mut stack = vec![base.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut entries = fs::read_dir(&dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let meta = entry.metadata().await?;
            let path = entry.path();
            if meta.is_dir() {
                stack.push(path);
                continue;
            }
            if let Ok(modified) = meta.modified() {
                if modified < threshold {
                    let _ = fs::remove_file(&path).await;
                }
            }
        }
    }
    Ok(())
}
