//! 印單統計 commands
//!
//! 資料來源:print_event 表(三個來源 scan / auto / ipc 都會寫入)
//! 計數規則:用 COUNT(DISTINCT shipping_no) 去重,同一張單重印只算一次
//!
//! 時間欄位 created_at 寫入時用 datetime('now','localtime'),
//! 查詢過濾與分組直接用本機時區字串比較,避免 UTC 轉換誤差。

use serde::{Deserialize, Serialize};
use sqlx::Row;
use tauri::{AppHandle, Emitter, State};

use crate::{AppResult, SharedState};

/// 印單事件寫入後 emit 給前端,讓統計即時刷新(取代輪詢)
pub const PRINT_STATS_UPDATED_EVENT: &str = "print-stats-updated";

#[derive(Debug, Clone, Serialize)]
pub struct PrintStatsUpdated {
    pub source: &'static str,
    pub shipping_no: String,
}

/// 發送統計更新事件(emit 失敗不影響業務,只記 warn)
pub fn emit_print_stats_updated(app: &AppHandle, source: &'static str, shipping_no: &str) {
    let payload = PrintStatsUpdated {
        source,
        shipping_no: shipping_no.to_string(),
    };
    if let Err(e) = app.emit(PRINT_STATS_UPDATED_EVENT, payload) {
        tracing::warn!(?e, "emit print-stats-updated 失敗");
    }
}

#[derive(Debug, Deserialize)]
pub struct RangeReq {
    /// 起始日期(yyyy-mm-dd),空字串等同今天
    #[serde(default)]
    pub start_date: Option<String>,
    /// 結束日期(yyyy-mm-dd),空字串等同今天
    #[serde(default)]
    pub end_date: Option<String>,
}

impl RangeReq {
    /// 取 [start 00:00:00, end+1day 00:00:00) 字串區間,SQL 用 BETWEEN/range 比較
    fn resolve(&self) -> (String, String) {
        let today = chrono::Local::now().date_naive();
        let start = self
            .start_date
            .as_deref()
            .and_then(|s| chrono::NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok())
            .unwrap_or(today);
        let end = self
            .end_date
            .as_deref()
            .and_then(|s| chrono::NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok())
            .unwrap_or(today);
        let (s, e) = if start <= end { (start, end) } else { (end, start) };
        let s_str = format!("{} 00:00:00", s.format("%Y-%m-%d"));
        // 結束日期 +1 天當作開區間上界
        let e_next = e + chrono::Duration::days(1);
        let e_str = format!("{} 00:00:00", e_next.format("%Y-%m-%d"));
        (s_str, e_str)
    }
}

#[derive(Debug, Serialize)]
pub struct SummaryResp {
    // 本場累計(self-reset session):從 app_setting.work_session_reset_at 開始計
    pub since_reset_at: String,
    pub since_reset: i64,
    pub packages_since_reset: i64,
    // 過去 24 小時滾動窗口(取代「今日」自然日,避免跨日切割)
    pub past_24h: i64,
    pub packages_past_24h: i64,
    pub last_7_days: i64,
    pub packages_last_7_days: i64,
    pub last_30_days: i64,
    pub packages_last_30_days: i64,
    pub range_total: i64,
    pub packages_range_total: i64,
    pub by_source: Vec<SourceCount>,
}

#[derive(Debug, Serialize)]
pub struct SourceCount {
    pub source: String,
    pub count: i64,
    /// 該來源的分袋數(僅 auto 來源有意義,scan / ipc 永遠 0)
    pub package_count: i64,
}

