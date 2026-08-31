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

use std::collections::{HashSet, VecDeque};
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
    /// 連續性異常:此袋還缺件(missing>0)時,中途就出現「別的袋號」→ 被打斷,標記 true。
    /// 現場常見「A袋沒印完先跳去B袋、稍後再回頭補印A」,故只在仍缺件時有意義;
    /// 一旦補齊(missing 歸 0)recount 會自動清除 → 對齊「最終補齊就算正確」。
    pub interrupted: bool,
    /// 最近一次設備請求此袋的時間
    pub last_request_at: String,
}

impl BagEntry {
    /// 依 orders 重算 total / printed / missing 三個統計欄位;補齊(missing=0)時清除 interrupted
    fn recount(&mut self) {
        self.total = self.orders.len();
        self.printed = self.orders.iter().filter(|o| o.printed()).count();
        self.missing = self.total.saturating_sub(self.printed);
        // 補齊即視為正確:清除中途被打斷旗標(回補後自動轉回正常)
        if self.missing == 0 {
            self.interrupted = false;
        }
    }
}

/// `recently_completed` 保留上限:記住最近被 prune 淘汰的「已完成袋號」,供辨識「回補已被淘汰的完成袋」。
/// 有界環形,滿了淘汰最舊,避免長班無限增長。
const RECENTLY_COMPLETED_CAP: usize = 128;

/// 袋件核對常駐狀態:袋清單 + 目前處理中的袋號,由單一鎖保護
/// (清單變動與連續性判定必須原子一致,避免並發下 active_bag 與 bags 不同步)。
#[derive(Default)]
struct BagCheckInner {
    /// 袋件核對清單(front=最新)
    bags: VecDeque<BagEntry>,
    /// 目前處理中的袋號(連續性判定用);None=尚未開始 / 已清空。
    /// 只有「成功查得有袋號」的請求會更新它;NoRead / 雲端錯誤 / 散單不動它 = 連續不中斷。
    active_bag: Option<String>,
    /// 「被切走且 entry 尚未建立」的袋號集合:切走前一袋時,若其 entry 還沒進 bags
    /// (背景 examine_package 雲端往返未完成),記入此集合,待其 entry 建立時再補標 interrupted。
    /// **只在「entry 尚未建立」時放入**(entry 已在 bags 者當下即可判定,無需延後)→ 集合恆為極小;
    /// 補標完成 / 重新成為 active / clear 時移除,不會無限增長。
    abandoned: HashSet<String>,
    /// 最近被 prune 淘汰的「已完成」袋號(有界環形,上限 [`RECENTLY_COMPLETED_CAP`])。
    /// 用於辨識「回補已被淘汰的完成袋」:此類袋不在 bags 內,單看 bags 會誤判成開新袋而打斷進行中的袋。
    recently_completed: VecDeque<String>,
}

#[derive(Clone)]
pub struct BagCheckState {
    inner: Arc<Mutex<BagCheckInner>>,
    cloud: CloudClient,
    app: AppHandle,
}

impl BagCheckState {
    pub fn new(cloud: CloudClient, app: AppHandle) -> Self {
        Self {
            inner: Arc::new(Mutex::new(BagCheckInner::default())),
            cloud,
            app,
        }
    }

    /// 當前清單快照(給 command 讀,切頁保留)
    pub fn snapshot(&self) -> Vec<BagEntry> {
        self.inner.lock().bags.iter().cloned().collect()
    }

    /// 清空清單(前端「清除清單」鈕):連 active_bag 一併重置,避免清空後殘留的舊袋號誤判打斷
    pub fn clear(&self) {
        {
            let mut g = self.inner.lock();
            g.bags.clear();
            g.active_bag = None;
            g.abandoned.clear();
            g.recently_completed.clear();
        }
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
        // 無袋號(散單)不納入袋核對:對齊雲端 !empty() 慣例,空字串 / "0" 皆視為散單。
        // 真實袋號可能以 0 開頭(如 0STTJX9B1694),故只排除「剛好等於 0」者。
        let package_sn = match package_sn {
            Some(s) if !s.trim().is_empty() && s.trim() != "0" => s.trim().to_string(),
            _ => return,
        };
        // 連續性判定必須在「請求順序」內完成 → 同步處理(只鎖記憶體、不 await),
        // 再把耗時的整袋 examine / 標記列印時間丟背景。
        self.note_active_bag(&package_sn);
        let this = self.clone();
        let order_sn = order_sn.to_string();
        let shipping_no = shipping_no.to_string();
        let provider = provider.to_string();
        tokio::spawn(async move {
            this.handle(package_sn, order_sn, shipping_no, provider).await;
        });
    }

