use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use reqwest::Client;
use tauri::AppHandle;
use tokio::fs;

use crate::config::AppConfig;
use crate::db::DbPool;
use crate::event_log;
use crate::{AppError, AppResult};

/// `fetch_now` 的結果:本地命中且來源一致(Hit)還是重新下載/重抓(Downloaded)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchOutcome {
    /// 本地已有且 `source_url` 一致,直接沿用
    Hit,
    /// 本地無檔、URL 變動或 meta 缺失,實際下載(覆寫)了一次
    Downloaded,
}

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

    /// 同步確保 `label_key` 對應「這次 `source_url`」的最新面單已在本地，完成才返回。
    ///
    /// 命中本地時**必須比對 `cache_meta.source_url`**:面單命中只認 `derive_label_key(URL)`
    /// 推出的 key,若雲端對同一面單路徑更新了內容(重印改地址、補資料重生成卻沿用同 URL),
    /// 只看「檔案存在」會永遠回舊圖 → 找錯(陳舊)面單圖,且永不自我修復。
    /// 因此只有「檔案存在且來源 URL 完全一致」才算 Hit;URL 不符或 meta 缺失一律重抓覆寫。
    pub async fn fetch_now(&self, label_key: &str, source_url: &str) -> AppResult<FetchOutcome> {
        if self.has_local(label_key) {
            let cached_url: Option<String> = sqlx::query_scalar(
                "SELECT source_url FROM cache_meta WHERE label_key = ?",
            )
            .bind(label_key)
            .fetch_optional(&self.inner.db)
            .await
            .ok()
            .flatten();
            if cached_url.as_deref() == Some(source_url) {
                return Ok(FetchOutcome::Hit);
            }
            tracing::info!(
                label_key,
                %source_url,
                cached = ?cached_url,
                "快取來源 URL 變動或 meta 缺失,重抓面單避免回陳舊圖"
            );
        }
        download_one(&self.inner, label_key, source_url).await?;
        Ok(FetchOutcome::Downloaded)
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
    // fallback:URL 不含 `labels/` 段。**保留 host 之後的完整路徑**而非只取最後一段檔名 ——
    // 只取檔名會讓「不同子資料夾、同檔名」的兩張面單推出同一 key 互相命中(找錯面單圖);
    // 保留子路徑才能維持唯一。空字串(雲端回空 shipping_image)退回 `unknown.png` 統一兜底,
    // 配合 fetch_now 的 source_url 校驗,空 URL 永遠下載失敗而不會誤回他單快取。
    let path = match no_query.split_once("://") {
        // 有 scheme:取 host 之後的 path;只有 host 無 path 時回空 → 下方兜底 unknown.png
        Some((_scheme, rest)) => rest.split_once('/').map(|(_host, p)| p).unwrap_or(""),
        None => no_query,
    };
    let trimmed = path.trim_start_matches('/');
    if trimmed.is_empty() {
        "unknown.png".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod label_key_tests {
    use super::derive_label_key;

    /// 正常路徑:保留 `labels/` 之後的完整子路徑(物流商/日期/檔名)→ 跨日不碰撞。
    #[test]
    fn keeps_subpath_after_labels() {
        assert_eq!(
            derive_label_key("https://cdn.x.com/data/labels/HCT/20260513/abc.png?t=1"),
            "HCT/20260513/abc.png"
        );
        assert_eq!(
            derive_label_key("/data/labels/SFExpress/20260415/xxx.png"),
            "SFExpress/20260415/xxx.png"
        );
    }

    /// 回歸:fallback(URL 不含 `labels/`)必須保留子路徑,**不可只取檔名** ——
    /// 否則不同子資料夾、同檔名會推出同一 key 互相命中(找錯面單圖)。
    #[test]
    fn fallback_keeps_full_subpath_not_just_filename() {
        let a = derive_label_key("https://cdn.x.com/foo/20260513/abc.png");
        let b = derive_label_key("https://cdn.x.com/foo/20260614/abc.png");
        assert_ne!(a, b, "fallback 跨資料夾同檔名不可碰撞");
        assert_eq!(a, "foo/20260513/abc.png");
        assert_eq!(b, "foo/20260614/abc.png");
    }

    /// 相對 URL(無 scheme、無 labels/)同樣保留子路徑。
    #[test]
    fn fallback_relative_url_keeps_subpath() {
        assert_eq!(derive_label_key("/cdn/foo/bar/abc.png"), "cdn/foo/bar/abc.png");
    }

    /// 空 / 純斜線 URL 統一兜底成 `unknown.png`(不會推出空字串 key)。
    #[test]
    fn empty_url_falls_back_to_sentinel() {
        assert_eq!(derive_label_key(""), "unknown.png");
        assert_eq!(derive_label_key("https://cdn.x.com/"), "unknown.png");
        assert_eq!(derive_label_key("https://cdn.x.com"), "unknown.png");
    }
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
        "INSERT INTO cache_meta (label_key, local_path, source_url, size_bytes, created_at)
         VALUES (?, ?, ?, ?, datetime('now','localtime'))
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
    event_log::log_bg(inner.db.clone(), "info", "cache", "面單下載",
        format!("面單下載完成 {label_key}"));
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
        // target 以 cache_meta 的 size_bytes 加總為準(= 本流程可淘汰的「受管快取」總量)。
        // 不掃磁碟總量:磁碟另含 @repeat 浮水印 / @error 錯誤面單等非 cache_meta 孤兒檔
        //(由上方 keep_days expiry 依齡清理)。若以磁碟總量為目標,會因孤兒檔扣不到而把
        // 合法快取整批刪光、容量卻仍超標(原 bug)。
        let mut total: i64 = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(SUM(size_bytes), 0) FROM cache_meta",
        )
        .fetch_one(db)
        .await
        .unwrap_or(0);
        if total > limit_bytes {
            // 依最少 hit + 最舊 last_hit 取出要刪的
            let victims = sqlx::query_as::<_, (String, String, i64)>(
                "SELECT label_key, local_path, size_bytes
                 FROM cache_meta
                 ORDER BY COALESCE(last_hit_at, created_at) ASC",
            )
            .fetch_all(db)
            .await?;

            let mut evict_count = 0usize;
            for (key, path, size) in victims {
                if total <= limit_bytes {
                    break;
                }
                if let Err(e) = fs::remove_file(&path).await {
                    // 檔案可能已被 expiry 先刪;非致命,仍續刪 DB 列保持帳實一致
                    tracing::debug!(path = %path, ?e, "刪快取檔失敗(可能已不存在)");
                }
                // 只有 DELETE 成功才扣 total(避免吞錯後帳實不符、下輪又重選同一筆)
                match sqlx::query("DELETE FROM cache_meta WHERE label_key = ?")
                    .bind(&key)
                    .execute(db)
                    .await
                {
                    Ok(_) => {
                        total -= size;
                        evict_count += 1;
                    }
                    Err(e) => {
                        tracing::warn!(label_key = %key, ?e, "刪 cache_meta 失敗,略過此筆");
                    }
                }
            }
            if evict_count > 0 {
                event_log::log_bg(db.clone(), "info", "cache", "快取清理",
                    format!("LRU 清理 {evict_count} 個快取檔案"));
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
