//! 分揀袋件核對 — 常駐記憶體狀態
//!
//! 現場流程:操作員不掃碼,工控機(設備)逐件 `GET /api/parcel` 接收單號。
//! 設備請求有正常回應(雲端會同步 update shipping_print_time)即視為該件已列印。
//!
//! 本模組維護袋件核對常駐清單(進程生命週期內持續,切頁保留):
//!   - 新袋(不在清單)→ 用該件 order_sn 呼叫 examine_package 取整袋應有清單,推入清單
//!   - 舊袋(已在清單)→ 不再請求雲端,就地把對應 shipping_no 那筆的列印時間更新為當下
//! 每次變動 emit `bag-check-updated`(完整快照)推播前端,達成最即時、不輪詢。
//!
//! 保留策略(prune):有未印件(missing > 0)的袋全部保留、永不淘汰(現場需隨時回補列印);
//! 已完成(missing == 0,含載入失敗占位卡)的袋只保留最新一個。只在「新袋插入」時 prune —
//! 避免操作員剛補印完成的舊袋,因清單中已有更新的完成袋而瞬間消失。
//!
//! examine_package 在背景 task 執行,不阻塞工控機 `GET /api/parcel` 的回應
//!(維持「不讓工控機等雲端」原則)。

use std::collections::VecDeque;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::cloud::CloudClient;

/// 袋件核對清單變動事件(payload 為完整快照,前端直接替換)
pub const BAG_CHECK_UPDATED_EVENT: &str = "bag-check-updated";

/// 袋內單筆訂單核對狀態
#[derive(Debug, Clone, Serialize)]
pub struct BagOrder {
    pub shipping_no: String,
    pub order_sn: Option<String>,
    pub shipping_provider: Option<String>,
    /// 列印時間;有值代表設備已成功要過圖(已列印),空 = 缺漏(尚未跑過)
    pub last_print_time: Option<String>,
}

impl BagOrder {
    fn printed(&self) -> bool {
        self.last_print_time
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
    }
}

/// 一袋的核對卡
///
/// 整袋清單載入失敗(散單 / 未登入 / 雲端查詢失敗)時不建立此卡 — 當下那件本身已正常列印,
/// 僅是「整袋交叉核對」這個次要功能取不到清單,不需在 UI 製造一張錯誤卡干擾現場。
#[derive(Debug, Clone, Serialize)]
pub struct BagEntry {
    pub package_sn: String,
    pub orders: Vec<BagOrder>,
    /// 應有件數(整袋清單筆數)
    pub total: usize,
    /// 已列印件數
    pub printed: usize,
    /// 缺漏件數(total - printed)
    pub missing: usize,
    /// 最近一次設備請求此袋的時間
    pub last_request_at: String,
}

impl BagEntry {
    /// 依 orders 重算 total / printed / missing 三個統計欄位
    fn recount(&mut self) {
        self.total = self.orders.len();
        self.printed = self.orders.iter().filter(|o| o.printed()).count();
        self.missing = self.total.saturating_sub(self.printed);
    }
}

#[derive(Clone)]
pub struct BagCheckState {
    inner: Arc<Mutex<VecDeque<BagEntry>>>,
    cloud: CloudClient,
    app: AppHandle,
}

