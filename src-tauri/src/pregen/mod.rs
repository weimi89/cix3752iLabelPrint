//! 面單預產自動排程 — 常駐背景 task(headless)
//!
//! 每天到設定的時間點(可多個 "HH:MM"),自動反查當日整批訂單編號,
//! 逐筆以 Download 模式預下載面單到本機快取,工控機之後來要直接命中、不用等雲端。
//!
//! 設計重點:
//!   - 不需開啟面單預產頁、不需人在場;設定熱套用(每 tick 重讀 config)即時生效。
//!   - 每日去重:當日已成功預產過的 order_sn 記在記憶體,多時段重跑只抓新出現的單,跨日自動清空。
//!   - 失敗不中斷:單筆失敗只計數,整批結果寫入事件記錄。
//!   - 觀測性:啟動、到點觸發、補跑、完成皆寫入 `pregen` 類別事件記錄;另以記憶體 `PregenStatus`
//!     供前端隨時查詢上次/啟動狀態(`get_pregen_status` command)。
//!   - 補跑:此排程為「進程內」常駐 task,只有 App 開著才會到點觸發。開啟 `catch_up_on_start`
//!     後,App 啟動時會把今日已過、且尚未跑過的時間點補跑一次(以 DB 既有觸發紀錄防重啟重跑)。

use std::collections::HashSet;
use std::time::Duration;

use tokio::task::JoinSet;

use crate::commands::cloud_commands::pregen_label_to_cache;
use crate::{event_log, SharedState};

mod done;
pub use done::PregenDoneStore;

/// 巡檢間隔:每 30s tick 一次,比對當前 HH:MM 是否命中排程時間點
const TICK_SECS: u64 = 30;
/// 預下載並發數(對齊前端 worker 數,避免一次打爆雲端)
const CONCURRENCY: usize = 4;

/// 自動預產「上次執行失敗單號」保留上限(避免大批失敗時記憶體無界成長;
/// 超出僅以 `last_fail` 計數呈現,清單截斷)。
const FAILED_SN_CAP: usize = 200;

/// 自動預產單筆失敗的單號 + 原因(供前端列出、複製、一鍵重跑)。
#[derive(Debug, Clone, serde::Serialize)]
pub struct PregenFailedSn {
    pub sn: String,
    /// 來源(clearance / transfer),多來源時可辨識
    pub source: String,
    /// 失敗原因(pregen_label_to_cache 的錯誤字串)
    pub reason: String,
}

/// 自動預產排程的可觀測狀態(記憶體,供前端 `get_pregen_status` 查詢)。
/// 時間字串一律本地時區 "YYYY-MM-DD HH:MM:SS"。
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct PregenStatus {
    /// 排程 worker 啟動時間(App 開啟即設定一次)
    pub scheduler_started_at: Option<String>,
    /// 上次實際跑預產的完成時間
    pub last_run_at: Option<String>,
    /// 上次觸發來源:命中的時間點 "HH:MM"、或 "catch-up:HH:MM"(啟動補跑)
    pub last_trigger: Option<String>,
    /// 上次彙總結果(跨所有來源加總)
    pub last_ok: u32,
    pub last_fail: u32,
    pub last_skipped: u32,
    pub last_empty: u32,
    /// 上次是否有任何來源反查失敗或單筆失敗
    pub last_had_error: bool,
    /// 上次執行的失敗單號清單(最多 FAILED_SN_CAP 筆;供前端列出 / 複製 / 重跑)
    pub last_failed_sns: Vec<PregenFailedSn>,
}

