use serde::{Deserialize, Serialize};
use sqlx::Row;
use tauri::State;

use crate::{db::DbPool, AppError, AppResult, SharedState};

pub const POSITIONS: [&str; 8] = ["L1", "L2", "L3", "L4", "R1", "R2", "R3", "R4"];

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
    /// 是否啟用。false=暫停,暫停的通道不參與路由分配
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[tauri::command]
pub async fn sort_channel_list(state: State<'_, SharedState>) -> AppResult<Vec<SortChannel>> {
    // 用 CASE 排序保證 L1..L4, R1..R4 順序
    let rows = sqlx::query(
        "SELECT position, channel_code, job_sticker, enabled
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
}

fn normalize(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

#[tauri::command]
pub async fn sort_channel_save(
    state: State<'_, SharedState>,
    req: SortChannelSaveReq,
) -> AppResult<()> {
    if !POSITIONS.contains(&req.position.as_str()) {
        return Err(AppError::Server(format!("無效的通道位置: {}", req.position)));
    }

    let channel_code = normalize(req.channel_code);
    // 通道代碼是分揀機格口機器碼(工控機讀它路由格口,如 L1/R4/A01),必為 ASCII 機器碼。
    // 限英數與 - _、長度 ≤ 16,擋下把貼標人員名等中文/長字串誤填進通道代碼 ——
    // 誤填會被工控機當格口碼、且污染「依分揀通道」統計(歷史以當時 channel_code 歸戶,事後難清)。
    // 只在「代碼有變更」時驗證:既有(驗證上線前)存入的不合規舊值放行,讓操作員仍能改該通道
    // 其他欄位(物流指派/貼標),不被舊資料把整列存檔卡死。
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
         SET channel_code = ?, job_sticker = ?, updated_at = datetime('now','localtime')
         WHERE position = ?",
    )
    .bind(&channel_code)
    .bind(&job_sticker)
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
    use tauri::Emitter;
    if !POSITIONS.contains(&position.as_str()) {
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
    let _ = app.emit(
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
