//! 印單統計 commands
//!
//! 資料來源:print_event 表(三個來源 scan / auto / ipc 都會寫入)
//! 計數規則:用 COUNT(DISTINCT shipping_no) 去重,同一張單重印只算一次
//!
//! 時間欄位 created_at 寫入時用 datetime('now','localtime'),
//! 查詢過濾與分組直接用本機時區字串比較,避免 UTC 轉換誤差。

use serde::{Deserialize, Serialize};
use sqlx::Row;
use tauri::State;

use crate::{AppResult, SharedState};

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
    pub today: i64,
    pub yesterday: i64,
    pub last_7_days: i64,
    pub last_30_days: i64,
    pub range_total: i64,
    pub by_source: Vec<SourceCount>,
}

#[derive(Debug, Serialize)]
pub struct SourceCount {
    pub source: String,
    pub count: i64,
}

/// 概況:今日 / 昨日 / 近 7 天 / 近 30 天 / 自訂區間 / 三來源拆分
#[tauri::command]
pub async fn print_stats_summary(
    state: State<'_, SharedState>,
    req: RangeReq,
) -> AppResult<SummaryResp> {
    let (range_start, range_end) = req.resolve();

    let today: i64 = sqlx::query(
        "SELECT COUNT(DISTINCT shipping_no) AS n FROM print_event
         WHERE created_at >= date('now','localtime') || ' 00:00:00'",
    )
    .fetch_one(&state.db)
    .await?
    .try_get("n")
    .unwrap_or(0);

    let yesterday: i64 = sqlx::query(
        "SELECT COUNT(DISTINCT shipping_no) AS n FROM print_event
         WHERE created_at >= date('now','localtime','-1 day') || ' 00:00:00'
           AND created_at <  date('now','localtime')         || ' 00:00:00'",
    )
    .fetch_one(&state.db)
    .await?
    .try_get("n")
    .unwrap_or(0);

    let last_7: i64 = sqlx::query(
        "SELECT COUNT(DISTINCT shipping_no) AS n FROM print_event
         WHERE created_at >= date('now','localtime','-6 days') || ' 00:00:00'",
    )
    .fetch_one(&state.db)
    .await?
    .try_get("n")
    .unwrap_or(0);

    let last_30: i64 = sqlx::query(
        "SELECT COUNT(DISTINCT shipping_no) AS n FROM print_event
         WHERE created_at >= date('now','localtime','-29 days') || ' 00:00:00'",
    )
    .fetch_one(&state.db)
    .await?
    .try_get("n")
    .unwrap_or(0);

    let range_total: i64 = sqlx::query(
        "SELECT COUNT(DISTINCT shipping_no) AS n FROM print_event
         WHERE created_at >= ? AND created_at < ?",
    )
    .bind(&range_start)
    .bind(&range_end)
    .fetch_one(&state.db)
    .await?
    .try_get("n")
    .unwrap_or(0);

    // 來源拆分:同一張單在 scan/auto/ipc 多次發生,
    // 各 source 內各自 DISTINCT(可能加總 > range_total,屬正常)
    let rows = sqlx::query(
        "SELECT source, COUNT(DISTINCT shipping_no) AS n FROM print_event
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
        })
        .collect();

    Ok(SummaryResp {
        today,
        yesterday,
        last_7_days: last_7,
        last_30_days: last_30,
        range_total,
        by_source,
    })
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