/// 啟動排程 worker(在 AppState 建立後呼叫,持有 SharedState)
pub fn start_scheduler(state: SharedState) {
    tokio::spawn(async move {
        let now = chrono::Local::now();

        // 記錄啟動時間 + 寫啟動事件(讓「排程是否活著」可被確認)
        {
            let mut st = state.pregen_status.write().await;
            st.scheduler_started_at = Some(now.format("%Y-%m-%d %H:%M:%S").to_string());
        }
        let cfg0 = { state.config.read().await.pre_gen_schedule.clone() };
        event_log::log_bg(
            state.db.clone(),
            "info",
            "pregen",
            "排程啟動",
            format!(
                "自動預產排程已啟動 enabled={} times={:?} sources={:?} date_offset={} catch_up={}",
                cfg0.enabled, cfg0.times, cfg0.sources, cfg0.date_offset_days, cfg0.catch_up_on_start
            ),
        );

        // 當日狀態:done_date 為當前日期字串;切日時清空 fired(排程時間點以「日曆日」計)。
        // 「今日已預產的 order_sn」去重改用共用儲存 state.pregen_done(快取日範圍、與手動頁共用、
        // persist 到 DB),此處不再各記一份。
        let mut done_date = now.format("%Y-%m-%d").to_string();
        let mut fired: HashSet<String> = HashSet::new(); // 今日已觸發的時間點

        // 從 DB 重建今日已觸發的時間點 → 重啟後 catch-up 不會把已跑過的時段重跑
        fired.extend(load_fired_today(&state).await);

        // 啟動補跑:今日已過(嚴格早於現在)、且尚未跑過的時間點,逐一補跑
        if cfg0.enabled && cfg0.catch_up_on_start && !cfg0.times.is_empty() && !cfg0.sources.is_empty()
        {
            let now_hm = now.format("%H:%M").to_string();
            // "HH:MM" 固定寬度全數字 → 字典序即時序
            let mut missed: Vec<String> = cfg0
                .times
                .iter()
                .map(|t| t.trim().to_string())
                .filter(|t| t.as_str() < now_hm.as_str() && !fired.contains(t))
                .collect();
            missed.sort();
            missed.dedup();
            let catchup_date = now.format("%Y-%m-%d").to_string();
            for t in missed {
                // 跨午夜守門(同主迴圈):補跑批次跨過 00:00 後剩餘時段不可再執行 ——
                // run_pregen 以「執行當下」日期+offset 取目標日,過午夜再跑會整批預產錯日訂單
                if chrono::Local::now().format("%Y-%m-%d").to_string() != catchup_date {
                    event_log::log_bg(state.db.clone(), "warn", "pregen", "跨日漏跑",
                        format!("啟動補跑跨過午夜,剩餘時段不執行 time={t}(目標日已過,避免錯日預產)"));
                    continue;
                }
                fired.insert(t.clone());
                event_log::log_bg(
                    state.db.clone(),
                    "info",
                    "pregen",
                    "啟動補跑",
                    format!("補跑今日已過、尚未執行的時間點 time={t}"),
                );
                run_pregen(&state, &cfg0, &format!("catch-up:{t}")).await;
            }
        }

        // 視窗掃描的下界:上輪檢查時刻的 "HH:MM"。每輪處理 [last_hhmm, now] 視窗內全部排程點
        //(fired 防重複觸發)—— run_pregen 為 inline await,批次跑數十分鐘會停 tick;
        // 舊做法用「當下分鐘 == 排程點」精確比對,批次跑過頭該時段永不相等 → 當日靜默漏跑。
        // 視窗掃描讓恢復 tick 後把錯過的時段一次補上。"HH:MM" 固定寬度全數字,字典序即時序。
        let mut last_hhmm = now.format("%H:%M").to_string();

        loop {
            tokio::time::sleep(Duration::from_secs(TICK_SECS)).await;

            let cfg = { state.config.read().await.pre_gen_schedule.clone() };

            let now = chrono::Local::now();
            let today = now.format("%Y-%m-%d").to_string();
            if today != done_date {
                // 跨日:昨日 last_hhmm 之後、尚未觸發的排程點已無法補跑
                //(run_pregen 以「當日」為目標,過了午夜再跑目標日就錯了)——
                // 至少記 warn 讓漏跑**可觀測**,不與修正前一樣靜默消失。
                let missed: Vec<String> = cfg
                    .times
                    .iter()
                    .map(|t| t.trim().to_string())
                    .filter(|t| t.as_str() > last_hhmm.as_str() && !fired.contains(t))
                    .collect();
                for t in missed {
                    event_log::log_bg(state.db.clone(), "warn", "pregen", "跨日漏跑",
                        format!("批次執行跨過午夜,昨日排程時段未觸發 time={t}(目標日已過,不補跑)"));
                }
                done_date = today;
                fired.clear();
                // 跨日:視窗下界歸 00:00,當日排程重新起算
                last_hhmm = "00:00".to_string();
            }
            let now_hhmm = now.format("%H:%M").to_string();

            if !cfg.enabled || cfg.times.is_empty() || cfg.sources.is_empty() {
                // 停用期間流逝的時段不補跑(對齊舊行為):下界跟著推進
                last_hhmm = now_hhmm;
                continue;
            }

            // 視窗內到期且未跑過的時段(含批次執行期間錯過的),依時序逐一觸發
            let mut due: Vec<String> = cfg
                .times
                .iter()
                .map(|t| t.trim().to_string())
                .filter(|t| {
                    last_hhmm.as_str() <= t.as_str()
                        && t.as_str() <= now_hhmm.as_str()
                        && !fired.contains(t)
                })
                .collect();
            due.sort();
            due.dedup();
            last_hhmm = now_hhmm;

            for (i, hhmm) in due.iter().enumerate() {
                // 跨午夜守門:前一時段批次跑過 00:00 後,剩餘 due 時段不可再執行 ——
                // run_pregen 以「執行當下」日期+offset 取目標日,過午夜再跑會整批預產**錯日**訂單
                // (灌錯快取 + pregen_done 標錯批)。剩餘時段**就地**逐一記「跨日漏跑」event_log
                // (不可依賴下個 tick 的 rollover 補記:last_hhmm 已推進到 now_hhmm,
                //  rollover 的 `t > last_hhmm` 過濾對這些時段恆為 false,會靜默無痕)。
                if chrono::Local::now().format("%Y-%m-%d").to_string() != done_date {
                    for miss in &due[i..] {
                        event_log::log_bg(state.db.clone(), "warn", "pregen", "跨日漏跑",
                            format!("批次執行跨過午夜,時段未執行 time={miss}(目標日已過,避免錯日預產)"));
                    }
                    break;
                }
                fired.insert(hhmm.clone());

                // 先寫「到點觸發」— 即使後續整批失敗,也留下「確實有跑」的證據
                event_log::log_bg(
                    state.db.clone(),
                    "info",
                    "pregen",
                    "到點觸發",
                    format!("到點觸發自動預產 time={hhmm}"),
                );

                run_pregen(&state, &cfg, hhmm).await;
            }
        }
    });
}