/// 概況:今日 / 昨日 / 近 7 天 / 近 30 天 / 自訂區間 / 三來源拆分
#[tauri::command]
pub async fn print_stats_summary(
    state: State<'_, SharedState>,
    req: RangeReq,
) -> AppResult<SummaryResp> {
    let (range_start, range_end) = req.resolve();

    // 本場累計起算時間 — 從 KV 拿(由 migration 初始化、reset command 更新)
    // value 欄位 schema 為 TEXT(nullable),sqlx 必須先取 Option<String> 再 flatten,
    // 否則 try_get::<String,_> 對 nullable column 一律拋 Err,被吞成空字串
    let since_reset_at: String = sqlx::query("SELECT COALESCE(value, '') AS v FROM app_setting WHERE key = 'work_session_reset_at'")
        .fetch_optional(&state.db)
        .await?
        .and_then(|row| row.try_get::<String, _>("v").ok())
        .unwrap_or_default();

    let since_reset_row = sqlx::query(
        "SELECT COUNT(DISTINCT shipping_no) AS n,
                COUNT(DISTINCT package_sn)  AS p
         FROM print_event WHERE created_at >= ?",
    )
    .bind(&since_reset_at)
    .fetch_one(&state.db)
    .await?;
    let since_reset: i64 = since_reset_row.try_get("n").unwrap_or(0);
    let packages_since_reset: i64 = since_reset_row.try_get("p").unwrap_or(0);

    // 過去 24 小時滾動 — datetime 而非 date,跨自然日不會被切
    let past_24h_row = sqlx::query(
        "SELECT COUNT(DISTINCT shipping_no) AS n,
                COUNT(DISTINCT package_sn)  AS p
         FROM print_event
         WHERE created_at >= datetime('now','localtime','-24 hours')",
    )
    .fetch_one(&state.db)
    .await?;
    let past_24h: i64 = past_24h_row.try_get("n").unwrap_or(0);
    let packages_past_24h: i64 = past_24h_row.try_get("p").unwrap_or(0);

    let last_7_row = sqlx::query(
        "SELECT COUNT(DISTINCT shipping_no) AS n,
                COUNT(DISTINCT package_sn)  AS p
         FROM print_event
         WHERE created_at >= date('now','localtime','-6 days') || ' 00:00:00'",
    )
    .fetch_one(&state.db)
    .await?;
    let last_7: i64 = last_7_row.try_get("n").unwrap_or(0);
    let packages_last_7: i64 = last_7_row.try_get("p").unwrap_or(0);

    let last_30_row = sqlx::query(
        "SELECT COUNT(DISTINCT shipping_no) AS n,
                COUNT(DISTINCT package_sn)  AS p
         FROM print_event
         WHERE created_at >= date('now','localtime','-29 days') || ' 00:00:00'",
    )
    .fetch_one(&state.db)
    .await?;
    let last_30: i64 = last_30_row.try_get("n").unwrap_or(0);
    let packages_last_30: i64 = last_30_row.try_get("p").unwrap_or(0);

    let range_row = sqlx::query(
        "SELECT COUNT(DISTINCT shipping_no) AS n,
                COUNT(DISTINCT package_sn)  AS p
         FROM print_event
         WHERE created_at >= ? AND created_at < ?",
    )
    .bind(&range_start)
    .bind(&range_end)
    .fetch_one(&state.db)
    .await?;
    let range_total: i64 = range_row.try_get("n").unwrap_or(0);
    let packages_range_total: i64 = range_row.try_get("p").unwrap_or(0);

    // 來源拆分:同一張單在 scan/auto/ipc 多次發生,
    // 各 source 內各自 DISTINCT(可能加總 > range_total,屬正常);
    // package_count 只 auto 來源有值,scan/ipc 因 package_sn 為 NULL 自動為 0
    let rows = sqlx::query(
        "SELECT source,
                COUNT(DISTINCT shipping_no) AS n,
                COUNT(DISTINCT package_sn)  AS p
         FROM print_event
         WHERE created_at >= ? AND created_at < ?
         GROUP BY source",
    )
    .bind(&range_start)
    .bind(&range_end)
    .fetch_all(&state.db)
    .await?;
    let by_source: Vec<SourceCount> = rows
        .into_iter()
        .map(|r| SourceCount {
            source: r.try_get("source").unwrap_or_default(),
            count: r.try_get("n").unwrap_or(0),
            package_count: r.try_get("p").unwrap_or(0),
        })
        .collect();

    Ok(SummaryResp {
        since_reset_at,
        since_reset,
        packages_since_reset,
        past_24h,
        packages_past_24h,
        last_7_days: last_7,
        packages_last_7_days: packages_last_7,
        last_30_days: last_30,
        packages_last_30_days: packages_last_30,
        range_total,
        packages_range_total,
        by_source,
    })
}

