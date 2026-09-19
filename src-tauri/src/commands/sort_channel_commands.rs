use serde::{Deserialize, Serialize};
use sqlx::Row;
use tauri::State;

use crate::{db::DbPool, AppError, AppResult, SharedState};

/// 每側最多幾格。格口配置對應現場分揀機的實體格口數,超過這個數多半是填錯。
pub const MAX_SIDE_COUNT: i64 = 10;

/// 格口配置:左右各幾格。以 `sort_channels` 現有的列為準、不另存設定 ——
/// 列在就是有這一格,設定頁、手機遙控、看板、分配全從同一份列讀,不會出現「設定說 3 格、列還有 5 格」。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SortLayout {
    pub left: i64,
    pub right: i64,
}

/// 拆位置代碼:`L3` → ('L', 3)。格式不對回 None,各入口靠它擋掉亂填的位置。
pub fn parse_position(pos: &str) -> Option<(char, i64)> {
    let mut chars = pos.chars();
    let side = chars.next()?;
    if side != 'L' && side != 'R' {
        return None;
    }
    let n: i64 = chars.as_str().parse().ok()?;
    (n >= 1).then_some((side, n))
}

/// 目前的格口配置:各側取最大的位置號(套用配置時位置一定連號,最大號就是格數)。
pub async fn load_layout(db: &DbPool) -> AppResult<SortLayout> {
    let rows = sqlx::query("SELECT position FROM sort_channels").fetch_all(db).await?;
    let mut layout = SortLayout { left: 0, right: 0 };
    for r in rows {
        let pos: String = r.try_get("position").unwrap_or_default();
        match parse_position(&pos) {
            Some(('L', n)) => layout.left = layout.left.max(n),
            Some(('R', n)) => layout.right = layout.right.max(n),
            _ => {}
        }
    }
    Ok(layout)
}

/// 位置是否存在於目前的格口配置。桌面與手機所有「依位置操作」的入口都靠這個擋:
/// 縮減配置後被移除的位置不能再被暫停／指派,否則會憑空寫出一列。
pub async fn position_exists(db: &DbPool, position: &str) -> bool {
    if parse_position(position).is_none() {
        return false;
    }
    sqlx::query("SELECT 1 FROM sort_channels WHERE position = ?")
        .bind(position)
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
        .is_some()
}

/// 套用格口配置:補建缺少的位置列、移除超出的位置(連同它的物流指派)。回傳被移除的位置。
/// 被移除位置的通道代碼、貼標人員、印表機設定跟著消失 —— 呼叫端要先讓使用者確認過。
pub async fn apply_layout(db: &DbPool, layout: SortLayout) -> AppResult<Vec<String>> {
    for (side, n) in [("左", layout.left), ("右", layout.right)] {
        if !(1..=MAX_SIDE_COUNT).contains(&n) {
            return Err(AppError::Server(format!(
                "{side}側格數必須在 1 到 {MAX_SIDE_COUNT} 之間"
            )));
        }
    }
    let mut tx = db.begin().await?;
    let mut removed = Vec::new();
    for (side, count) in [('L', layout.left), ('R', layout.right)] {
        for n in 1..=count {
            // updated_at 明寫 localtime:0003 建表時的預設值是 UTC,沿用會讓新列比其他列少 8 小時
            sqlx::query(
                "INSERT OR IGNORE INTO sort_channels (position, updated_at) VALUES (?, datetime('now','localtime'))",
            )
            .bind(format!("{side}{n}"))
            .execute(&mut *tx)
            .await?;
        }
        let extra = sqlx::query(
            "SELECT position FROM sort_channels
              WHERE substr(position,1,1) = ? AND CAST(substr(position,2) AS INTEGER) > ?
              ORDER BY CAST(substr(position,2) AS INTEGER)",
        )
        .bind(side.to_string())
        .bind(count)
        .fetch_all(&mut *tx)
        .await?;
        for r in extra {
            let pos: String = r.try_get("position").unwrap_or_default();
            sqlx::query("DELETE FROM sort_channel_dispatch WHERE position = ?")
                .bind(&pos)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM sort_channels WHERE position = ?")
                .bind(&pos)
                .execute(&mut *tx)
                .await?;
            removed.push(pos);
        }
    }
    tx.commit().await?;
    Ok(removed)
}

