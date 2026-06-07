//! 分揀袋件核對 — 常駐記憶體狀態
//!
//! 現場流程:操作員不掃碼,工控機(設備)逐件 `GET /api/parcel` 接收單號。
//! 設備請求有正常回應(雲端會同步 update shipping_print_time)即視為該件已列印。
//!
//! 本模組維護「最近處理的 N 袋」常駐清單(進程生命週期內持續,切頁保留):
//!   - 新袋(不在清單)→ 用該件 order_sn 呼叫 examine_package 取整袋應有清單,推入清單
//!   - 舊袋(已在清單)→ 不再請求雲端,就地把對應 shipping_no 那筆的列印時間更新為當下
//! 每次變動 emit `bag-check-updated`(完整快照)推播前端,達成最即時、不輪詢。
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

/// 預設保留最近袋數
const DEFAULT_LIMIT: usize = 3;

/// 袋卡載入狀態
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BagStatus {
    /// 整袋清單已由 examine_package 載入
    Ok,
    /// examine 失敗 / 散單(NO-PACKAGE-DATA) / 未登入,整袋清單無法載入
    LoadFailed,
}

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
#[derive(Debug, Clone, Serialize)]
pub struct BagEntry {
    pub package_sn: String,
    pub status: BagStatus,
    /// 載入失敗時的原因說明(誠實顯示,不靜默吞)
    pub message: Option<String>,
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
    limit: usize,
}

impl BagCheckState {
    pub fn new(cloud: CloudClient, app: AppHandle) -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::new())),
            cloud,
            app,
            limit: DEFAULT_LIMIT,
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

        // 2. 新袋 → 在 lock 外呼叫 examine_package 取整袋清單
        let mut entry = self
            .build_entry(&package_sn, &order_sn, &shipping_no, &provider, &printed_at)
            .await;
        entry.recount();

        // 3. 重新 lock,double-check 防並發期間別的請求已建立同袋
        {
            let mut bags = self.inner.lock();
            if let Some(bag) = bags.iter_mut().find(|b| b.package_sn == entry.package_sn) {
                bag.last_request_at = printed_at.clone();
                mark_printed(bag, &shipping_no, &order_sn, &provider, &printed_at);
            } else {
                bags.push_front(entry);
                while bags.len() > self.limit {
                    bags.pop_back();
                }
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

    /// 新袋:examine_package 取整袋清單,組裝 BagEntry(失敗則回 LoadFailed 占位卡)
    async fn build_entry(
        &self,
        package_sn: &str,
        order_sn: &str,
        shipping_no: &str,
        provider: &str,
        printed_at: &str,
    ) -> BagEntry {
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
                BagEntry {
                    package_sn: pkg,
                    status: BagStatus::Ok,
                    message: None,
                    orders,
                    total: 0,
                    printed: 0,
                    missing: 0,
                    last_request_at: printed_at.to_string(),
                }
            }
            Ok(res) => failed_entry(
                package_sn,
                order_sn,
                shipping_no,
                provider,
                printed_at,
                res.respond_message.unwrap_or(res.respond_code),
            ),
            Err(e) => failed_entry(
                package_sn,
                order_sn,
                shipping_no,
                provider,
                printed_at,
                e.to_string(),
            ),
        }
    }
}

/// 在袋內標記某 shipping_no 已印;清單查無該件則補一筆(僅 Ok 狀態的袋),並重算統計
fn mark_printed(
    bag: &mut BagEntry,
    shipping_no: &str,
    order_sn: &str,
    provider: &str,
    printed_at: &str,
) {
    if let Some(ord) = bag.orders.iter_mut().find(|o| o.shipping_no == shipping_no) {
        ord.last_print_time = Some(printed_at.to_string());
    } else if bag.status == BagStatus::Ok {
        bag.orders.push(BagOrder {
            shipping_no: shipping_no.to_string(),
            order_sn: Some(order_sn.to_string()),
            shipping_provider: Some(provider.to_string()),
            last_print_time: Some(printed_at.to_string()),
        });
    }
    bag.recount();
}

/// 整袋清單載入失敗的占位卡(至少帶當下這件,誠實標示原因)
fn failed_entry(
    package_sn: &str,
    order_sn: &str,
    shipping_no: &str,
    provider: &str,
    printed_at: &str,
    message: String,
) -> BagEntry {
    BagEntry {
        package_sn: package_sn.to_string(),
        status: BagStatus::LoadFailed,
        message: Some(message),
        orders: vec![BagOrder {
            shipping_no: shipping_no.to_string(),
            order_sn: Some(order_sn.to_string()),
            shipping_provider: Some(provider.to_string()),
            last_print_time: Some(printed_at.to_string()),
        }],
        total: 0,
        printed: 0,
        missing: 0,
        last_request_at: printed_at.to_string(),
    }
}

/// 本機時區當下時間,格式對齊雲端 last_print_time("Y-m-d H:i:s")
fn now_local() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}
