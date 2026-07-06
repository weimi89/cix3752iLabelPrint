//! 件數核對跨機同步 — Reverb(Pusher 相容)WebSocket 訂閱端。
//!
//! 訂閱 `bag.{package_sn}` public 頻道,收到雲端廣播的 `parcel.printed` 事件後,呼叫
//! [`BagCheckState::apply_remote_print`] 就地更新本機核對清單,讓「同一袋包裹拿到別台處理」
//! 也能即時同步(原本舊袋不再回雲端會失真)。
//!
//! 設計:
//!   - **動態訂閱**:每 [`RECONCILE_SECS`] 比對 bag_check 目前持有的袋,訂新的、退掉沒了的。
//!   - **補洞**:每次新訂閱(含重連後重訂)對該袋呼叫 [`BagCheckState::refresh_bag`],
//!     補回 build_entry→訂閱空窗 或 WS 斷線期間漏掉的列印(雲端為真相來源)。
//!   - **自動重連**:指數退避至 [`RECONNECT_MAX_SECS`]。
//!   - public 頻道免認證,只需 app key。
//!
//! 注意:wss 需在啟動時安裝 rustls CryptoProvider(見 `lib.rs`,用 ring 對齊 reqwest/sqlx)。

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use parking_lot::Mutex;
use tauri::{AppHandle, Emitter};
use tokio::task::JoinHandle;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

use crate::bag_check::BagCheckState;
use crate::config::SyncConfig;

/// 清關進度浮動框收到某件已印的事件名(前端 listen 此事件即時遞減剩餘)
pub const CLEARANCE_PROGRESS_PRINTED_EVENT: &str = "clearance-progress-printed";
/// 清關進度浮動框收到「新增件」的事件名(前端 listen 此事件即時累加總數)
pub const CLEARANCE_PROGRESS_ADDED_EVENT: &str = "clearance-progress-added";
/// 清關進度浮動框收到「移除件」的事件名(報關日改掉 → 原日期扣掉總數)
pub const CLEARANCE_PROGRESS_REMOVED_EVENT: &str = "clearance-progress-removed";

/// 多久比對一次「該訂閱哪些袋」
const RECONCILE_SECS: u64 = 2;
/// 重連退避上限
const RECONNECT_MAX_SECS: u64 = 30;
/// 主動送 pusher:ping 的間隔(伺服器會回 pusher:pong,讓 half-open 連線可被偵測)
const PING_SECS: u64 = 30;
/// 讀閒置上限:超過此秒數完全收不到任何訊息(含 pong)→ 視為 half-open,斷線重連。
/// TCP half-open(NAT 逾時 / 系統睡醒 / 換網路)時 `rx.next()` 會永久 pending,
/// 不主動偵測的話同步會靜默死亡且永不重連。
const IDLE_TIMEOUT_SECS: u64 = 90;
/// 等待 pusher:connection_established 握手的上限
const HANDSHAKE_TIMEOUT_SECS: u64 = 15;
/// TCP+TLS 建線(connect_async)上限 —— 防火牆吞 SYN / 路由黑洞時 OS 預設可掛 1-2 分鐘,
/// 不設上限會讓重連迴圈停擺遠超設計的退避窗(half-open 修正的同結構缺口)
const CONNECT_TIMEOUT_SECS: u64 = 15;
/// 連線存活達此秒數才視為「穩定」、把重連退避歸 1。
/// 若握手成功就歸 1,雲端 crash-loop(能握手但數秒內斷)時所有中介機會以 1s 間隔
/// 無限重連打出連線風暴;以存活時間為準,短命連線照樣退避到上限。
const STABLE_RESET_SECS: u64 = 60;
/// 重連(含首連)成功後 emit 給前端的事件:清關進度框收到後重拉基準,
/// 補回斷線窗內遺失的 clearance-date 廣播(bag.* 另有 refresh_bag 補洞,不靠此事件)。
pub const SYNC_CONNECTED_EVENT: &str = "sync-reconnected";

