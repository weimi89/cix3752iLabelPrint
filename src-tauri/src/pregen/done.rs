//! 面單預產「今日已預產的 order_sn」共用去重記憶。
//!
//! 過去自動排程(worker 記憶體區域變數)與手動頁(前端 localStorage)各記一份、互不同步 →
//! 自動跑完後手動又全部重打雲端、全報「成功」。此模組把它統一到後端單一來源:
//!   - 記憶體 `HashSet` 提供快查(自動排程 filter / 手動頁 skip)。
//!   - 持久化到 DB `pregen_done` 表,撐過 App 重啟(同一快取日內仍記得)。
//!   - 以「快取日(08:00 為界)」為範圍,跨日自動清空(業務日 08:00~次日 07:59,對齊現場作息)。
//! 自動排程與手動頁(經 command)共用同一份,接續執行時已預產的就真正略過、不重打雲端。

use std::collections::HashSet;

use tokio::sync::Mutex;

use crate::db::DbPool;
use crate::AppResult;

/// 快取日字串:把現在往前推 8 小時讓 08:00 對齊午夜,再取年-月-日。
/// 業務日 08:00~次日 07:59,對齊現場作息(前端去重範圍統一由後端此處決定)。
pub fn current_cache_day() -> String {
    cache_day_offset(0)
}

/// 相對當前快取日位移 `days` 天的快取日字串(days=-1 為前一快取日)。
fn cache_day_offset(days: i64) -> String {
    let shifted =
        chrono::Local::now() - chrono::Duration::hours(8) + chrono::Duration::days(days);
    shifted.format("%Y-%m-%d").to_string()
}

struct Inner {
    /// 目前記憶體集合對應的快取日;與 `current_cache_day()` 不同 → 需重載/清舊
    day: String,
    sns: HashSet<String>,
    /// 是否已從 DB 載入過(首次使用前為 false,強制走一次 ensure)
    loaded: bool,
}

/// 「今日已預產」共用去重儲存。內含 async Mutex,可直接 `Arc` 共享於 AppState。
pub struct PregenDoneStore {
    inner: Mutex<Inner>,
}

impl PregenDoneStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                day: String::new(),
                sns: HashSet::new(),
                loaded: false,
            }),
        }
    }

    /// 確保記憶體集合對應「當前快取日」:首次使用或跨日時,清掉 DB 內非當日列並重載當日列。
    /// 呼叫端須已持有 inner 鎖。
    async fn ensure_current(inner: &mut Inner, db: &DbPool) {
        let today = current_cache_day();
        if inner.loaded && inner.day == today {
            return;
        }

        // 保留「今日 + 昨日」兩個快取日:夜班 00:00–08:00 產出的標記記在前一快取日,
        // 08:00 換日後仍在保留窗內、不被清掉,避免那批訂單被重打雲端並誤報成功
        //(對齊快取檔以天齡清理、非 08:00 硬刪的實情)。只刪早於昨日的列。
        let yesterday = cache_day_offset(-1);
        if let Err(e) = sqlx::query("DELETE FROM pregen_done WHERE cache_day < ?")
            .bind(&yesterday)
            .execute(db)
            .await
        {
            tracing::warn!(?e, "清理過期 pregen_done 失敗");
        }

        // 載入保留窗(今日 + 昨日)已預產集合
        let rows = match sqlx::query_scalar::<_, String>(
            "SELECT order_sn FROM pregen_done WHERE cache_day >= ?",
        )
        .bind(&yesterday)
        .fetch_all(db)
        .await
        {
            Ok(r) => r,
            Err(e) => {
                // 讀取失敗**不可**把空集合當成「今日已載入」快取 —— 否則當日所有訂單被判未預產、
                // 全部重打雲端且一律誤報成功。保持 loaded=false,下次呼叫重試。
                tracing::warn!(?e, "載入 pregen_done 失敗,保持未載入以便下次重試");
                return;
            }
        };

        inner.day = today;
        inner.sns = rows.into_iter().collect();
        inner.loaded = true;
    }

    /// 取當前快取日與已預產 order_sn 快照(供手動頁批次開始時一次取回、在前端記憶體 skip)。
    pub async fn snapshot(&self, db: &DbPool) -> (String, Vec<String>) {
        let mut inner = self.inner.lock().await;
        Self::ensure_current(&mut inner, db).await;
        (inner.day.clone(), inner.sns.iter().cloned().collect())
    }

    /// 此 order_sn 今日是否已預產(供自動排程 filter)。
    pub async fn contains(&self, db: &DbPool, order_sn: &str) -> bool {
        let mut inner = self.inner.lock().await;
        Self::ensure_current(&mut inner, db).await;
        inner.sns.contains(order_sn)
    }

    /// 標記一批 order_sn 為「今日已預產」(記憶體 + DB)。空字串略過。
    pub async fn mark(&self, db: &DbPool, order_sns: &[String]) -> AppResult<()> {
        // 先在鎖內更新記憶體並挑出真正要寫入的新項,隨即**放鎖**;DB 寫入在鎖外做,
        // 避免大批標記持鎖逐筆 await 卡住 snapshot()/contains()(預產頁看似卡死)。
        let (day, to_insert) = {
            let mut inner = self.inner.lock().await;
            Self::ensure_current(&mut inner, db).await;
            let day = inner.day.clone();
            let mut to_insert = Vec::new();
            for sn in order_sns {
                let sn = sn.trim();
                if sn.is_empty() || inner.sns.contains(sn) {
                    continue;
                }
                inner.sns.insert(sn.to_string());
                to_insert.push(sn.to_string());
            }
            (day, to_insert)
        };
        if to_insert.is_empty() {
            return Ok(());
        }
        // 一次交易批次寫入(單一 commit,不逐筆 autocommit)
        let mut tx = db.begin().await?;
        for sn in &to_insert {
            sqlx::query("INSERT OR IGNORE INTO pregen_done (order_sn, cache_day) VALUES (?, ?)")
                .bind(sn)
                .bind(&day)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// 清除「已預產」記憶(記憶體 + DB 全表),讓所有訂單下次都重新抓取。
    /// 供手動頁「清除已預產記錄」鈕與清空後端快取時連帶呼叫。
    pub async fn clear(&self, db: &DbPool) -> AppResult<()> {
        let mut inner = self.inner.lock().await;
        sqlx::query("DELETE FROM pregen_done").execute(db).await?;
        inner.sns.clear();
        inner.day = current_cache_day();
        inner.loaded = true;
        Ok(())
    }
}

impl Default for PregenDoneStore {
    fn default() -> Self {
        Self::new()
    }
}