/// 從 event_log 撈出「今日(本地)」已觸發的時間點 "HH:MM" 集合。
/// 觸發/補跑事件 message 內固定以 `time=HH:MM` 結尾,解析該欄即可。
async fn load_fired_today(state: &SharedState) -> HashSet<String> {
    let rows = sqlx::query_scalar::<_, String>(
        "SELECT message FROM event_log
         WHERE category = 'pregen'
           AND action IN ('到點觸發', '啟動補跑')
           AND date(created_at) = date('now','localtime')",
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    rows.iter()
        .filter_map(|m| m.rsplit("time=").next().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
        .collect()
}

/// 跑一輪預產:對每個來源反查當日訂單 → 預下載快取,結果寫事件記錄並更新狀態。
/// `trigger`:觸發來源描述("HH:MM" 或 "catch-up:HH:MM"),記入 PregenStatus。
/// 去重用 state.pregen_done(與手動頁共用),已預產者跳過、成功者標記。
async fn run_pregen(
    state: &SharedState,
    cfg: &crate::config::PreGenScheduleConfig,
    trigger: &str,
) {
    let date = (chrono::Local::now().date_naive()
        + chrono::Duration::days(cfg.date_offset_days))
    .format("%Y-%m-%d")
    .to_string();

    // 跨來源加總,結束後一次性更新 PregenStatus
    let (mut g_ok, mut g_fail, mut g_skip, mut g_empty) = (0u32, 0u32, 0u32, 0u32);
    let mut had_error = false;
    // 跨來源累積的失敗單號(截斷至 FAILED_SN_CAP;g_fail 仍為完整計數)
    let mut g_failed: Vec<PregenFailedSn> = Vec::new();

    for source in &cfg.sources {
        let res = match state.cloud.fetch_orders_by_date(&date, source).await {
            Ok(r) => r,
            Err(e) => {
                had_error = true;
                event_log::log_bg(
                    state.db.clone(),
                    "warn",
                    "pregen",
                    "自動預產:反查訂單失敗",
                    format!("date={date} source={source} err={e}"),
                );
                continue;
            }
        };

        if res.respond_code != "FIND-PACKAGE-ORDER" || res.order_sns.is_empty() {
            event_log::log_bg(
                state.db.clone(),
                "info",
                "pregen",
                "自動預產:當日無訂單",
                format!(
                    "date={date} source={source} code={} msg={}",
                    res.respond_code,
                    res.respond_message.unwrap_or_default()
                ),
            );
            continue;
        }

        // 去重:略過今日已預產過的單(與手動頁共用 state.pregen_done;多時段/接手動只抓新出現的)
        let total = res.order_sns.len();
        let (day, done_sns) = state.pregen_done.snapshot(&state.db).await;
        let done_set: HashSet<String> = done_sns.into_iter().collect();
        let fresh: Vec<String> = res
            .order_sns
            .into_iter()
            .filter(|sn| !sn.trim().is_empty() && !done_set.contains(sn))
            .collect();
        let skipped = total - fresh.len();

        let (mut ok, mut empty, mut fail) = (0usize, 0usize, 0usize);
        let mut ok_sns: Vec<String> = Vec::new();
        for batch in fresh.chunks(CONCURRENCY) {
            let mut set: JoinSet<(String, crate::AppResult<bool>)> = JoinSet::new();
            for sn in batch {
                let st = state.clone();
                let sn = sn.clone();
                set.spawn(async move {
                    let r = pregen_label_to_cache(&st, &sn).await;
                    (sn, r)
                });
            }
            while let Some(joined) = set.join_next().await {
                if let Ok((sn, r)) = joined {
                    match r {
                        Ok(true) => {
                            ok += 1;
                            ok_sns.push(sn);
                        }
                        Ok(false) => empty += 1,
                        Err(e) => {
                            fail += 1;
                            if g_failed.len() < FAILED_SN_CAP {
                                g_failed.push(PregenFailedSn {
                                    sn,
                                    source: source.clone(),
                                    reason: format!("{e}"),
                                });
                            }
                        }
                    }
                }
            }
        }
        // 成功者標記為「今日已預產」(共用儲存,手動頁接手時可略過)
        if !ok_sns.is_empty() {
            if let Err(e) = state.pregen_done.mark(&state.db, &ok_sns).await {
                tracing::warn!(?e, day = %day, "標記 pregen_done 失敗");
            }
        }

        if fail > 0 {
            had_error = true;
        }
        g_ok += ok as u32;
        g_fail += fail as u32;
        g_skip += skipped as u32;
        g_empty += empty as u32;

        event_log::log_bg(
            state.db.clone(),
            if fail > 0 { "warn" } else { "info" },
            "pregen",
            "自動預產完成",
            format!(
                "date={date} source={source} 總數={total} 成功={ok} 略過={skipped} 失敗={fail} 無檔={empty}"
            ),
        );
    }

    // 更新可觀測狀態(供前端「上次執行」顯示)
    let now = chrono::Local::now();
    let mut st = state.pregen_status.write().await;
    st.last_run_at = Some(now.format("%Y-%m-%d %H:%M:%S").to_string());
    st.last_trigger = Some(trigger.to_string());
    st.last_ok = g_ok;
    st.last_fail = g_fail;
    st.last_skipped = g_skip;
    st.last_empty = g_empty;
    st.last_had_error = had_error;
    st.last_failed_sns = g_failed;
}