#[tauri::command]
pub async fn sort_layout_get(state: State<'_, SharedState>) -> AppResult<SortLayout> {
    load_layout(&state.db).await
}

/// 改格口配置(桌面與手機 /rpc 共用)。記事件並廣播,看板與設定頁重拉清單。
#[tauri::command]
pub async fn sort_layout_save(
    state: State<'_, SharedState>,
    app: tauri::AppHandle,
    layout: SortLayout,
) -> AppResult<SortLayout> {
    let removed = apply_layout(&state.db, layout).await?;
    let detail = if removed.is_empty() {
        String::new()
    } else {
        format!(",移除 {}", removed.join("、"))
    };
    crate::event_log::log_bg(
        state.db.clone(),
        "info",
        "server",
        "格口配置",
        format!("格口配置改為左 {} 格／右 {} 格{detail}", layout.left, layout.right),
    );
    let _ = crate::event_bridge::emit(
        &app,
        "sort-layout-updated",
        serde_json::json!({ "left": layout.left, "right": layout.right, "removed": removed }),
    );
    load_layout(&state.db).await
}

/// 將人員姓名寫入共用歷史名單(操作 / 貼單 / 貼標人員三者共用同一份),
/// 已存在則只更新 used_at。空字串不寫入。
pub async fn upsert_sticker_history(db: &DbPool, name: &str) -> AppResult<()> {
    let name = name.trim();
    if name.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO sticker_history (name, used_at) VALUES (?, datetime('now','localtime'))
         ON CONFLICT(name) DO UPDATE SET used_at = datetime('now','localtime')",
    )
    .bind(name)
    .execute(db)
    .await?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SortChannel {
    pub position: String,
    pub channel_code: Option<String>,
    /// 指派物流(1 對多):一個通道可指派多個物流商,對應 dispatch_provider.code
    #[serde(default)]
    pub dispatch_codes: Vec<String>,
    pub job_sticker: Option<String>,
    /// direct_print 模式此通道面單送印的本機印表機(取代舊 dispatch_provider.printer_name)
    #[serde(default)]
    pub printer_name: Option<String>,
    /// 是否啟用。false=暫停,暫停的通道不參與路由分配
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[tauri::command]
pub async fn sort_channel_list(state: State<'_, SharedState>) -> AppResult<Vec<SortChannel>> {
    // 用 CASE 排序保證先左後右、號碼由小到大(L10 不會排在 L2 前面)
    let rows = sqlx::query(
        "SELECT position, channel_code, job_sticker, printer_name, enabled
         FROM sort_channels
         ORDER BY
           CASE substr(position,1,1) WHEN 'L' THEN 0 WHEN 'R' THEN 1 ELSE 2 END,
           CAST(substr(position,2) AS INTEGER)",
    )
    .fetch_all(&state.db)
    .await?;

    // 一次撈出全部通道→物流指派,在記憶體分組(避免 N+1 查詢)
    let dispatch_rows = sqlx::query(
        "SELECT position, dispatch_code FROM sort_channel_dispatch
         ORDER BY position, dispatch_code",
    )
    .fetch_all(&state.db)
    .await?;
    let mut dispatch_map: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for r in dispatch_rows {
        let pos: String = r.try_get("position").unwrap_or_default();
        let code: String = r.try_get("dispatch_code").unwrap_or_default();
        if !pos.is_empty() && !code.is_empty() {
            dispatch_map.entry(pos).or_default().push(code);
        }
    }

    Ok(rows
        .into_iter()
        .map(|r| {
            let position: String = r.try_get("position").unwrap_or_default();
            let dispatch_codes = dispatch_map.remove(&position).unwrap_or_default();
            SortChannel {
                channel_code: r.try_get("channel_code").ok(),
                job_sticker: r.try_get("job_sticker").ok(),
                printer_name: r.try_get("printer_name").ok().flatten(),
                enabled: r.try_get::<i64, _>("enabled").unwrap_or(1) != 0,
                dispatch_codes,
                position,
            }
        })
        .collect())
}

#[derive(Debug, Deserialize)]
pub struct SortChannelSaveReq {
    pub position: String,
    #[serde(default)]
    pub channel_code: Option<String>,
    /// 指派物流(1 對多):空陣列代表未指派
    #[serde(default)]
    pub dispatch_codes: Vec<String>,
    #[serde(default)]
    pub job_sticker: Option<String>,
    /// direct_print 模式此通道的本機印表機
    #[serde(default)]
    pub printer_name: Option<String>,
}

fn normalize(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

#[tauri::command]
pub async fn sort_channel_save(
    state: State<'_, SharedState>,
    req: SortChannelSaveReq,
) -> AppResult<()> {
    if !position_exists(&state.db, &req.position).await {
        return Err(AppError::Server(format!("無效的通道位置: {}", req.position)));
    }

    let channel_code = normalize(req.channel_code);
    // 通道代碼是分揀機格口機器碼(工控機讀它路由格口,如 L1/R4/A01),必為 ASCII 機器碼。
    // 限英數與 - _、長度 ≤ 16,擋下把貼標人員名等中文/長字串誤填進通道代碼 ——
    // 誤填會被工控機當格口碼、且污染「依分揀通道」統計(歷史以當時 channel_code 歸戶,事後難清)。
    // 只在「代碼有變更」時驗證:DB 內既有的不合規值放行,讓操作員仍能改該通道
    // 其他欄位(物流指派/貼標),不被這類資料把整列存檔卡死。
    if let Some(code) = channel_code.as_deref() {
        let current: Option<String> = sqlx::query(
            "SELECT channel_code FROM sort_channels WHERE position = ?",
        )
        .bind(&req.position)
        .fetch_optional(&state.db)
        .await?
        .and_then(|r| r.try_get::<Option<String>, _>("channel_code").ok().flatten());
        if current.as_deref() != Some(code) {
            let ok = code.chars().count() <= 16
                && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
            if !ok {
                return Err(AppError::Server(format!(
                    "通道代碼 \"{code}\" 格式不符:僅允許英數字與 - _(長度 ≤ 16),請勿填入人名等文字"
                )));
            }
        }
    }
    let job_sticker = normalize(req.job_sticker);
    let printer_name = normalize(req.printer_name);
    // 指派物流去重 + 去空白,保持原始順序
    let mut dispatch_codes: Vec<String> = Vec::new();
    for code in req.dispatch_codes {
        let code = code.trim().to_string();
        if !code.is_empty() && !dispatch_codes.contains(&code) {
            dispatch_codes.push(code);
        }
    }

    // channel_code 若有值，檢查是否被其他 position 佔用
    if let Some(code) = channel_code.as_deref() {
        let row = sqlx::query(
            "SELECT position FROM sort_channels WHERE channel_code = ? AND position <> ?",
        )
        .bind(code)
        .bind(&req.position)
        .fetch_optional(&state.db)
        .await?;
        if let Some(r) = row {
            let conflict: String = r.try_get("position").unwrap_or_default();
            return Err(AppError::Server(format!(
                "通道代碼 \"{code}\" 已被 {conflict} 使用"
            )));
        }
    }

    // 通道本身與多對多指派一起寫入,用交易保證原子性(避免刪了舊指派卻沒寫入新指派)
    let mut tx = state.db.begin().await?;

    sqlx::query(
        "UPDATE sort_channels
         SET channel_code = ?, job_sticker = ?, printer_name = ?, updated_at = datetime('now','localtime')
         WHERE position = ?",
    )
    .bind(&channel_code)
    .bind(&job_sticker)
    .bind(&printer_name)
    .bind(&req.position)
    .execute(&mut *tx)
    .await?;

    // 重設此通道的指派物流:先清空再寫入(整列覆蓋語意,與前端「儲存整列」一致)
    sqlx::query("DELETE FROM sort_channel_dispatch WHERE position = ?")
        .bind(&req.position)
        .execute(&mut *tx)
        .await?;
    for code in &dispatch_codes {
        sqlx::query(
            "INSERT OR IGNORE INTO sort_channel_dispatch (position, dispatch_code) VALUES (?, ?)",
        )
        .bind(&req.position)
        .bind(code)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    // 寫入 sticker 歷史（給 autocomplete）
    if let Some(name) = job_sticker.as_deref() {
        upsert_sticker_history(&state.db, name).await?;
    }

    Ok(())
}

/// 快速暫停 / 啟用某通道(分揀進行中即時生效,不需整列儲存)。
/// 只動 enabled 欄位,不影響使用者尚未儲存的通道代碼 / 指派物流編輯。
#[tauri::command]
pub async fn sort_channel_set_enabled(
    state: State<'_, SharedState>,
    app: tauri::AppHandle,
    position: String,
    enabled: bool,
) -> AppResult<()> {
    if !position_exists(&state.db, &position).await {
        return Err(AppError::Server(format!("無效的通道位置: {position}")));
    }
    sqlx::query(
        "UPDATE sort_channels
         SET enabled = ?, updated_at = datetime('now','localtime')
         WHERE position = ?",
    )
    .bind(if enabled { 1 } else { 0 })
    .bind(&position)
    .execute(&state.db)
    .await?;
    // 廣播給所有視窗(及讓手機輪詢一致):桌面與手機任一端切換,兩邊都同步
    // 記進事件記錄:格口件數突然掛零時要查得到是誰何時關的(手機遙控那條在 server::set_channel_enabled)
    crate::event_log::log_bg(
        state.db.clone(),
        "info",
        "server",
        "分揀通道切換",
        format!("通道 {position} 已{}(桌面)", if enabled { "啟用" } else { "暫停" }),
    );
    let _ = crate::event_bridge::emit(
        &app,
        "sort-channel-updated",
        serde_json::json!({ "position": position, "enabled": enabled }),
    );
    Ok(())
}

const SETTING_UNASSIGNED_CHANNEL: &str = "unassigned_channel_code";

/// 讀取「未設定指派物流」的 fallback 通道代碼
#[tauri::command]
pub async fn sort_channel_unassigned_get(
    state: State<'_, SharedState>,
) -> AppResult<Option<String>> {
    let row = sqlx::query(
        "SELECT value FROM settings WHERE key = ?",
    )
    .bind(SETTING_UNASSIGNED_CHANNEL)
    .fetch_optional(&state.db)
    .await?;
    Ok(row
        .and_then(|r| r.try_get::<String, _>("value").ok())
        .filter(|s| !s.is_empty()))
}

/// 儲存「未設定指派物流」的 fallback 通道代碼（傳 None / 空字串表示清除）
#[tauri::command]
pub async fn sort_channel_unassigned_save(
    state: State<'_, SharedState>,
    code: Option<String>,
) -> AppResult<()> {
    let code = code.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    match code {
        Some(c) => {
            sqlx::query(
                "INSERT INTO settings (key, value, updated_at)
                 VALUES (?, ?, datetime('now','localtime'))
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            )
            .bind(SETTING_UNASSIGNED_CHANNEL)
            .bind(c)
            .execute(&state.db)
            .await?;
        }
        None => {
            sqlx::query("DELETE FROM settings WHERE key = ?")
                .bind(SETTING_UNASSIGNED_CHANNEL)
                .execute(&state.db)
                .await?;
        }
    }
    Ok(())
}

/// 前端主動把人員姓名加入歷史名單(掃描/自動列印頁送出時呼叫)。
#[tauri::command]
pub async fn sticker_history_add(state: State<'_, SharedState>, name: String) -> AppResult<()> {
    upsert_sticker_history(&state.db, &name).await
}

#[tauri::command]
pub async fn sticker_history_list(state: State<'_, SharedState>) -> AppResult<Vec<String>> {
    let rows = sqlx::query("SELECT name FROM sticker_history ORDER BY used_at DESC LIMIT 200")
        .fetch_all(&state.db)
        .await?;
    Ok(rows
        .into_iter()
        .map(|r| r.try_get("name").unwrap_or_default())
        .collect())
}

#[tauri::command]
pub async fn sticker_history_delete(
    state: State<'_, SharedState>,
    name: String,
) -> AppResult<u64> {
    let result = sqlx::query("DELETE FROM sticker_history WHERE name = ?")
        .bind(&name)
        .execute(&state.db)
        .await?;
    Ok(result.rows_affected())
}