/// 重置本場累計 — 把 work_session_reset_at 更新為當下,返回新起算時間
#[tauri::command]
pub async fn work_session_reset(state: State<'_, SharedState>) -> AppResult<String> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    sqlx::query(
        "INSERT INTO app_setting (key, value, updated_at)
         VALUES ('work_session_reset_at', ?, datetime('now','localtime'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
    )
    .bind(&now)
    .execute(&state.db)
    .await?;
    Ok(now)
}

#[derive(Debug, Serialize)]
pub struct DailyPoint {
    pub date: String,
    pub count: i64,
}

#[derive(Debug, Deserialize)]
pub struct DailyReq {
    /// 起始日期(yyyy-mm-dd),預設今天往前 6 天(共 7 天)
    #[serde(default)]
    pub start_date: Option<String>,
    /// 結束日期,預設今天
    #[serde(default)]
    pub end_date: Option<String>,
}

/// 每日趨勢:[start, end] 範圍內每一天的 DISTINCT shipping_no
/// 空缺日期會補 count=0 點(前端畫圖才不會跳格)
#[tauri::command]
pub async fn print_stats_daily(
    state: State<'_, SharedState>,
    req: DailyReq,
) -> AppResult<Vec<DailyPoint>> {
    let today = chrono::Local::now().date_naive();
    let end = req
        .end_date
        .as_deref()
        .and_then(|s| chrono::NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok())
        .unwrap_or(today);
    let start = req
        .start_date
        .as_deref()
        .and_then(|s| chrono::NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok())
        .unwrap_or(end - chrono::Duration::days(6));
    let (start, end) = if start <= end { (start, end) } else { (end, start) };

    let s_str = format!("{} 00:00:00", start.format("%Y-%m-%d"));
    let e_str = format!("{} 00:00:00", (end + chrono::Duration::days(1)).format("%Y-%m-%d"));

    let rows = sqlx::query(
        "SELECT date(created_at) AS d, COUNT(DISTINCT shipping_no) AS n
         FROM print_event
         WHERE created_at >= ? AND created_at < ?
         GROUP BY date(created_at)
         ORDER BY d",
    )
    .bind(&s_str)
    .bind(&e_str)
    .fetch_all(&state.db)
    .await?;

    use std::collections::HashMap;
    let mut map: HashMap<String, i64> = HashMap::new();
    for r in rows {
        let d: String = r.try_get("d").unwrap_or_default();
        let n: i64 = r.try_get("n").unwrap_or(0);
        map.insert(d, n);
    }

    // 補空白日期為 0
    let mut points = Vec::new();
    let mut cur = start;
    while cur <= end {
        let key = cur.format("%Y-%m-%d").to_string();
        let count = map.get(&key).copied().unwrap_or(0);
        points.push(DailyPoint { date: key, count });
        cur += chrono::Duration::days(1);
    }
    Ok(points)
}

#[derive(Debug, Serialize)]
pub struct HourlyPoint {
    /// "HH:00" 格式
    pub hour: String,
    pub count: i64,
}

/// 當下往前 8 個整點的每小時印單數(用 DISTINCT shipping_no 同小時內去重)
/// 例:現在 14:43 → 回 [07:00, 08:00, 09:00, 10:00, 11:00, 12:00, 13:00, 14:00]
#[tauri::command]
pub async fn print_stats_hourly(state: State<'_, SharedState>) -> AppResult<Vec<HourlyPoint>> {
    let now = chrono::Local::now();
    // 從目前所在整點往前 7 個(共 8 個整點)
    let cur_hour = now
        .date_naive()
        .and_hms_opt(now.format("%H").to_string().parse::<u32>().unwrap_or(0), 0, 0)
        .unwrap();
    let start_hour = cur_hour - chrono::Duration::hours(7);
    let end_hour = cur_hour + chrono::Duration::hours(1);

    let s_str = start_hour.format("%Y-%m-%d %H:%M:%S").to_string();
    let e_str = end_hour.format("%Y-%m-%d %H:%M:%S").to_string();

    // strftime('%Y-%m-%d %H',created_at) 取到「YYYY-MM-DD HH」key
    let rows = sqlx::query(
        "SELECT strftime('%Y-%m-%d %H', created_at) AS hkey, COUNT(DISTINCT shipping_no) AS n
         FROM print_event
         WHERE created_at >= ? AND created_at < ?
         GROUP BY hkey
         ORDER BY hkey",
    )
    .bind(&s_str)
    .bind(&e_str)
    .fetch_all(&state.db)
    .await?;

    use std::collections::HashMap;
    let mut map: HashMap<String, i64> = HashMap::new();
    for r in rows {
        let k: String = r.try_get("hkey").unwrap_or_default();
        let n: i64 = r.try_get("n").unwrap_or(0);
        map.insert(k, n);
    }

    let mut points = Vec::with_capacity(8);
    let mut cur = start_hour;
    while cur < end_hour {
        let key = cur.format("%Y-%m-%d %H").to_string();
        let label = cur.format("%H:00").to_string();
        let count = map.get(&key).copied().unwrap_or(0);
        points.push(HourlyPoint { hour: label, count });
        cur += chrono::Duration::hours(1);
    }
    Ok(points)
}

#[derive(Debug, Serialize)]
pub struct ProviderCount {
    pub provider_code: String,
    pub count: i64,
}

/// 依物流商分組(指定區間內)
#[tauri::command]
pub async fn print_stats_by_provider(
    state: State<'_, SharedState>,
    req: RangeReq,
) -> AppResult<Vec<ProviderCount>> {
    let (s, e) = req.resolve();
    let rows = sqlx::query(
        "SELECT COALESCE(provider_code,'') AS p, COUNT(DISTINCT shipping_no) AS n
         FROM print_event
         WHERE created_at >= ? AND created_at < ?
         GROUP BY p
         ORDER BY n DESC, p",
    )
    .bind(&s)
    .bind(&e)
    .fetch_all(&state.db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| ProviderCount {
            provider_code: r.try_get("p").unwrap_or_default(),
            count: r.try_get("n").unwrap_or(0),
        })
        .collect())
}

