use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use reqwest::Client;
use tauri::{AppHandle, Manager};
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

/// 快取根目錄的識別 marker 檔名。
///
/// `clean_expired` 與 `clear_all_files` 會對快取根做**無差別遞迴刪檔**(需涵蓋 @repeat / @error
/// 等不在 cache_meta 的孤兒檔),等於「這個資料夾整個屬於本 App」的強假設。兩道防線:
/// 1. **初始化前拒絕受保護資料夾**([`assert_not_protected_dir`]):~/Pictures、~/Documents 等
///    系統使用者資料夾**根本不允許被設成快取根**(new 退回 app_data、apply_config 回錯),
///    marker 不會被蓋章 —— 這是主要防護,擋掉「誤設後被蓋章、清理照樣通過」的繞過。
/// 2. **marker 憑證**:清理只對「有 marker(= 本 App 初始化過)」的目錄執行;手改設定檔
///    指到任意舊目錄、或狀態異常時,缺 marker 即拒絕遞迴刪除(fail-safe 第二道)。
pub const CACHE_MARKER: &str = ".cix3752i-cache";

/// 檢查目錄是否為系統使用者資料夾(圖片 / 文件 / 下載 / 桌面 / 影音 / 家目錄**根**)。
/// 這些資料夾被設成快取根 = 個人檔案納入清理刪除範圍 → 一律拒絕。
/// 其「子資料夾」(如 ~/Pictures/cix3752iLabelPrint)不在此限。
/// 比對用 canonicalize(取不到時退回原路徑)吸收大小寫 / symlink / 尾斜線差異。
fn assert_not_protected_dir(handle: &AppHandle, dir: &Path) -> AppResult<()> {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let target = canon(dir);
    // 磁碟根目錄(D:\、/)一律拒絕:整槽都會被清理器納入遞迴刪除範圍,
    // Windows 現場「把快取設到資料槽根」是常見習慣,必須明確擋下
    if target.parent().is_none() {
        return Err(AppError::Config(format!(
            "快取目錄不可設為磁碟根目錄({}):快取清理會遞迴刪除該目錄下的檔案,\
             整個磁碟的資料都會被當過期快取刪掉。請改用子資料夾(例如 {}cix3752iLabelPrint)",
            target.display(),
            target.display(),
        )));
    }
    // Unix 掛載點根(/Volumes/DATA、/mnt/data、/media/user/DATA …)同屬「整顆資料碟」:
    // 有 parent、也不在受保護資料夾清單,但設為快取根 = 整碟納入清理刪除範圍。
    // 以 device id 比較偵測:目錄與其 parent 分屬不同檔案系統 → 目錄是掛載點根 → 拒絕。
    // (Windows 槽根已由上方 parent()==None 涵蓋;NTFS 資料夾掛載點極罕見,不在此防護範圍)
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(m), Some(parent)) = (std::fs::metadata(&target), target.parent()) {
            if let Ok(pm) = std::fs::metadata(parent) {
                if m.dev() != pm.dev() {
                    return Err(AppError::Config(format!(
                        "快取目錄不可設為磁碟掛載點根目錄({}):快取清理會遞迴刪除該目錄下的檔案,\
                         整顆資料碟的檔案都會被當過期快取刪掉。請改用其子資料夾(例如 {}/cix3752iLabelPrint)",
                        target.display(),
                        target.display(),
                    )));
                }
            }
        }
    }
    let r = handle.path();
    let protected: [(&str, Option<std::path::PathBuf>); 8] = [
        ("圖片", r.picture_dir().ok()),
        ("文件", r.document_dir().ok()),
        ("下載", r.download_dir().ok()),
        ("桌面", r.desktop_dir().ok()),
        ("音樂", r.audio_dir().ok()),
        ("影片", r.video_dir().ok()),
        ("公用", r.public_dir().ok()),
        ("家目錄", r.home_dir().ok()),
    ];
    for (name, p) in protected {
        if let Some(p) = p {
            if canon(&p) == target {
                return Err(AppError::Config(format!(
                    "快取目錄不可設為系統「{name}」資料夾({}):快取清理會遞迴刪除該目錄下的檔案,\
                     個人檔案會被當過期快取刪掉。請改用其專屬子資料夾(例如 {}/cix3752iLabelPrint)",
                    p.display(),
                    p.display(),
                )));
            }
        }
    }
    Ok(())
}

