//! 面單預產自動排程 — 常駐背景 task(headless)
//!
//! 每天到設定的時間點(可多個 "HH:MM"),自動反查當日整批訂單編號,
//! 逐筆以 Download 模式預下載面單到本機快取,工控機之後來要直接命中、不用等雲端。
//!
//! 設計重點:
//!   - 不需開啟面單預產頁、不需人在場;設定熱套用(每 tick 重讀 config)即時生效。
//!   - 每日去重:當日已成功預產過的 order_sn 記在記憶體,多時段重跑只抓新出現的單,跨日自動清空。
//!   - 失敗不中斷:單筆失敗只計數,整批結果寫入事件記錄。

use std::collections::HashSet;
use std::time::Duration;

use tokio::task::JoinSet;

use crate::commands::cloud_commands::pregen_label_to_cache;
use crate::{event_log, SharedState};

/// 巡檢間隔:每 30s tick 一次,比對當前 HH:MM 是否命中排程時間點
const TICK_SECS: u64 = 30;
/// 預下載並發數(對齊前端 worker 數,避免一次打爆雲端)
const CONCURRENCY: usize = 4;

/// 啟動排程 worker(在 AppState 建立後呼叫,持有 SharedState)
pub fn start_scheduler(state: SharedState) {
    tokio::spawn(async move {
        // 當日狀態:done_date 為當前日期字串;切日時清空 fired / done_orders
        let mut done_date = String::new();
        let mut fired: HashSet<String> = HashSet::new(); // 今日已觸發的時間點
        let mut done_orders: HashSet<String> = HashSet::new(); // 今日已成功預產的 order_sn

        loop {
            tokio::time::sleep(Duration::from_secs(TICK_SECS)).await;

            let cfg = { state.config.read().await.pre_gen_schedule.clone() };

            let now = chrono::Local::now();
            let today = now.format("%Y-%m-%d").to_string();
            if today != done_date {
                done_date = today;
                fired.clear();
                done_orders.clear();
            }

            if !cfg.enabled || cfg.times.is_empty() || cfg.sources.is_empty() {
                continue;
            }

            let hhmm = now.format("%H:%M").to_string();
            if !cfg.times.iter().any(|t| t.trim() == hhmm) || fired.contains(&hhmm) {
                continue;
            }
            fired.insert(hhmm.clone());

            run_pregen(&state, &cfg, &mut done_orders).await;
        }
    });
}

/// 跑一輪預產:對每個來源反查當日訂單 → 預下載快取,結果寫事件記錄
async fn run_pregen(
    state: &SharedState,
    cfg: &crate::config::PreGenScheduleConfig,
    done_orders: &mut HashSet<String>,
) {
    let date = (chrono::Local::now().date_naive()
        + chrono::Duration::days(cfg.date_offset_days))
    .format("%Y-%m-%d")
    .to_string();

    for source in &cfg.sources {
        let res = match state.cloud.fetch_orders_by_date(&date, source).await {
            Ok(r) => r,
            Err(e) => {
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

        // 去重:略過今日已成功預產過的單(多時段重跑只抓新出現的)
        let total = res.order_sns.len();
        let fresh: Vec<String> = res
            .order_sns
            .into_iter()
            .filter(|sn| !sn.trim().is_empty() && !done_orders.contains(sn))
            .collect();
        let skipped = total - fresh.len();

        let (mut ok, mut empty, mut fail) = (0usize, 0usize, 0usize);
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
                            done_orders.insert(sn);
                        }
                        Ok(false) => empty += 1,
                        Err(_) => fail += 1,
                    }
                }
            }
        }

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
}
