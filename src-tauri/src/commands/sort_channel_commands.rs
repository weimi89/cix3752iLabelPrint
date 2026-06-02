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
        "INSERT INTO sticker_history (name, used_at) VALUES (?, datetime('now'))
         ON CONFLICT(name) DO UPDATE SET used_at = datetime('now')",
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
    pub dispatch_code: Option<String>,
    pub job_sticker: Option<String>,
}

#[tauri::command]
pub async fn sort_channel_list(state: State<'_, SharedState>) -> AppResult<Vec<SortChannel>> {
    // 用 CASE 排序保證 L1..L4, R1..R4 順序
    let rows = sqlx::query(
        "SELECT position, channel_code, dispatch_code, job_sticker
         FROM sort_channels
         ORDER BY
           CASE substr(position,1,1) WHEN 'L' THEN 0 WHEN 'R' THEN 1 ELSE 2 END,
           CAST(substr(position,2) AS INTEGER)",
    )
    .fetch_all(&state.db)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| SortChannel {
            position: r.try_get("position").unwrap_or_default(),
            channel_code: r.try_get("channel_code").ok(),
            dispatch_code: r.try_get("dispatch_code").ok(),
            job_sticker: r.try_get("job_sticker").ok(),
        })
        .collect())
}

#[derive(Debug, Deserialize)]
pub struct SortChannelSaveReq {
    pub position: String,
    #[serde(default)]
    pub channel_code: Option<String>,
    #[serde(default)]
    pub dispatch_code: Option<String>,
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
    let dispatch_code = normalize(req.dispatch_code);
    let job_sticker = normalize(req.job_sticker);

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

    sqlx::query(
        "UPDATE sort_channels
         SET channel_code = ?, dispatch_code = ?, job_sticker = ?, updated_at = datetime('now')
         WHERE position = ?",
    )
    .bind(&channel_code)
    .bind(&dispatch_code)
    .bind(&job_sticker)
    .bind(&req.position)
    .execute(&state.db)
    .await?;

    // 寫入 sticker 歷史（給 autocomplete）
    if let Some(name) = job_sticker.as_deref() {
        upsert_sticker_history(&state.db, name).await?;
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