    /// 記錄「目前處理中的袋」並偵測連續性 —— 成功查得有袋號時同步呼叫(請求順序內、不阻塞)。
    /// 換到不同袋號、且前一袋仍缺件(missing>0)→ 標前一袋 interrupted(中途被打斷),即時 emit。
    /// NoRead / 雲端錯誤 / 散單不會走到這裡 → active_bag 不變 = 連續不中斷
    /// (對齊「沒有袋號算也含在連續次數內」)。回補後該袋 missing 補到 0 時 recount 會自動清旗標。
    fn note_active_bag(&self, package_sn: &str) {
        let changed = {
            let mut g = self.inner.lock();
            apply_active_switch(&mut g, package_sn)
        };
        if changed {
            self.emit();
        }
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
            let mut g = self.inner.lock();
            if let Some(bag) = g.bags.iter_mut().find(|b| b.package_sn == key) {
                // 並發期間別的請求已建此袋 → 就地標記已印
                bag.last_request_at = printed_at.clone();
                mark_printed(bag, &shipping_no, &order_sn, &provider, &printed_at);
            } else if let Some(mut entry) = entry {
                // 載入成功才推入清單。
                // 此袋號若先前在 recently_completed(完成後被淘汰),現在以新 entry 回到清單 →
                // 必須移出 recently_completed,否則同一袋號被重用為新的未完成袋時會被永久
                // 當成「回補已完成袋」,連續性偵測漏掉它。
                entry.recount();
                g.recently_completed.retain(|s| s != &key);
                g.bags.push_front(entry);
                prune(&mut g);
            } else {
                // 載入失敗(散單 / 未登入 / 雲端失敗)且無既有袋 → 不建卡,清單不變。
                // 此袋不會有卡,延後補標已無意義 → 移出 abandoned,確保集合有界(不因反覆失敗而累積)。
                g.abandoned.remove(&key);
                return;
            }
            // 此袋 entry 剛建立/更新 → 若它先前「被切走」時 entry 尚未建立而漏標,現在補標。
            apply_deferred_interrupt(&mut g, &key);
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
        let mut g = self.inner.lock();
        if let Some(bag) = g.bags.iter_mut().find(|b| b.package_sn == package_sn) {
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
                let orders: Vec<BagOrder> = res
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
                let unprinted: Vec<String> = orders
                    .iter()
                    .filter(|o| o.last_print_time.as_deref().map(|s| s.trim().is_empty()).unwrap_or(true))
                    .map(|o| o.shipping_no.clone())
                    .collect();
                tracing::info!(package_sn = %pkg, total = orders.len(), unprinted = ?unprinted,
                    "件數核對:新袋載入");
                Some(BagEntry {
                    package_sn: pkg,
                    orders,
                    total: 0,
                    printed: 0,
                    missing: 0,
                    interrupted: false,
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

    /// 目前持有的袋號集合 —— 供跨機同步模組決定要訂閱哪些 `bag.{package_sn}` 頻道。
    pub fn held_package_sns(&self) -> Vec<String> {
        self.inner.lock().bags.iter().map(|b| b.package_sn.clone()).collect()
    }

    /// 套用遠端(別台經雲端廣播)的「某件已印」事件。
    /// **僅在本機記憶體已持有該袋時**才更新,不從遠端事件建袋(否則每台都會冒出全部袋)。
    /// 時間字串同格式("Y-m-d H:i:s")可字典序比較;只有實際造成變動才 emit。
    pub fn apply_remote_print(&self, package_sn: &str, shipping_no: &str, print_time: &str) {
        let changed = {
            let mut g = self.inner.lock();
            apply_remote_to(&mut g.bags, package_sn, shipping_no, print_time)
        };
        if changed {
            tracing::info!(package_sn, shipping_no, print_time, "件數核對:套用遠端列印(跨機同步)");
            self.emit();
        }
    }

    /// 重連 / 新訂閱後對指定袋重抓雲端 manifest 補洞 —— WS 斷線期間漏掉的列印,靠雲端真相補回。
    /// 僅在本機已持有此袋時動作;以雲端 `last_print_time` 為準取較新者(不會把已印變未印)。
    /// `examine_package` 接受袋號(package_sn)。注意:`await` 在鎖外,避免跨 await 持鎖。
    pub async fn refresh_bag(&self, package_sn: &str) {
        // examine_package 需要「訂單號 / 單號」而非袋號(用袋號查會回 NO-PACKAGE-DATA),
        // 故從該袋取一個成員單號當查詢鍵。鎖只在 await 前借用。
        let probe = {
            let g = self.inner.lock();
            let Some(bag) = g.bags.iter().find(|b| b.package_sn == package_sn) else {
                return; // 未持有 → 不抓
            };
            bag.orders
                .iter()
                .map(|o| o.shipping_no.clone())
                .find(|s| !s.trim().is_empty())
        };
        let Some(probe) = probe else { return };
        let res = match self.cloud.examine_package(&probe).await {
            Ok(r) if r.respond_code == "FIND-PACKAGE-ORDER" => r,
            Ok(r) => {
                tracing::debug!(package_sn, code = %r.respond_code, "袋核對 refresh:雲端非命中,略過");
                return;
            }
            Err(e) => {
                tracing::debug!(package_sn, error = %e, "袋核對 refresh:examine_package 失敗,略過");
                return;
            }
        };
        let changed = {
            let mut g = self.inner.lock();
            let Some(bag) = g.bags.iter_mut().find(|b| b.package_sn == package_sn) else {
                return;
            };
            let mut any = false;
            for o in &res.orders {
                let sn = o.shipping_no.clone().unwrap_or_default();
                let cloud_t = o.last_print_time.clone().unwrap_or_default();
                if sn.is_empty() || cloud_t.trim().is_empty() {
                    continue;
                }
                if let Some(ord) = bag.orders.iter_mut().find(|x| x.shipping_no == sn) {
                    let cur = ord.last_print_time.clone().unwrap_or_default();
                    if cur.trim().is_empty() || cloud_t.as_str() > cur.as_str() {
                        ord.last_print_time = Some(cloud_t);
                        any = true;
                    }
                }
            }
            if any {
                bag.recount();
            }
            any
        };
        if changed {
            self.emit();
        }
    }
}

/// 保留策略:有未印件(missing > 0)的袋全部保留;已完成(missing == 0)的袋只保留最新一個。
/// deque front = 最新、back = 最舊;retain 由 front→back 走訪,
/// 第一個遇到的已完成袋(最新)保留,其餘已完成袋淘汰。
/// **淘汰的完成袋記入 `recently_completed`**(有界):之後回補這些袋時才認得出它是「已完成袋回補」
/// 而非開新袋,避免誤打斷進行中的袋。
fn prune(inner: &mut BagCheckInner) {
    let mut kept_complete = false;
    let mut evicted: Vec<String> = Vec::new();
    inner.bags.retain(|b| {
        if b.missing > 0 {
            // 有未印件 → 永遠保留
            true
        } else if !kept_complete {
            // 最新的已完成袋 → 保留(只留這一個)
            kept_complete = true;
            true
        } else {
            // 其餘已完成袋 → 淘汰(記住其袋號)
            evicted.push(b.package_sn.clone());
            false
        }
    });
    for sn in evicted {
        remember_completed(inner, sn);
    }
}

/// 記一個「已完成且被淘汰」的袋號到有界環形 `recently_completed`(去重、滿了淘汰最舊)。
fn remember_completed(inner: &mut BagCheckInner, sn: String) {
    if inner.recently_completed.contains(&sn) {
        return;
    }
    if inner.recently_completed.len() >= RECENTLY_COMPLETED_CAP {
        inner.recently_completed.pop_front();
    }
    inner.recently_completed.push_back(sn);
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

/// 套用遠端「某件已印」到清單核心邏輯(供 [`BagCheckState::apply_remote_print`] 與測試共用)。
/// 僅在清單已有該袋、且袋內有該件時更新;只有 last_print_time 由空變有、或新時間較新才算變動。
/// 回傳是否實際造成變動(true 才需 emit)。時間字串同格式可字典序比較。
fn apply_remote_to(
    bags: &mut VecDeque<BagEntry>,
    package_sn: &str,
    shipping_no: &str,
    print_time: &str,
) -> bool {
    let Some(bag) = bags.iter_mut().find(|b| b.package_sn == package_sn) else {
        return false; // 未持有此袋
    };
    let Some(ord) = bag.orders.iter_mut().find(|o| o.shipping_no == shipping_no) else {
        return false; // 袋內查無此件(清單以雲端 manifest 為準)
    };
    let newer = match ord.last_print_time.as_deref() {
        Some(cur) if !cur.trim().is_empty() => print_time > cur,
        _ => true,
    };
    if newer {
        ord.last_print_time = Some(print_time.to_string());
        bag.recount();
    }
    newer
}

/// 連續性判定核心(供 [`BagCheckState::note_active_bag`] 與測試共用)。
/// 由 `active_bag` 換到不同的 `new_bag`、且前一袋仍缺件(missing>0)時 → 標前一袋 interrupted;
/// 前一袋 entry 若尚未建立(背景 examine 未完成),記入 `abandoned` 待其建立時補標
/// (見 [`apply_deferred_interrupt`])。一律把 active 更新為 `new_bag`。回傳是否造成旗標變動。
///
/// **回補已完成袋不算開新袋作業**:若 `new_bag` 已補齊(在清單且 missing==0,**或**先前完成後已被 prune
/// 淘汰,見 `recently_completed`),代表這是「回頭補印已完成的袋」,非開始新袋 → 直接 no-op
/// (不打斷前一袋、不動 active),避免誤標仍在進行的袋為異常。
///
/// NoRead / 雲端錯誤 / 散單不會呼叫此函式,active 不變,故不會打斷連續 —— 這正是「沒袋號含在連續內」。
fn apply_active_switch(inner: &mut BagCheckInner, new_bag: &str) -> bool {
    // 切到「已完成的袋」= 回補列印,非開新袋 → 不影響連續性判定。
    // 含兩種:仍在清單且 missing==0,或已完成後被 prune 淘汰(recently_completed)。
    let reprint_complete = inner
        .bags
        .iter()
        .any(|b| b.package_sn == new_bag && b.missing == 0)
        || inner.recently_completed.iter().any(|s| s == new_bag);
    if reprint_complete {
        return false;
    }

    let switched = matches!(inner.active_bag.as_deref(), Some(a) if a != new_bag);
    let mut changed = false;
    if switched {
        if let Some(prev) = inner.active_bag.clone() {
            match inner.bags.iter_mut().find(|b| b.package_sn == prev) {
                // 前一袋 entry 已建 → 當下即可判定(缺件才標),無需延後
                Some(bag) => {
                    if bag.missing > 0 && !bag.interrupted {
                        bag.interrupted = true;
                        changed = true;
                    }
                }
                // 前一袋 entry 尚未建(背景 examine 未完成)→ 記入 abandoned,待其建立時補標。
                // 只在此情況放入,故集合恆為極小、不會無限增長。
                None => {
                    inner.abandoned.insert(prev);
                }
            }
        }
    }
    // 新袋成為 active → 操作員正在處理它,不再是「被遺棄」狀態
    inner.abandoned.remove(new_bag);
    inner.active_bag = Some(new_bag.to_string());
    changed
}

/// 補標:某袋 entry 剛建立/更新時,若它在 `abandoned` 集合內(先前被切走時 entry 尚未建立而漏標)
/// 且仍缺件 → 補上 interrupted 旗標。**無論是否標記,一律將其移出 `abandoned`**(已解決)→ 集合有界。
/// 回傳是否造成變動。
fn apply_deferred_interrupt(inner: &mut BagCheckInner, package_sn: &str) -> bool {
    if !inner.abandoned.remove(package_sn) {
        return false;
    }
    if let Some(bag) = inner.bags.iter_mut().find(|b| b.package_sn == package_sn) {
        if bag.missing > 0 && !bag.interrupted {
            bag.interrupted = true;
            return true;
        }
    }
    false
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
            interrupted: false,
            last_request_at: String::new(),
        }
    }

    fn codes(bags: &VecDeque<BagEntry>) -> Vec<String> {
        bags.iter().map(|b| b.package_sn.clone()).collect()
    }

    #[test]
    fn keeps_all_incomplete_bags() {
        // 全部都有未印件 → 一個都不淘汰(即使超過舊上限 3)
        let mut inner = inner_with(
            ["e", "d", "c", "b", "a"].iter().map(|s| bag(s, 1)).collect(),
            None,
        );
        prune(&mut inner);
        assert_eq!(codes(&inner.bags), ["e", "d", "c", "b", "a"]);
    }

    #[test]
    fn keeps_only_newest_complete_bag() {
        // front=最新。三個已完成袋 → 只留最新(front-most)那個;被淘汰者(b、a)記入 recently_completed
        let mut inner = inner_with(["c", "b", "a"].iter().map(|s| bag(s, 0)).collect(), None);
        prune(&mut inner);
        assert_eq!(codes(&inner.bags), ["c"]);
        assert!(inner.recently_completed.iter().any(|s| s == "b"));
        assert!(inner.recently_completed.iter().any(|s| s == "a"));
    }

    #[test]
    fn mixed_keeps_incomplete_plus_one_complete() {
        // 混合:未印件袋全留,已完成只留最新一個(此處 d 為最新的已完成袋,a 被淘汰)
        let mut inner = inner_with(
            vec![
                bag("e", 2), // 未印 → 留
                bag("d", 0), // 已完成(最新)→ 留
                bag("c", 1), // 未印 → 留
                bag("b", 3), // 未印 → 留
                bag("a", 0), // 已完成(較舊)→ 淘汰
            ],
            None,
        );
        prune(&mut inner);
        assert_eq!(codes(&inner.bags), ["e", "d", "c", "b"]);
    }

    /// 建一筆訂單(last_print_time 空=未印)
    fn ord(sn: &str, printed_at: Option<&str>) -> BagOrder {
        BagOrder {
            shipping_no: sn.to_string(),
            order_sn: Some(format!("O-{sn}")),
            shipping_provider: Some("EXPRESS".to_string()),
            last_print_time: printed_at.map(|s| s.to_string()),
        }
    }

    /// 建含訂單的袋並先 recount
    fn bag_with(sn: &str, orders: Vec<BagOrder>) -> BagEntry {
        let mut b = BagEntry {
            package_sn: sn.to_string(),
            orders,
            total: 0,
            printed: 0,
            missing: 0,
            interrupted: false,
            last_request_at: String::new(),
        };
        b.recount();
        b
    }

    #[test]
    fn remote_print_marks_held_bag_and_recounts() {
        // 持有袋、件未印 → 套用後標記已印、missing 由 1 變 0、回傳 true
        let mut bags = VecDeque::from(vec![bag_with(
            "P1",
            vec![ord("S1", Some("2026-06-26 10:00:00")), ord("S2", None)],
        )]);
        assert_eq!(bags[0].missing, 1);
        let changed = apply_remote_to(&mut bags, "P1", "S2", "2026-06-26 10:05:00");
        assert!(changed);
        assert_eq!(bags[0].missing, 0);
        assert_eq!(bags[0].printed, 2);
    }

    #[test]
    fn remote_print_ignores_unheld_bag_or_unknown_order() {
        let mut bags = VecDeque::from(vec![bag_with("P1", vec![ord("S1", None)])]);
        // 未持有的袋 → 不變動
        assert!(!apply_remote_to(&mut bags, "PX", "S1", "2026-06-26 10:00:00"));
        // 持有袋但袋內無此件 → 不變動
        assert!(!apply_remote_to(&mut bags, "P1", "S999", "2026-06-26 10:00:00"));
        assert_eq!(bags[0].missing, 1);
    }

    #[test]
    fn remote_print_skips_when_not_newer() {
        // 已印且來件時間較舊/相等 → 不覆蓋、回 false
        let mut bags = VecDeque::from(vec![bag_with("P1", vec![ord("S1", Some("2026-06-26 12:00:00"))])]);
        assert!(!apply_remote_to(&mut bags, "P1", "S1", "2026-06-26 11:00:00")); // 較舊
        assert!(!apply_remote_to(&mut bags, "P1", "S1", "2026-06-26 12:00:00")); // 相等
        assert_eq!(bags[0].orders[0].last_print_time.as_deref(), Some("2026-06-26 12:00:00"));
        // 較新 → 覆蓋
        assert!(apply_remote_to(&mut bags, "P1", "S1", "2026-06-26 13:00:00"));
        assert_eq!(bags[0].orders[0].last_print_time.as_deref(), Some("2026-06-26 13:00:00"));
    }

    // ===== 連續性判定(apply_active_switch / apply_deferred_interrupt)=====

    /// 建測試用 inner(bags + active_bag,abandoned / recently_completed 空)
    fn inner_with(bags: Vec<BagEntry>, active: Option<&str>) -> BagCheckInner {
        BagCheckInner {
            bags: bags.into_iter().collect(),
            active_bag: active.map(|s| s.to_string()),
            abandoned: HashSet::new(),
            recently_completed: VecDeque::new(),
        }
    }
    fn find_bag<'a>(inner: &'a BagCheckInner, sn: &str) -> &'a BagEntry {
        inner.bags.iter().find(|b| b.package_sn == sn).unwrap()
    }

    #[test]
    fn switch_marks_incomplete_previous_bag() {
        // A 還缺件時出現 B → A 被標中途被打斷,active 換成 B
        let mut inner = inner_with(vec![bag("A", 3), bag("B", 5)], Some("A"));
        let changed = apply_active_switch(&mut inner, "B");
        assert!(changed);
        assert!(find_bag(&inner, "A").interrupted);
        assert_eq!(inner.active_bag.as_deref(), Some("B"));
    }

    #[test]
    fn switch_from_complete_bag_not_marked() {
        // A 已補齊(missing=0)時出現 B → 不算異常,不標記,active 換成 B
        let mut inner = inner_with(vec![bag("A", 0), bag("B", 2)], Some("A"));
        let changed = apply_active_switch(&mut inner, "B");
        assert!(!changed);
        assert!(!find_bag(&inner, "A").interrupted);
        assert_eq!(inner.active_bag.as_deref(), Some("B"));
    }

    #[test]
    fn same_bag_never_interrupts() {
        // 同袋連續請求(含中間夾 NoRead=不呼叫本函式)→ active 不變、永不打斷
        let mut inner = inner_with(vec![bag("A", 3)], Some("A"));
        assert!(!apply_active_switch(&mut inner, "A"));
        assert!(!apply_active_switch(&mut inner, "A"));
        assert!(!find_bag(&inner, "A").interrupted);
    }

    #[test]
    fn first_bag_has_no_previous_to_interrupt() {
        // active 尚未建立(None)→ 第一袋不會打斷任何人
        let mut inner = inner_with(vec![bag("A", 3)], None);
        assert!(!apply_active_switch(&mut inner, "A"));
        assert_eq!(inner.active_bag.as_deref(), Some("A"));
        assert!(!find_bag(&inner, "A").interrupted);
    }

    #[test]
    fn interrupt_cleared_after_backfill() {
        // A→B→回補A:A 先被標打斷,補齊後 recount 自動清除旗標(最終補齊就算正確)
        let mut inner = inner_with(
            vec![
                bag_with("A", vec![ord("S1", Some("t")), ord("S2", None)]), // A 缺 1
                bag_with("B", vec![ord("S3", None)]),
            ],
            Some("A"),
        );
        // 跳去 B → A 被標打斷
        assert!(apply_active_switch(&mut inner, "B"));
        let a = inner.bags.iter_mut().find(|b| b.package_sn == "A").unwrap();
        assert!(a.interrupted);
        assert_eq!(a.missing, 1);
        // 回補 A 的 S2 → recount 後 missing=0、interrupted 自動清除
        mark_printed(a, "S2", "O-S2", "EXPRESS", "t2");
        assert_eq!(a.missing, 0);
        assert!(!a.interrupted);
    }

    #[test]
    fn reprint_completed_bag_does_not_interrupt_in_progress() {
        // B 進行中(缺件),回頭補印「已完成的 A」→ 不可誤標 B 中途被打斷
        let mut inner = inner_with(vec![bag("A", 0), bag("B", 3)], Some("B"));
        let changed = apply_active_switch(&mut inner, "A");
        assert!(!changed);
        assert!(!find_bag(&inner, "B").interrupted, "回補已完成袋不該打斷進行中的 B");
        // active 不變(仍是 B),abandoned 不記 B
        assert_eq!(inner.active_bag.as_deref(), Some("B"));
        assert!(inner.abandoned.is_empty());
    }

    #[test]
    fn deferred_interrupt_marks_bag_built_after_switch() {
        // 切走 A 時 A 的 entry 尚未建立(背景 examine 未回)→ 記入 abandoned;
        // 當 A 的 entry 稍後建立時,apply_deferred_interrupt 補標。
        let mut inner = inner_with(vec![bag("B", 5)], Some("A")); // A 尚未進 bags
        let changed = apply_active_switch(&mut inner, "B");
        assert!(!changed, "A 尚未建立,當下無法標記,但應記入 abandoned");
        assert!(inner.abandoned.contains("A"));
        // A 的 entry 建立(缺件)→ 補標成功
        inner.bags.push_front(bag("A", 2));
        assert!(apply_deferred_interrupt(&mut inner, "A"));
        assert!(find_bag(&inner, "A").interrupted);
        // 再次呼叫不重複變動(冪等)
        assert!(!apply_deferred_interrupt(&mut inner, "A"));
    }

    #[test]
    fn abandoned_only_holds_bags_not_yet_built() {
        // 前一袋 entry 已在 bags 時,當下即判定、**不**放入 abandoned
        //(避免完成袋殘留集合後續誤標,也避免無界增長)。
        let mut inner = inner_with(vec![bag("A", 0), bag("B", 3)], Some("A"));
        apply_active_switch(&mut inner, "B"); // A 已完成且在 bags → 不標、不進 abandoned
        assert!(inner.abandoned.is_empty(), "已建 entry 的袋不該進 abandoned");
    }

    #[test]
    fn returning_to_abandoned_bag_clears_it_from_set() {
        // A 的 entry 尚未建立就被切走 → 進 abandoned;操作員回到 A → 移出 abandoned
        let mut inner = inner_with(vec![bag("B", 3)], Some("A")); // A 不在 bags
        apply_active_switch(&mut inner, "B"); // 離開未建的 A → A 進 abandoned
        assert!(inner.abandoned.contains("A"));
        apply_active_switch(&mut inner, "A"); // 回到 A → A 移出 abandoned
        assert!(!inner.abandoned.contains("A"));
    }

    #[test]
    fn reprint_pruned_completed_bag_does_not_interrupt() {
        // 已完成的 A 被 prune 淘汰(不在 bags,但在 recently_completed);
        // 進行中的 B 時回補 A → 不可誤標 B。
        let mut inner = inner_with(vec![bag("B", 3)], Some("B"));
        inner.recently_completed.push_back("A".to_string()); // A 完成後已被淘汰
        let changed = apply_active_switch(&mut inner, "A");
        assert!(!changed);
        assert!(!find_bag(&inner, "B").interrupted, "回補已淘汰的完成袋不該打斷 B");
        assert_eq!(inner.active_bag.as_deref(), Some("B"), "回補不改變 active");
    }

    #[test]
    fn reused_bag_number_no_longer_treated_as_reprint_after_removal() {
        // X 在 recently_completed 時,切到 X 視為回補、no-op(不打斷進行中的 A)
        let mut inner = inner_with(vec![bag("A", 2)], Some("A"));
        inner.recently_completed.push_back("X".to_string());
        assert!(!apply_active_switch(&mut inner, "X"), "X 在 recently_completed → 當回補 no-op");
        assert!(!find_bag(&inner, "A").interrupted);
        // handle 重建 X(未完成)時會移出 recently_completed(見 handle push_front);之後切到 X 就是真正換袋
        inner.recently_completed.retain(|s| s != "X");
        inner.bags.push_front(bag("X", 2));
        assert!(apply_active_switch(&mut inner, "X"));
        assert!(find_bag(&inner, "A").interrupted, "移出 recently_completed 後切到 X 應正常打斷 A");
    }

    #[test]
    fn prune_records_evicted_completed_into_recently_completed() {
        // prune 淘汰的完成袋要進 recently_completed(供之後回補辨識)
        let mut inner = inner_with(vec![bag("newC", 0), bag("oldC", 0), bag("inc", 2)], None);
        prune(&mut inner);
        // newC(最新完成)保留、oldC 被淘汰記入、inc 未印保留
        assert_eq!(codes(&inner.bags), ["newC", "inc"]);
        assert!(inner.recently_completed.iter().any(|s| s == "oldC"));
    }
}