/// 在快取根寫入 marker(冪等,失敗只 warn —— marker 缺失時清理會被擋下,fail-safe)。
/// 呼叫前必須先通過 [`assert_not_protected_dir`](受保護資料夾不可被蓋章)。
fn ensure_marker(dir: &Path) {
    let marker = dir.join(CACHE_MARKER);
    if marker.exists() {
        return;
    }
    if let Err(e) = std::fs::write(&marker, "cix3752iLabelPrint cache root - do not delete\n") {
        tracing::warn!(?e, dir = %dir.display(), "寫入快取 marker 失敗(清理將被安全擋下)");
    }
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
    /// 面單下載 client。RwLock:apply_config 需跟隨 cloud 設定
    ///(allow_invalid_certs / timeout_secs)重建,否則熱套用後
    /// session API 生效、面單下載卻仍用舊 TLS 行為(自簽環境全數下載失敗)。
    http: RwLock<Client>,
}

/// 依 cloud 設定建面單下載 client(TLS 跟隨 CloudClient 熱套用)。
/// timeout 取 `max(cloud.timeout_secs, 30)`:圖檔下載(數 MB、跨境慢網)需要的餘裕
/// 遠大於 API 呼叫,站點為了讓健康檢查快速失敗而調低 timeout_secs 時,
/// 不可連帶把下載逾時砍短(下限固定 30s)。
fn build_http(config: &AppConfig) -> AppResult<Client> {
    Client::builder()
        .timeout(Duration::from_secs(config.cloud.timeout_secs.max(30)))
        .danger_accept_invalid_certs(config.cloud.allow_invalid_certs)
        .build()
        .map_err(|e| AppError::Other(format!("無法建立快取 HTTP client: {e}")))
}

impl CacheManager {
    /// 驗證設定解析出的快取目錄可用:非受保護使用者資料夾 + 可建立。
    /// 回傳解析後的目錄。供 new / apply_config 與 update_config **儲存前預檢**共用
    ///(預檢失敗即中止整個設定更新,不留「server 已重啟、設定已存、cache 套用失敗」的斷鏈)。
    pub fn validate_dir(handle: &AppHandle, config: &AppConfig) -> AppResult<PathBuf> {
        let dir = config.resolved_cache_dir(handle)?;
        assert_not_protected_dir(handle, &dir)?;
        std::fs::create_dir_all(&dir).map_err(|e| {
            AppError::Config(format!("無法建立快取目錄 {}: {e}", dir.display()))
        })?;
        Ok(dir)
    }

    /// 解析「安全可用」的快取目錄:驗證失敗(受保護資料夾 / 磁碟根 / 建立失敗)時
    /// 退回 `app_data/cache/labels` 安全預設(啟動不可因壞設定整個失敗,記 error 供診斷)。
    /// **CacheManager::new 與 server 的 /images ServeDir 必須共用此函式** —— 各自解析會在
    /// 壞設定下 split-brain:下載寫 app_data、/images 供壞目錄 → 面單全 404,
    /// 甚至把使用者資料夾以 HTTP 曝露給整個區網。
    pub fn resolve_safe_dir(handle: &AppHandle, config: &AppConfig) -> AppResult<PathBuf> {
        match Self::validate_dir(handle, config) {
            Ok(d) => Ok(d),
            Err(e) => {
                tracing::error!(%e, "快取目錄不可用,退回 app_data 安全預設");
                let fallback = handle
                    .path()
                    .app_data_dir()
                    .map_err(|e| AppError::Config(format!("無法取得 app_data 目錄: {e}")))?
                    .join("cache")
                    .join("labels");
                std::fs::create_dir_all(&fallback).map_err(AppError::from)?;
                Ok(fallback)
            }
        }
    }

    pub fn new(handle: &AppHandle, config: &AppConfig, db: DbPool) -> AppResult<Self> {
        let base_dir = Self::resolve_safe_dir(handle, config)?;
        ensure_marker(&base_dir);

        let http = build_http(config)?;

        Ok(Self {
            inner: Arc::new(Inner {
                base_dir: RwLock::new(base_dir),
                keep_days: RwLock::new(config.cache.keep_days),
                max_size_mb: RwLock::new(config.cache.max_size_mb),
                db,
                http: RwLock::new(http),
            }),
        })
    }

