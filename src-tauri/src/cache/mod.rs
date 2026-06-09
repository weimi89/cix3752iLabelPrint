use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use reqwest::Client;
use tauri::AppHandle;
use tokio::fs;

use crate::config::AppConfig;
use crate::db::DbPool;
use crate::{AppError, AppResult};

/// 圖片快取管理 — 負責 local 判斷、補下載、清理過期、命中率統計
#[derive(Clone)]
pub struct CacheManager {
    inner: Arc<Inner>,
}

struct Inner {
    base_dir: RwLock<PathBuf>,
    keep_days: RwLock<u32>,
    max_size_mb: RwLock<u64>,
    db: DbPool,
    http: Client,
}

impl CacheManager {
    pub fn new(handle: &AppHandle, config: &AppConfig, db: DbPool) -> AppResult<Self> {
        let base_dir = config.resolved_cache_dir(handle)?;
        std::fs::create_dir_all(&base_dir).map_err(AppError::from)?;

        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .danger_accept_invalid_certs(config.cloud.allow_invalid_certs)
            .build()
            .map_err(|e| AppError::Other(format!("無法建立快取 HTTP client: {e}")))?;

        Ok(Self {
            inner: Arc::new(Inner {
                base_dir: RwLock::new(base_dir),
                keep_days: RwLock::new(config.cache.keep_days),
                max_size_mb: RwLock::new(config.cache.max_size_mb),
                db,
                http,
            }),
        })
    }

    /// 套用新設定
    pub fn apply_config(&self, handle: &AppHandle, config: &AppConfig) -> AppResult<()> {
        let new_dir = config.resolved_cache_dir(handle)?;
        std::fs::create_dir_all(&new_dir)?;
        *self.inner.base_dir.write() = new_dir;
        *self.inner.keep_days.write() = config.cache.keep_days;
        *self.inner.max_size_mb.write() = config.cache.max_size_mb;
        Ok(())
    }

    pub fn base_dir(&self) -> PathBuf {
        self.inner.base_dir.read().clone()
    }

    pub fn local_path_for_key(&self, label_key: &str) -> PathBuf {
        let relative = label_key.trim_start_matches('/');
        self.inner.base_dir.read().join(relative)
    }

    pub fn has_local(&self, label_key: &str) -> bool {
        self.local_path_for_key(label_key).exists()
    }

    /// 記一次 cache hit:更新 cache_meta.hit_count + daily_stats.cache_hit
    pub async fn record_hit(&self, label_key: &str) -> AppResult<()> {
        sqlx::query(
            "UPDATE cache_meta
             SET hit_count = hit_count + 1, last_hit_at = datetime('now','localtime')
             WHERE label_key = ?",
        )
        .bind(label_key)
        .execute(&self.inner.db)
        .await?;

        sqlx::query(
            "INSERT INTO daily_stats (date, cache_hit)
             VALUES (date('now'), 1)
             ON CONFLICT(date) DO UPDATE SET cache_hit = cache_hit + 1",
        )
        .execute(&self.inner.db)
        .await?;
        Ok(())
    }

    /// 記一次 cache miss
    pub async fn record_miss(&self) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO daily_stats (date, cache_miss)
             VALUES (date('now'), 1)
             ON CONFLICT(date) DO UPDATE SET cache_miss = cache_miss + 1",
        )
        .execute(&self.inner.db)
        .await?;
        Ok(())
    }

    /// 同步下載並寫入快取，完成才返回（給 GET /api/parcel 即時取得最新路徑用）
    /// 若檔案已存在則直接返回 Ok(()) 不重抓
    pub async fn fetch_now(&self, label_key: &str, source_url: &str) -> AppResult<()> {
        if self.has_local(label_key) {
            return Ok(());
        }
        download_one(&self.inner, label_key, source_url).await
    }

    /// 啟動背景清理 task(依 keep_days 與 max_size_mb 刪除)
    pub fn start_cleaner(&self) {
        let inner = self.inner.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(3600)).await;
                let (days, max_mb, base) = {
                    (
                        *inner.keep_days.read(),
                        *inner.max_size_mb.read(),
                        inner.base_dir.read().clone(),
                    )
                };
                if let Err(e) = clean_cache(&base, days, max_mb, &inner.db).await {
                    tracing::warn!(?e, "快取清理失敗");
                }
            }
        });
    }

    pub fn db(&self) -> &DbPool {
        &self.inner.db
    }
}