/// 件數核對跨機同步管理器 —— 持有當前連線 task,支援設定頁熱套用(切換 enabled / 換 host 即時生效)。
#[derive(Clone)]
pub struct SyncManager {
    /// 當前連線迴圈的 task handle;停用 / 重設時 abort 之
    task: Arc<Mutex<Option<JoinHandle<()>>>>,
    bag_check: BagCheckState,
    app: AppHandle,
    /// 清關進度浮動框啟用中的報關日(原始字串,reconcile 時正規化成 clearance-date.{YYYYMMDD} 訂閱);
    /// 空集合 = 沒開進度框,不訂任何日期頻道。由 progress_set_dates command 更新,連線 task 每輪讀取。
    active_dates: Arc<Mutex<HashSet<String>>>,
}

impl SyncManager {
    pub fn new(bag_check: BagCheckState, app: AppHandle) -> Self {
        Self {
            task: Arc::new(Mutex::new(None)),
            bag_check,
            app,
            active_dates: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// 設定清關進度浮動框要追蹤的報關日(浮動框開啟/換日期時呼叫;空=關閉、退訂)。
    /// 只更新共享集合,連線 task 下一輪 reconcile(≤2s)自動訂/退對應 clearance-date 頻道。
    pub fn set_active_dates(&self, dates: Vec<String>) {
        let mut set = self.active_dates.lock();
        set.clear();
        for d in dates {
            let d = d.trim().to_string();
            if !d.is_empty() {
                set.insert(d);
            }
        }
    }

    /// 套用(熱)同步設定:先停既有連線 task;`enabled` 且 host / app_key 齊備才以新設定重啟。
    /// 設定頁切換開關 / 改 host 即時生效,無須重啟 App(對齊 CameraManager)。
    pub fn apply_config(&self, cfg: &SyncConfig) {
        let mut guard = self.task.lock();
        if let Some(h) = guard.take() {
            h.abort();
        }
        if !cfg.enabled || cfg.reverb_host.trim().is_empty() || cfg.reverb_app_key.trim().is_empty() {
            tracing::info!(enabled = cfg.enabled, "件數核對同步:未啟用或 host/key 不全,不連線");
            return;
        }
        let cfg = cfg.clone();
        let bag_check = self.bag_check.clone();
        let app = self.app.clone();
        let active_dates = self.active_dates.clone();
        let handle = tokio::spawn(async move {
            let mut backoff = 1u64;
            loop {
                let started = tokio::time::Instant::now();
                if let Err(e) = run_once(&cfg, &bag_check, &active_dates, &app).await {
                    tracing::warn!(error = %e, backoff_secs = backoff, "件數核對同步:連線中斷,稍後重連");
                }
                // 連線存活 ≥ STABLE_RESET_SECS 才把退避歸 1:短暫閃斷後快速重連,
                // 但雲端 crash-loop(握手即斷)時照樣退避到上限,不打連線風暴
                if started.elapsed() >= Duration::from_secs(STABLE_RESET_SECS) {
                    backoff = 1;
                }
                tokio::time::sleep(Duration::from_secs(backoff)).await;
                backoff = (backoff.saturating_mul(2)).min(RECONNECT_MAX_SECS);
            }
        });
        *guard = Some(handle);
    }
}

fn ws_url(c: &SyncConfig) -> String {
    format!(
        "{}://{}:{}/app/{}?protocol=7&client=cix3752i&version=1.0",
        c.reverb_scheme, c.reverb_host, c.reverb_port, c.reverb_app_key
    )
}

fn subscribe_msg(channel: &str) -> String {
    format!("{{\"event\":\"pusher:subscribe\",\"data\":{{\"channel\":\"{channel}\"}}}}")
}

fn unsubscribe_msg(channel: &str) -> String {
    format!("{{\"event\":\"pusher:unsubscribe\",\"data\":{{\"channel\":\"{channel}\"}}}}")
}

/// 一條連線的生命週期:連上(有超時)→ 等 `connection_established`(有超時)→ reconcile 訂閱 + 收事件。
/// 連線斷掉 / 出錯 / 讀閒置逾時回 `Err`(交給外層退避重連);
/// 成功握手後 emit [`SYNC_CONNECTED_EVENT`](供清關進度框重拉基準)。
/// 退避歸一由呼叫端依「連線存活時間」判定(見 [`STABLE_RESET_SECS`])。
async fn run_once(
    config: &SyncConfig,
    bag_check: &BagCheckState,
    active_dates: &Arc<Mutex<HashSet<String>>>,
    app: &AppHandle,
) -> Result<(), String> {
    let url = ws_url(config);
    tracing::info!(host = %config.reverb_host, "件數核對同步:連線 Reverb");
    let (ws, _) = tokio::time::timeout(
        Duration::from_secs(CONNECT_TIMEOUT_SECS),
        connect_async(&url),
    )
    .await
    .map_err(|_| "建線逾時".to_string())?
    .map_err(|e| e.to_string())?;
    let (mut tx, mut rx) = ws.split();

    // 先等 pusher:connection_established,握手完成才開始訂閱。
    // 包整體超時:half-open / 伺服器不回握手時不可永久卡在這裡(卡住=外層重連迴圈停擺)。
    let handshake = async {
        loop {
            match rx.next().await {
                Some(Ok(Message::Text(t))) if t.contains("pusher:connection_established") => {
                    return Ok(());
                }
                Some(Ok(Message::Ping(p))) => {
                    tx.send(Message::Pong(p)).await.map_err(|e| e.to_string())?;
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(e.to_string()),
                None => return Err("握手前連線關閉".to_string()),
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(HANDSHAKE_TIMEOUT_SECS), handshake)
        .await
        .map_err(|_| "握手逾時".to_string())??;
    tracing::info!("件數核對同步:已連上 Reverb");

    // subscribed 存「頻道名」:bag.{package_sn}(件數核對)+ clearance-date.{YYYYMMDD}(清關進度框)
    let mut subscribed: HashSet<String> = HashSet::new();
    let mut reconcile = tokio::time::interval(Duration::from_secs(RECONCILE_SECS));
    // SYNC_CONNECTED_EVENT 延到「首輪 reconcile 訂閱送出後」才 emit:
    // 若握手後立即 emit,前端重拉基準完成、訂閱尚未生效的間隙內落地的廣播仍會漏
    //(與斷線窗同型的遺失);訂閱先送出再重拉,視窗收斂到近零。
    let mut announced = false;
    // 主動 ping + 讀閒置偵測:half-open 時 rx 永久沉默,靠「PING_SECS 送 ping、
    // IDLE_TIMEOUT_SECS 內連 pong 都收不到就斷」讓連線死亡可被發現。
    let mut ping_tick = tokio::time::interval(Duration::from_secs(PING_SECS));
    ping_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_rx = tokio::time::Instant::now();

    loop {
        tokio::select! {
            _ = ping_tick.tick() => {
                if last_rx.elapsed() >= Duration::from_secs(IDLE_TIMEOUT_SECS) {
                    return Err(format!("讀閒置逾 {IDLE_TIMEOUT_SECS}s(疑似 half-open),重連"));
                }
                tx.send(Message::Text("{\"event\":\"pusher:ping\"}".into()))
                    .await.map_err(|e| format!("送 ping 失敗: {e}"))?;
            }
            _ = reconcile.tick() => {
                // 目標頻道集 = 持有袋的 bag 頻道 ∪ 進度框啟用日期的 clearance-date 頻道
                let mut desired: HashSet<String> = bag_check
                    .held_package_sns()
                    .into_iter()
                    .map(|sn| format!("bag.{sn}"))
                    .collect();
                for d in active_dates.lock().iter() {
                    let ymd: String = d.chars().filter(|c| c.is_ascii_digit()).collect();
                    if !ymd.is_empty() {
                        desired.insert(format!("clearance-date.{ymd}"));
                    }
                }

                // 訂閱新增頻道(含重連後重訂);bag.* 新訂閱額外補洞一次
                for ch in desired.difference(&subscribed).cloned().collect::<Vec<_>>() {
                    if tx.send(Message::Text(subscribe_msg(&ch).into())).await.is_err() {
                        return Err("subscribe 送出失敗".into());
                    }
                    tracing::info!(channel = %ch, "件數核對同步:訂閱");
                    subscribed.insert(ch.clone());
                    if let Some(sn) = ch.strip_prefix("bag.") {
                        let bc = bag_check.clone();
                        let sn = sn.to_string();
                        tokio::spawn(async move { bc.refresh_bag(&sn).await; });
                    }
                }

                // 退訂已不再需要的頻道(袋被 prune / 進度框關閉或換日期)
                for ch in subscribed.difference(&desired).cloned().collect::<Vec<_>>() {
                    let _ = tx.send(Message::Text(unsubscribe_msg(&ch).into())).await;
                    subscribed.remove(&ch);
                }

                // 首輪訂閱已送出 → 通知前端重拉基準(清關進度框補洞;tokio interval 首 tick 立即,
                // 故此點距握手成功僅毫秒級)。emit 成功才標 announced:失敗(視窗重建窗等罕見情況)
                // 下輪 reconcile 重試,不讓補洞通知靜默消失
                if !announced && app.emit(SYNC_CONNECTED_EVENT, ()).is_ok() {
                    announced = true;
                }
            }
            msg = rx.next() => {
                let Some(msg) = msg else { return Err("連線關閉".into()); };
                last_rx = tokio::time::Instant::now();
                match msg.map_err(|e| e.to_string())? {
                    Message::Text(text) => {
                        if text.contains("\"pusher:ping\"") {
                            tx.send(Message::Text("{\"event\":\"pusher:pong\"}".into()))
                                .await.map_err(|e| e.to_string())?;
                        } else {
                            apply_event(text.as_str(), bag_check, app);
                        }
                    }
                    Message::Ping(p) => {
                        tx.send(Message::Pong(p)).await.map_err(|e| e.to_string())?;
                    }
                    Message::Close(_) => return Err("收到 Close".into()),
                    _ => {}
                }
            }
        }
    }
}

/// 收到 `parcel.printed` 後依頻道分流:
///   - `bag.*` → 件數核對 apply_remote_print
///   - `clearance-date.*` → emit 給前端清關進度框遞減
/// 同一次列印會在兩個頻道各來一筆(若都訂),分屬不同功能、互不重複。
fn apply_event(text: &str, bag_check: &BagCheckState, app: &AppHandle) {
    // 1) parcel.printed:bag.* → 件數核對;clearance-date.* → 進度框遞減
    if let Some(ev) = parse_parcel_printed(text) {
        if ev.channel.starts_with("clearance-date.") {
            tracing::info!(clearance_date = %ev.clearance_date, shipping_no = %ev.shipping_no, "清關進度:收到 parcel.printed");
            let _ = app.emit(
                CLEARANCE_PROGRESS_PRINTED_EVENT,
                serde_json::json!({
                    "clearance_date": ev.clearance_date,
                    "package_sn": ev.package_sn,
                    "shipping_no": ev.shipping_no,
                    "print_time": ev.print_time,
                }),
            );
        } else {
            tracing::info!(package_sn = %ev.package_sn, shipping_no = %ev.shipping_no, "件數核對同步:收到 parcel.printed");
            bag_check.apply_remote_print(&ev.package_sn, &ev.shipping_no, &ev.print_time);
        }
        return;
    }
    // 2) clearance.parcels-added / -removed(clearance-date.*):進度框累加 / 扣除總數
    if let Some((event, data)) = parse_clearance_event(text) {
        match event.as_str() {
            "clearance.parcels-added" => {
                tracing::info!("清關進度:收到 clearance.parcels-added");
                let _ = app.emit(CLEARANCE_PROGRESS_ADDED_EVENT, data);
            }
            "clearance.parcels-removed" => {
                tracing::info!("清關進度:收到 clearance.parcels-removed");
                let _ = app.emit(CLEARANCE_PROGRESS_REMOVED_EVENT, data);
            }
            _ => {}
        }
    }
}

/// 解析 `clearance.parcels-added` / `clearance.parcels-removed` → (事件名, data)。
/// data(`{clearance_date, parcels:[{shipping_no,package_sn}]}`)為逸出 JSON 字串,需再解析一層。
fn parse_clearance_event(text: &str) -> Option<(String, serde_json::Value)> {
    let frame: serde_json::Value = serde_json::from_str(text).ok()?;
    let event = frame.get("event").and_then(|e| e.as_str())?;
    if event != "clearance.parcels-added" && event != "clearance.parcels-removed" {
        return None;
    }
    let data_str = frame.get("data").and_then(|d| d.as_str())?;
    let data: serde_json::Value = serde_json::from_str(data_str).ok()?;
    Some((event.to_string(), data))
}

/// 解析後的 parcel.printed 事件
struct ParcelEvent {
    channel: String,
    package_sn: String,
    shipping_no: String,
    print_time: String,
    clearance_date: String,
}

/// 從 Reverb 文字幀解析 `parcel.printed`。非此事件、必填(package_sn/shipping_no/print_time)缺漏 → None。
/// Pusher 慣例:外層 `data` 是「逸出後的 JSON 字串」,需再解析一層(易錯點,獨立可測)。`channel` 用於分流。
fn parse_parcel_printed(text: &str) -> Option<ParcelEvent> {
    let frame: serde_json::Value = serde_json::from_str(text).ok()?;
    if frame.get("event").and_then(|e| e.as_str()) != Some("parcel.printed") {
        return None;
    }
    let channel = frame.get("channel").and_then(|c| c.as_str()).unwrap_or("").to_string();
    let data_str = frame.get("data").and_then(|d| d.as_str())?;
    let data: serde_json::Value = serde_json::from_str(data_str).ok()?;
    let pkg = data.get("package_sn").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let sn = data.get("shipping_no").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let pt = data.get("print_time").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let cd = data.get("clearance_date").and_then(|x| x.as_str()).unwrap_or("").to_string();
    if pkg.is_empty() || sn.is_empty() || pt.is_empty() {
        return None;
    }
    Some(ParcelEvent { channel, package_sn: pkg, shipping_no: sn, print_time: pt, clearance_date: cd })
}

#[cfg(test)]
mod tests {
    use super::parse_parcel_printed;

    #[test]
    fn parses_reverb_parcel_printed_frame() {
        // 對齊實測 wire 格式:data 為逸出 JSON 字串;含 channel 供分流
        let frame = r#"{"event":"parcel.printed","data":"{\"package_sn\":\"XYE2620390\",\"shipping_no\":\"SF001\",\"print_time\":\"2026-06-26 21:45:30\",\"print_num\":2,\"shipping_provider\":\"EXPRESS\",\"clearance_date\":\"2026-06-26\"}","channel":"clearance-date.20260626"}"#;
        let ev = parse_parcel_printed(frame).expect("應解析成功");
        assert_eq!(ev.channel, "clearance-date.20260626");
        assert_eq!(ev.package_sn, "XYE2620390");
        assert_eq!(ev.shipping_no, "SF001");
        assert_eq!(ev.print_time, "2026-06-26 21:45:30");
        assert_eq!(ev.clearance_date, "2026-06-26");
    }

    #[test]
    fn ignores_other_events_and_missing_fields() {
        assert!(parse_parcel_printed(r#"{"event":"pusher_internal:subscription_succeeded","data":"{}","channel":"bag.X"}"#).is_none());
        assert!(parse_parcel_printed(r#"{"event":"parcel.printed","data":"{\"package_sn\":\"P\",\"shipping_no\":\"\",\"print_time\":\"t\"}"}"#).is_none());
        assert!(parse_parcel_printed("not json").is_none());
    }

    #[test]
    fn parses_clearance_added_removed_and_preserves_printed_flag() {
        use super::parse_clearance_event;
        // added:data 為逸出 JSON 字串;parcels 含 printed 旗標(報關日變動時已印件不可被當未印計回)
        let added = r#"{"event":"clearance.parcels-added","data":"{\"clearance_date\":\"2026-06-27\",\"parcels\":[{\"shipping_no\":\"SF1\",\"package_sn\":\"XYE1\",\"printed\":true},{\"shipping_no\":\"SF2\",\"package_sn\":\"XYE1\",\"printed\":false}]}","channel":"clearance-date.20260627"}"#;
        let (event, data) = parse_clearance_event(added).expect("added 應解析成功");
        assert_eq!(event, "clearance.parcels-added");
        let parcels = data.get("parcels").and_then(|p| p.as_array()).expect("parcels 陣列");
        assert_eq!(parcels.len(), 2);
        assert_eq!(parcels[0].get("printed").and_then(|b| b.as_bool()), Some(true));
        assert_eq!(parcels[1].get("printed").and_then(|b| b.as_bool()), Some(false));

        // removed 也要能解析
        let removed = r#"{"event":"clearance.parcels-removed","data":"{\"clearance_date\":\"2026-06-26\",\"parcels\":[]}","channel":"clearance-date.20260626"}"#;
        assert_eq!(parse_clearance_event(removed).expect("removed 應解析").0, "clearance.parcels-removed");

        // 非清關事件 / 壞字串 → None
        assert!(parse_clearance_event(r#"{"event":"parcel.printed","data":"{}","channel":"x"}"#).is_none());
        assert!(parse_clearance_event("not json").is_none());
    }
}
