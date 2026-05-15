use serde::{Deserialize, Serialize};
use sqlx::Row;
use tauri::State;

use crate::{AppError, AppResult, SharedState};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchProvider {
    pub code: String,
    pub name: String,
    pub sort_order: i64,
    pub print_profile: Option<String>,
}

#[tauri::command]
pub async fn dispatch_provider_list(
    state: State<'_, SharedState>,
) -> AppResult<Vec<DispatchProvider>> {
    let rows = sqlx::query(
        "SELECT code, name, sort_order, print_profile FROM dispatch_provider
         ORDER BY sort_order ASC, code ASC",
    )
    .fetch_all(&state.db)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| DispatchProvider {
            code: r.try_get("code").unwrap_or_default(),
            name: r.try_get("name").unwrap_or_default(),
            sort_order: r.try_get("sort_order").unwrap_or(0),
            print_profile: r.try_get("print_profile").ok(),
        })
        .collect())
}

#[derive(Debug, Deserialize)]
pub struct DispatchUpsertReq {
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub sort_order: i64,
    #[serde(default)]
    pub print_profile: Option<String>,
}

#[tauri::command]
pub async fn dispatch_provider_upsert(
    state: State<'_, SharedState>,
    req: DispatchUpsertReq,
) -> AppResult<()> {
    let code = req.code.trim();
    let name = req.name.trim();
    let print_profile = req
        .print_profile
        .as_deref()
        .map(str::trim)
        .unwrap_or_default();
    if code.is_empty() || name.is_empty() {
        return Err(AppError::Server("代碼與名稱皆不可為空".to_string()));
    }
    if print_profile.is_empty() {
        return Err(AppError::Server("列印設定不可為空".to_string()));
    }

    sqlx::query(
        "INSERT INTO dispatch_provider (code, name, sort_order, print_profile, updated_at)
         VALUES (?, ?, ?, ?, datetime('now'))
         ON CONFLICT(code) DO UPDATE SET
            name = excluded.name,
            sort_order = excluded.sort_order,
            print_profile = excluded.print_profile,
            updated_at = datetime('now')",
    )
    .bind(code)
    .bind(name)
    .bind(req.sort_order)
    .bind(print_profile)
    .execute(&state.db)
    .await?;
    Ok(())
}

#[tauri::command]
pub async fn dispatch_provider_delete(
    state: State<'_, SharedState>,
    code: String,
) -> AppResult<u64> {
    // 若仍被通道引用，先解除引用（不刪通道，只清空 dispatch_code）
    sqlx::query("UPDATE sort_channels SET dispatch_code = NULL WHERE dispatch_code = ?")
        .bind(&code)
        .execute(&state.db)
        .await?;
    let result = sqlx::query("DELETE FROM dispatch_provider WHERE code = ?")
        .bind(&code)
        .execute(&state.db)
        .await?;
    Ok(result.rows_affected())
}