/// 從雲端圖片 URL 推導本地快取相對 key，保留 `labels/` 之後的子資料夾結構
/// 規則：找到 URL 路徑中 `labels/` 出現的位置，回傳其後的相對路徑（去除 query string）
/// 例：
///   https://cdn.../data/labels/HCT/20260513/abc.png?t=1 → "HCT/20260513/abc.png"
///   /data/labels/SFExpress/20260415/xxx.png            → "SFExpress/20260415/xxx.png"
///   labels/foo.png                                      → "foo.png"
///   其他無法解析的 URL → fallback 為 URL 路徑最後一段
pub fn derive_label_key(image_url: &str) -> String {
    let no_query = image_url.split('?').next().unwrap_or(image_url);
    const NEEDLE: &str = "/labels/";
    if let Some(idx) = no_query.find(NEEDLE) {
        return no_query[idx + NEEDLE.len()..].to_string();
    }
    if let Some(rest) = no_query.strip_prefix("labels/") {
        return rest.to_string();
    }
    no_query
        .rsplit('/')
        .next()
        .unwrap_or("unknown.png")
        .to_string()
}

async fn download_one(inner: &Inner, label_key: &str, source_url: &str) -> AppResult<()> {
    let target = {
        let base = inner.base_dir.read().clone();
        base.join(label_key.trim_start_matches('/'))
    };

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).await?;
    }

    let resp = inner
        .http
        .get(source_url)
        .send()
        .await?
        .error_for_status()?;
    let bytes = resp.bytes().await?;

    fs::write(&target, &bytes).await?;
    let size = bytes.len() as i64;

    sqlx::query(
        "INSERT INTO cache_meta (label_key, local_path, source_url, size_bytes)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(label_key) DO UPDATE SET
            local_path = excluded.local_path,
            source_url = excluded.source_url,
            size_bytes = excluded.size_bytes,
            created_at = datetime('now','localtime')",
    )
    .bind(label_key)
    .bind(target.to_string_lossy().to_string())
    .bind(source_url)
    .bind(size)
    .execute(&inner.db)
    .await?;

    tracing::info!(label_key, %source_url, size, "快取補下載完成");
    Ok(())
}

/// 清理快取:先處理 keep_days 過期,再處理 max_size_mb(LRU 依 last_hit_at)
async fn clean_cache(base: &Path, keep_days: u32, max_size_mb: u64, db: &DbPool) -> AppResult<()> {
    // 1. 過期檔
    if keep_days > 0 {
        let threshold = std::time::SystemTime::now()
            - std::time::Duration::from_secs(keep_days as u64 * 86400);
        clean_expired(base, threshold).await?;
        sqlx::query(
            "DELETE FROM cache_meta
             WHERE created_at < datetime('now','localtime', ?)",
        )
        .bind(format!("-{keep_days} days"))
        .execute(db)
        .await?;
    }

    // 2. 容量上限
    if max_size_mb > 0 {
        let limit_bytes = (max_size_mb * 1024 * 1024) as i64;
        let mut total = total_size(base).await?;
        if total > limit_bytes {
            // 依最少 hit + 最舊 last_hit 取出要刪的
            let victims = sqlx::query_as::<_, (String, String, i64)>(
                "SELECT label_key, local_path, size_bytes
                 FROM cache_meta
                 ORDER BY COALESCE(last_hit_at, created_at) ASC",
            )
            .fetch_all(db)
            .await?;

            for (key, path, size) in victims {
                if total <= limit_bytes {
                    break;
                }
                let _ = fs::remove_file(&path).await;
                let _ = sqlx::query("DELETE FROM cache_meta WHERE label_key = ?")
                    .bind(&key)
                    .execute(db)
                    .await;
                total -= size;
            }
        }
    }
    Ok(())
}

async fn clean_expired(base: &Path, threshold: std::time::SystemTime) -> AppResult<()> {
    let mut stack = vec![base.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut entries = match fs::read_dir(&dir).await {
            Ok(e) => e,
            Err(_) => continue,
        };
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

async fn total_size(base: &Path) -> AppResult<i64> {
    let mut total: i64 = 0;
    let mut stack = vec![base.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut entries = match fs::read_dir(&dir).await {
            Ok(e) => e,
            Err(_) => continue,
        };
        while let Some(entry) = entries.next_entry().await? {
            let meta = entry.metadata().await?;
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                total += meta.len() as i64;
            }
        }
    }
    Ok(total)
}