#[derive(Debug, Serialize)]
pub struct StickerCount {
    pub sticker_user: String,
    pub count: i64,
}

/// 依貼標人員分組(指定區間內,排除空 sticker)
#[tauri::command]
pub async fn print_stats_by_sticker(
    state: State<'_, SharedState>,
    req: RangeReq,
) -> AppResult<Vec<StickerCount>> {
    let (s, e) = req.resolve();
    let rows = sqlx::query(
        "SELECT sticker_user AS u, COUNT(DISTINCT shipping_no) AS n
         FROM print_event
         WHERE created_at >= ? AND created_at < ?
           AND sticker_user IS NOT NULL AND sticker_user <> ''
         GROUP BY u
         ORDER BY n DESC, u",
    )
    .bind(&s)
    .bind(&e)
    .fetch_all(&state.db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| StickerCount {
            sticker_user: r.try_get("u").unwrap_or_default(),
            count: r.try_get("n").unwrap_or(0),
        })
        .collect())
}

// =====================================================================
// 階段 2:scanner_user(操作人員)分組
// =====================================================================

#[derive(Debug, Serialize)]
pub struct ScannerCount {
    pub scanner_user: String,
    pub count: i64,
}

/// 依操作人員分組(scan / auto 來源由前端帶,ipc 沒此欄位)
#[tauri::command]
pub async fn print_stats_by_scanner(
    state: State<'_, SharedState>,
    req: RangeReq,
) -> AppResult<Vec<ScannerCount>> {
    let (s, e) = req.resolve();
    let rows = sqlx::query(
        "SELECT scanner_user AS u, COUNT(DISTINCT shipping_no) AS n
         FROM print_event
         WHERE created_at >= ? AND created_at < ?
           AND scanner_user IS NOT NULL AND scanner_user <> ''
         GROUP BY u
         ORDER BY n DESC, u",
    )
    .bind(&s)
    .bind(&e)
    .fetch_all(&state.db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| ScannerCount {
            scanner_user: r.try_get("u").unwrap_or_default(),
            count: r.try_get("n").unwrap_or(0),
        })
        .collect())
}