impl BagCheckState {
    pub fn new(cloud: CloudClient, app: AppHandle) -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::new())),
            cloud,
            app,
        }
    }

    /// 當前清單快照(給 command 讀,切頁保留)
    pub fn snapshot(&self) -> Vec<BagEntry> {
        self.inner.lock().iter().cloned().collect()
    }

    /// 清空清單(前端「清除清單」鈕)
    pub fn clear(&self) {
        self.inner.lock().clear();
        self.emit();
    }

    fn emit(&self) {
        let payload = self.snapshot();
        if let Err(e) = self.app.emit(BAG_CHECK_UPDATED_EVENT, payload) {
            tracing::warn!(?e, "emit bag-check-updated 失敗");
        }
    }

    /// 工控機 `GET /api/parcel` 成功後呼叫 — 非阻塞(內部 spawn 背景處理),
    /// package_sn 為空(散單/雲端未回袋號)直接忽略,不納入核對。
    pub fn on_parcel(
        &self,
        package_sn: Option<String>,
        order_sn: &str,
        shipping_no: &str,
        provider: &str,
    ) {
        let package_sn = match package_sn {
            Some(s) if !s.trim().is_empty() => s.trim().to_string(),
            _ => return,
        };
        let this = self.clone();
        let order_sn = order_sn.to_string();
        let shipping_no = shipping_no.to_string();
        let provider = provider.to_string();
        tokio::spawn(async move {
            this.handle(package_sn, order_sn, shipping_no, provider).await;
        });
    }

    async fn handle(
        &self,
        package_sn: String,
        order_sn: String,
        shipping_no: String,
        provider: String,
    ) {
        let printed_at = now_local();

        // 1. 袋已存在 → 就地更新列印時間,不打雲端
        if self.update_existing(&package_sn, &shipping_no, &order_sn, &provider, &printed_at) {
            self.emit();
            return;
        }

        // 2. 新袋 → 在 lock 外呼叫 examine_package 取整袋清單(失敗回 None)
        let entry = self
            .build_entry(&package_sn, &order_sn, &shipping_no, &provider, &printed_at)
            .await;
        // double-check 用的 key:成功用雲端回的 package_sn,失敗則用傳入的
        let key = entry
            .as_ref()
            .map(|e| e.package_sn.clone())
            .unwrap_or_else(|| package_sn.clone());

        // 3. 重新 lock,double-check 防並發期間別的請求已建立同袋
        {
            let mut bags = self.inner.lock();
            if let Some(bag) = bags.iter_mut().find(|b| b.package_sn == key) {
                // 並發期間別的請求已建此袋 → 就地標記已印
                bag.last_request_at = printed_at.clone();
                mark_printed(bag, &shipping_no, &order_sn, &provider, &printed_at);
            } else if let Some(mut entry) = entry {
                // 載入成功才推入清單
                entry.recount();
                bags.push_front(entry);
                prune(&mut bags);
            } else {
                // 載入失敗(散單 / 未登入 / 雲端失敗)且無既有袋 → 不建卡,清單不變
                return;
            }
        }
        self.emit();
    }

    /// 袋已在清單時就地更新;回傳是否命中
    fn update_existing(
        &self,
        package_sn: &str,
        shipping_no: &str,
        order_sn: &str,
        provider: &str,
        printed_at: &str,
    ) -> bool {
        let mut bags = self.inner.lock();
        if let Some(bag) = bags.iter_mut().find(|b| b.package_sn == package_sn) {
            bag.last_request_at = printed_at.to_string();
            mark_printed(bag, shipping_no, order_sn, provider, printed_at);
            true
        } else {
            false
        }
    }

    /// 新袋:examine_package 取整袋清單,組裝 BagEntry。
    /// 散單 / 未登入 / 雲端查詢失敗 → 回 None(不建卡,失敗只記 log 不干擾現場)。
    async fn build_entry(
        &self,
        package_sn: &str,
        order_sn: &str,
        shipping_no: &str,
        _provider: &str,
        printed_at: &str,
    ) -> Option<BagEntry> {
        match self.cloud.examine_package(order_sn).await {
            Ok(res) if res.respond_code == "FIND-PACKAGE-ORDER" => {
                let pkg = res
                    .package_sn
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| package_sn.to_string());
                let orders = res
                    .orders
                    .iter()
                    .map(|o| {
                        let sn = o.shipping_no.clone().unwrap_or_default();
                        // 當下這件設備剛請求成功 → 標記已印;其餘沿用雲端 last_print_time
                        let last_print_time = if sn == shipping_no {
                            Some(printed_at.to_string())
                        } else {
                            o.last_print_time.clone()
                        };
                        BagOrder {
                            shipping_no: sn,
                            order_sn: o.order_sn.clone(),
                            shipping_provider: o.shipping_provider.clone(),
                            last_print_time,
                        }
                    })
                    .collect();
                Some(BagEntry {
                    package_sn: pkg,
                    orders,
                    total: 0,
                    printed: 0,
                    missing: 0,
                    last_request_at: printed_at.to_string(),
                })
            }
            Ok(res) => {
                tracing::debug!(order_sn, code = %res.respond_code, "袋件核對:整袋清單載入失敗,跳過不建卡");
                None
            }
            Err(e) => {
                tracing::debug!(order_sn, error = %e, "袋件核對:examine_package 失敗,跳過不建卡");
                None
            }
        }
    }
}