    /// 套用新設定(含依 cloud 設定重建下載 client —— 與 CloudClient.apply_config 對稱)。
    /// - **先建 http client**(唯一真正可失敗的步驟),失敗即中止且未寫入任何欄位(無半套用窗)。
    /// - 目錄驗證失敗 → **沿用現有 base_dir、只 warn 不回錯**:cache.dir「未變」時不可因
    ///   legacy 壞設定卡死所有無關設定的儲存(runtime 本就跑在 new() 的安全 fallback 上);
    ///   cache.dir「有變且壞」的情況由 update_config 的預檢在儲存前擋下,不會走到這裡。
    pub fn apply_config(&self, handle: &AppHandle, config: &AppConfig) -> AppResult<()> {
        let http = build_http(config)?;
        match Self::validate_dir(handle, config) {
            Ok(new_dir) => {
                ensure_marker(&new_dir);
                *self.inner.base_dir.write() = new_dir;
            }
            Err(e) => tracing::warn!(%e, "快取目錄不可用,沿用現有目錄(僅套用其餘設定)"),
        }
        *self.inner.keep_days.write() = config.cache.keep_days;
        *self.inner.max_size_mb.write() = config.cache.max_size_mb;
        *self.inner.http.write() = http;
        Ok(())
    }

    pub fn base_dir(&self) -> PathBuf {
        self.inner.base_dir.read().clone()
    }

    /// 清空快取根下所有檔案(遞迴;保留目錄結構與 marker)。
    /// 帶 marker 安全鎖:目錄缺 marker(未經 CacheManager 初始化,可能是誤設的使用者資料夾)
    /// 時**拒絕執行**並回錯誤,讓 UI 明確告知,而非默默清掉非快取檔案。
    pub async fn clear_all_files(&self) -> AppResult<()> {
        let base = self.base_dir();
        if !base.exists() {
            return Ok(());
        }
        if !base.join(CACHE_MARKER).exists() {
            return Err(AppError::Other(format!(
                "快取目錄缺少識別 marker({CACHE_MARKER}),為避免誤刪非快取檔案已拒絕清空;\
                 請確認快取目錄設定是否指向專屬資料夾",
            )));
        }
        let mut stack = vec![base];
        while let Some(dir) = stack.pop() {
            let mut entries = fs::read_dir(&dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                let meta = entry.metadata().await?;
                let path = entry.path();
                if meta.is_dir() {
                    stack.push(path);
                } else if path.file_name().and_then(|n| n.to_str()) != Some(CACHE_MARKER) {
                    let _ = fs::remove_file(&path).await;
                }
            }
        }
        Ok(())
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

    /// 強制重抓:**忽略本地命中與 source_url 比對**,一律重新下載覆寫檔案與 meta。
    /// 供面單預產「強制重跑」用 —— 即使雲端回相同 URL,也重新拉一份(排除快取檔陳舊/毀損)。
    pub async fn refetch(&self, label_key: &str, source_url: &str) -> AppResult<FetchOutcome> {
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

    // clone 出 client 再發請求(Client 內部 Arc,clone 便宜;不跨 await 持鎖)
    let http = inner.http.read().clone();
    let resp = http
        .get(source_url)
        .send()
        .await?
        .error_for_status()?;
    let bytes = resp.bytes().await?;

    // 原子寫入(temp+rename,含寫失敗清檔 + rename 重試):讀取端(/images 服務、浮水印、
    // DirectPrint / 工控機)永遠讀到完整檔,避免截斷讀("unexpected end of file")。
    // 跨日快取清理後重印 / 預產背景重抓 與 前景列印併發時尤其關鍵。詳見 crate::fs_atomic。
    let size = bytes.len() as i64;
    crate::fs_atomic::write_async(&target, &bytes).await?;

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
        // 合法快取整批刪光、容量卻仍超標。
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
    // 安全鎖:無 marker 的目錄不做無差別遞迴刪除(可能是誤設的使用者資料夾,如 ~/Pictures)。
    // 刪錯個人檔案不可回復,寧可不清理;marker 由 CacheManager 初始化時寫入。
    if !base.join(CACHE_MARKER).exists() {
        tracing::warn!(base = %base.display(),
            "快取根缺少 marker({CACHE_MARKER}),跳過過期清理(避免誤刪非快取檔案)");
        return Ok(());
    }
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
            // marker 本身永不刪(刪了下一輪清理就被安全鎖擋下)
            if path.file_name().and_then(|n| n.to_str()) == Some(CACHE_MARKER) {
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