// =====================================================================
// 分揀通道分組(只有 source='ipc' 工控機分揀機出單帶 channel_code)
// =====================================================================

#[derive(Debug, Serialize)]
pub struct ChannelCount {
    /// 分揀通道代碼(print_event.channel_code,工控機分揀機出口)
    pub channel_code: String,
    /// 該通道當前對應的物理位置(L1-L4 / R1-R4),通道代碼已被改派則為 None
    pub position: Option<String>,
    /// 該通道當前指派的物流商代碼(對齊 dispatch_provider.code),未設定則為 None
    pub dispatch_code: Option<String>,
    pub count: i64,
}

/// 依分揀通道分組(指定區間內,只統計工控機 ipc 來源、channel_code 非空者)
/// LEFT JOIN sort_channels 帶出當前位置與指派物流(一對一,channel_code 有 unique index)。
/// 通道代碼事後被改派時,position/dispatch_code 反映「當前」設定,歷史印單數仍以當時 channel_code 歸戶。
#[tauri::command]
pub async fn print_stats_by_channel(
    state: State<'_, SharedState>,
    req: RangeReq,
) -> AppResult<Vec<ChannelCount>> {
    let (s, e) = req.resolve();
    let rows = sqlx::query(
        "SELECT pe.channel_code AS code,
                sc.position      AS position,
                sc.dispatch_code AS dispatch_code,
                COUNT(DISTINCT pe.shipping_no) AS n
         FROM print_event pe
         LEFT JOIN sort_channels sc ON sc.channel_code = pe.channel_code
         WHERE pe.created_at >= ? AND pe.created_at < ?
           AND pe.channel_code IS NOT NULL AND pe.channel_code <> ''
         GROUP BY pe.channel_code
         ORDER BY n DESC, code",
    )
    .bind(&s)
    .bind(&e)
    .fetch_all(&state.db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| ChannelCount {
            channel_code: r.try_get("code").unwrap_or_default(),
            position: r.try_get::<Option<String>, _>("position").ok().flatten(),
            dispatch_code: r.try_get::<Option<String>, _>("dispatch_code").ok().flatten(),
            count: r.try_get("n").unwrap_or(0),
        })
        .collect())
}

// =====================================================================
// 階段 3:熱力圖 / 重印分布 / 物流×來源 cross-tab
// =====================================================================

#[derive(Debug, Serialize)]
pub struct HeatmapCell {
    /// 0 (Sun) ~ 6 (Sat)
    pub weekday: i64,
    /// 0 ~ 23
    pub hour: i64,
    pub count: i64,
}

/// 7 天 × 24 小時熱力圖:DISTINCT shipping_no
/// 範圍取「最近 N 天」(預設 30),用整體分布看尖峰時段
#[tauri::command]
pub async fn print_stats_heatmap(
    state: State<'_, SharedState>,
    req: RangeReq,
) -> AppResult<Vec<HeatmapCell>> {
    let (s, e) = req.resolve();
    // strftime('%w') 回 '0'(Sun) ~ '6'(Sat),'%H' 回 '00' ~ '23'
    let rows = sqlx::query(
        "SELECT CAST(strftime('%w', created_at) AS INTEGER) AS w,
                CAST(strftime('%H', created_at) AS INTEGER) AS h,
                COUNT(DISTINCT shipping_no) AS n
         FROM print_event
         WHERE created_at >= ? AND created_at < ?
         GROUP BY w, h",
    )
    .bind(&s)
    .bind(&e)
    .fetch_all(&state.db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| HeatmapCell {
            weekday: r.try_get("w").unwrap_or(0),
            hour: r.try_get("h").unwrap_or(0),
            count: r.try_get("n").unwrap_or(0),
        })
        .collect())
}