/// 保留策略:有未印件(missing > 0)的袋全部保留;已完成(missing == 0)的袋只保留最新一個。
/// deque front = 最新、back = 最舊;retain 由 front→back 走訪,
/// 第一個遇到的已完成袋(最新)保留,其餘已完成袋淘汰。
fn prune(bags: &mut VecDeque<BagEntry>) {
    let mut kept_complete = false;
    bags.retain(|b| {
        if b.missing > 0 {
            // 有未印件 → 永遠保留
            true
        } else if !kept_complete {
            // 最新的已完成袋 → 保留(只留這一個)
            kept_complete = true;
            true
        } else {
            // 其餘已完成袋 → 淘汰
            false
        }
    });
}

/// 在袋內標記某 shipping_no 已印;清單查無該件則補一筆,並重算統計
fn mark_printed(
    bag: &mut BagEntry,
    shipping_no: &str,
    order_sn: &str,
    provider: &str,
    printed_at: &str,
) {
    if let Some(ord) = bag.orders.iter_mut().find(|o| o.shipping_no == shipping_no) {
        ord.last_print_time = Some(printed_at.to_string());
    } else {
        bag.orders.push(BagOrder {
            shipping_no: shipping_no.to_string(),
            order_sn: Some(order_sn.to_string()),
            shipping_provider: Some(provider.to_string()),
            last_print_time: Some(printed_at.to_string()),
        });
    }
    bag.recount();
}

/// 本機時區當下時間,格式對齊雲端 last_print_time("Y-m-d H:i:s")
fn now_local() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 建測試袋:missing 決定是否「有未印件」;sn 當 package_sn 方便斷言保留結果
    fn bag(sn: &str, missing: usize) -> BagEntry {
        BagEntry {
            package_sn: sn.to_string(),
            orders: vec![],
            total: 0,
            printed: 0,
            missing,
            last_request_at: String::new(),
        }
    }

    fn codes(bags: &VecDeque<BagEntry>) -> Vec<String> {
        bags.iter().map(|b| b.package_sn.clone()).collect()
    }

    #[test]
    fn keeps_all_incomplete_bags() {
        // 全部都有未印件 → 一個都不淘汰(即使超過舊上限 3)
        let mut bags: VecDeque<BagEntry> =
            ["e", "d", "c", "b", "a"].iter().map(|s| bag(s, 1)).collect();
        prune(&mut bags);
        assert_eq!(codes(&bags), ["e", "d", "c", "b", "a"]);
    }

    #[test]
    fn keeps_only_newest_complete_bag() {
        // front=最新。三個已完成袋 → 只留最新(front-most)那個
        let mut bags: VecDeque<BagEntry> =
            ["c", "b", "a"].iter().map(|s| bag(s, 0)).collect();
        prune(&mut bags);
        assert_eq!(codes(&bags), ["c"]);
    }

    #[test]
    fn mixed_keeps_incomplete_plus_one_complete() {
        // 混合:未印件袋全留,已完成只留最新一個(此處 d 為最新的已完成袋,a 被淘汰)
        let mut bags: VecDeque<BagEntry> = VecDeque::from(vec![
            bag("e", 2), // 未印 → 留
            bag("d", 0), // 已完成(最新)→ 留
            bag("c", 1), // 未印 → 留
            bag("b", 3), // 未印 → 留
            bag("a", 0), // 已完成(較舊)→ 淘汰
        ]);
        prune(&mut bags);
        assert_eq!(codes(&bags), ["e", "d", "c", "b"]);
    }
}