#[derive(Debug, Serialize)]
pub struct ReprintBucket {
    /// 該張單在區間內被列印的次數
    pub print_times: i64,
    /// 落在此 bucket 的張數
    pub shipment_count: i64,
}

/// 重印分布:統計區間內每張單被印幾次(1=只印一次 / 2=印過 2 次 / ...)
#[tauri::command]
pub async fn print_stats_reprint(
    state: State<'_, SharedState>,
    req: RangeReq,
) -> AppResult<Vec<ReprintBucket>> {
    let (s, e) = req.resolve();
    let rows = sqlx::query(
        "WITH per_ship AS (
           SELECT shipping_no, COUNT(*) AS c
           FROM print_event
           WHERE created_at >= ? AND created_at < ?
           GROUP BY shipping_no
         )
         SELECT c AS print_times, COUNT(*) AS shipment_count
         FROM per_ship
         GROUP BY c
         ORDER BY c",
    )
    .bind(&s)
    .bind(&e)
    .fetch_all(&state.db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| ReprintBucket {
            print_times: r.try_get("print_times").unwrap_or(0),
            shipment_count: r.try_get("shipment_count").unwrap_or(0),
        })
        .collect())
}

#[derive(Debug, Serialize)]
pub struct ProviderSourceCell {
    pub provider_code: String,
    pub source: String,
    pub count: i64,
}

/// 物流商 × 來源 cross-tab:每組合的去重面單數
#[tauri::command]
pub async fn print_stats_provider_source(
    state: State<'_, SharedState>,
    req: RangeReq,
) -> AppResult<Vec<ProviderSourceCell>> {
    let (s, e) = req.resolve();
    let rows = sqlx::query(
        "SELECT COALESCE(provider_code,'') AS p,
                source AS s,
                COUNT(DISTINCT shipping_no) AS n
         FROM print_event
         WHERE created_at >= ? AND created_at < ?
         GROUP BY p, s
         ORDER BY p, s",
    )
    .bind(&s)
    .bind(&e)
    .fetch_all(&state.db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| ProviderSourceCell {
            provider_code: r.try_get("p").unwrap_or_default(),
            source: r.try_get("s").unwrap_or_default(),
            count: r.try_get("n").unwrap_or(0),
        })
        .collect())
}

// =====================================================================
// 階段 4:失敗 / 異常率
// =====================================================================

#[derive(Debug, Serialize)]
pub struct FailureBucket {
    pub respond_code: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
pub struct FailureResp {
    /// 區間內失敗事件數
    pub failure_count: i64,
    /// 同區間內成功 events 數(來自 print_event)— 用於計算失敗率
    pub success_count: i64,
    /// failure_count / (failure_count + success_count),四捨五入到小數 4 位
    pub failure_rate: f64,
    pub by_code: Vec<FailureBucket>,
}

/// 失敗率與分類:從 print_failure_event 拉
#[tauri::command]
pub async fn print_stats_failure(
    state: State<'_, SharedState>,
    req: RangeReq,
) -> AppResult<FailureResp> {
    let (s, e) = req.resolve();

    let failure_count: i64 = sqlx::query(
        "SELECT COUNT(*) AS n FROM print_failure_event WHERE created_at >= ? AND created_at < ?",
    )
    .bind(&s)
    .bind(&e)
    .fetch_one(&state.db)
    .await?
    .try_get("n")
    .unwrap_or(0);

    let success_count: i64 = sqlx::query(
        "SELECT COUNT(*) AS n FROM print_event WHERE created_at >= ? AND created_at < ?",
    )
    .bind(&s)
    .bind(&e)
    .fetch_one(&state.db)
    .await?
    .try_get("n")
    .unwrap_or(0);

    let total = failure_count + success_count;
    let failure_rate = if total > 0 {
        (failure_count as f64 / total as f64 * 10000.0).round() / 10000.0
    } else {
        0.0
    };

    let rows = sqlx::query(
        "SELECT respond_code, COUNT(*) AS n FROM print_failure_event
         WHERE created_at >= ? AND created_at < ?
         GROUP BY respond_code
         ORDER BY n DESC",
    )
    .bind(&s)
    .bind(&e)
    .fetch_all(&state.db)
    .await?;
    let by_code: Vec<FailureBucket> = rows
        .into_iter()
        .map(|r| FailureBucket {
            respond_code: r.try_get("respond_code").unwrap_or_default(),
            count: r.try_get("n").unwrap_or(0),
        })
        .collect();

    Ok(FailureResp {
        failure_count,
        success_count,
        failure_rate,
        by_code,
    })
}

// =====================================================================
// 階段 5:歷史比對(本週 vs 上週 / 本月 vs 上月)
// =====================================================================

#[derive(Debug, Serialize)]
pub struct ComparePeriod {
    pub label: String,
    pub current: i64,
    pub previous: i64,
    /// (current - previous) / previous;previous = 0 時為 None(無法計算)
    pub delta_ratio: Option<f64>,
}

/// 本週 vs 上週、本月 vs 上月,皆用 DISTINCT shipping_no
#[tauri::command]
pub async fn print_stats_compare(state: State<'_, SharedState>) -> AppResult<Vec<ComparePeriod>> {
    let now = chrono::Local::now().naive_local();
    let today = now.date();

    // 「本週」定義為「最近 7 天」(含今天往前 6 天),非 ISO week,簡單對齊使用者直覺
    let week_now_start = today - chrono::Duration::days(6);
    let week_prev_end = week_now_start; // open upper bound
    let week_prev_start = week_now_start - chrono::Duration::days(7);

    // 本月 = 今天往前 30 天,上月 = 前 30 天
    let month_now_start = today - chrono::Duration::days(29);
    let month_prev_end = month_now_start;
    let month_prev_start = month_now_start - chrono::Duration::days(30);

    async fn count_in(
        db: &sqlx::SqlitePool,
        start: chrono::NaiveDate,
        end_exclusive: chrono::NaiveDate,
    ) -> sqlx::Result<i64> {
        let s = format!("{} 00:00:00", start.format("%Y-%m-%d"));
        let e = format!("{} 00:00:00", end_exclusive.format("%Y-%m-%d"));
        let row = sqlx::query(
            "SELECT COUNT(DISTINCT shipping_no) AS n FROM print_event
             WHERE created_at >= ? AND created_at < ?",
        )
        .bind(&s)
        .bind(&e)
        .fetch_one(db)
        .await?;
        Ok(row.try_get::<i64, _>("n").unwrap_or(0))
    }

    let tomorrow = today + chrono::Duration::days(1);
    let week_current = count_in(&state.db, week_now_start, tomorrow).await?;
    let week_previous = count_in(&state.db, week_prev_start, week_prev_end).await?;
    let month_current = count_in(&state.db, month_now_start, tomorrow).await?;
    let month_previous = count_in(&state.db, month_prev_start, month_prev_end).await?;

    let ratio = |cur: i64, prev: i64| -> Option<f64> {
        if prev == 0 {
            None
        } else {
            Some(((cur - prev) as f64 / prev as f64 * 10000.0).round() / 10000.0)
        }
    };

    Ok(vec![
        ComparePeriod {
            label: "week".into(),
            current: week_current,
            previous: week_previous,
            delta_ratio: ratio(week_current, week_previous),
        },
        ComparePeriod {
            label: "month".into(),
            current: month_current,
            previous: month_previous,
            delta_ratio: ratio(month_current, month_previous),
        },
    ])
}
