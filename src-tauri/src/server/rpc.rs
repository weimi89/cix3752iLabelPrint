//! Tauri command 的 HTTP 對應出口 —— 網頁版的資料通道。
//!
//! 桌面端走 `invoke('cmd', args)`,網頁端走 `POST /rpc/{cmd}`,兩者最終呼叫同一批
//! `#[tauri::command]` 函式。業務邏輯只有一份,不會出現「桌面修好了網頁沒修」。
//!
//! `AppHandle::state()` 就地生出 `State<'_, SharedState>` 餵給既有 command,
//! 因此新增 command 時只需在 `dispatch` 補一行,command 本身不必改。
//!
//! **新增 command 時務必同步補這裡的分派**,否則桌面用得到、網頁會回 404。

use axum::{
    extract::{ConnectInfo, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::de::DeserializeOwned;
use std::net::SocketAddr;
use serde_json::Value;
use tauri::Manager;

use crate::commands;
use crate::AppError;

/// RPC 層自身的錯誤(command 內部的 AppError 另外包)。
#[derive(Debug)]
pub(super) enum RpcError {
    /// 分派表沒有這支 command —— 多半是前端改了名字但這裡忘了補
    UnknownCommand(String),
    /// 參數缺漏或型別不符
    BadArgs(String),
    /// command 執行失敗(業務錯誤)
    Command(AppError),
    /// 這個來源不准做這件事
    Forbidden(String),
}

impl From<AppError> for RpcError {
    fn from(e: AppError) -> Self {
        RpcError::Command(e)
    }
}

impl IntoResponse for RpcError {
    fn into_response(self) -> Response {
        // 前端 fetch wrapper 見到非 2xx 就 throw,讓 `await api.xxx()` 的
        // try/catch 行為與桌面 invoke 的 reject 一致。
        let (code, msg) = match self {
            RpcError::UnknownCommand(c) => (
                StatusCode::NOT_FOUND,
                format!("未知的指令: {c}"),
            ),
            RpcError::BadArgs(m) => (StatusCode::BAD_REQUEST, m),
            RpcError::Forbidden(m) => (StatusCode::FORBIDDEN, m),
            RpcError::Command(e) => (StatusCode::BAD_REQUEST, e.to_string()),
        };
        (code, Json(serde_json::json!({ "error": msg }))).into_response()
    }
}

/// snake_case → camelCase(Tauri 前端傳參的慣例)
fn to_camel(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut upper_next = false;
    for ch in s.chars() {
        if ch == '_' {
            upper_next = true;
        } else if upper_next {
            out.extend(ch.to_uppercase());
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// 從 args 取出具名參數。
///
/// Tauri 的 invoke 會把 JS 的 camelCase key 對到 Rust 的 snake_case 參數
/// (`invoke('update_config', { newConfig })` → `new_config`),這裡沿用同一套規則,
/// 前端呼叫端因此一個字都不用改。
fn arg<T: DeserializeOwned>(args: &Value, name: &str) -> Result<T, RpcError> {
    let raw = args
        .get(name)
        .or_else(|| args.get(to_camel(name).as_str()))
        // 參數缺席時交給 serde 判斷:Option / 有 default 的型別會成功,
        // 必填的則回報缺少哪個參數,不會靜默塞空值進業務邏輯。
        .cloned()
        .unwrap_or(Value::Null);

    serde_json::from_value(raw)
        .map_err(|e| RpcError::BadArgs(format!("參數 {name} 解析失敗: {e}")))
}

/// command 回傳值轉 JSON
fn v<T: serde::Serialize>(val: T) -> Result<Value, RpcError> {
    serde_json::to_value(val)
        .map_err(|e| RpcError::BadArgs(format!("回應序列化失敗: {e}")))
}

/// `POST /rpc/{command}` —— body 是前端 invoke 的第二個參數原樣。
pub(super) async fn rpc_handler(
    State(state): State<super::ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(cmd): Path<String>,
    body: Option<Json<Value>>,
) -> Result<Json<Value>, RpcError> {
    let args = body.map(|Json(v)| v).unwrap_or(Value::Null);

    // 存取控制本身不能由外網來源改動,否則對方可以把自己劃進「內網」(見 auth.rs)
    if cmd == "update_config" {
        super::auth::guard_web_access_change(&state, peer.ip(), &args)
            .await
            .map_err(RpcError::Forbidden)?;
    }
    // 外網不得指定伺服器端的任意檔案路徑當列印來源(見 auth.rs)
    if cmd == "print_image" {
        super::auth::guard_print_image_path(&state, peer.ip(), &args)
            .await
            .map_err(RpcError::Forbidden)?;
    }
    // 換共用密碼等同換門鎖,同樣只能在門內做
    if cmd == "web_auth_set_password" {
        super::auth::guard_lan_only(&state, peer.ip(), "更換網頁存取密碼")
            .await
            .map_err(RpcError::Forbidden)?;
    }

    let out = dispatch(&state.app, &cmd, args).await?;
    Ok(Json(out))
}

/// command 名 → 既有 Tauri command 函式。
async fn dispatch(app: &tauri::AppHandle, cmd: &str, args: Value) -> Result<Value, RpcError> {
    Ok(match cmd {
        "bag_check_clear" => v(commands::bag_check_commands::bag_check_clear(app.state())?)?,
        "bag_check_snapshot" => v(commands::bag_check_commands::bag_check_snapshot(app.state())?)?,
        "cache_clear" => v(commands::cache_commands::cache_clear(app.state()).await?)?,
        "cache_stats" => v(commands::cache_commands::cache_stats(app.state()).await?)?,
        "camera_capture_now" => v(commands::camera_commands::camera_capture_now(app.clone(), app.state()).await?)?,
        "camera_set_zoom" => v(commands::camera_commands::camera_set_zoom(app.state(), arg(&args, "zoom")?).await?)?,
        "cloud_clearance_dispatch" => v(commands::cloud_commands::cloud_clearance_dispatch(app.state(), arg(&args, "req")?).await?)?,
        "cloud_clearance_options" => v(commands::cloud_commands::cloud_clearance_options(app.state()).await?)?,
        "cloud_clearance_progress" => v(commands::cloud_commands::cloud_clearance_progress(app.state(), arg(&args, "req")?).await?)?,
        "cloud_clearance_sticker" => v(commands::cloud_commands::cloud_clearance_sticker(app.state(), arg(&args, "req")?).await?)?,
        "cloud_clearance_store" => v(commands::cloud_commands::cloud_clearance_store(app.state(), arg(&args, "req")?).await?)?,
        "cloud_examine_package" => v(commands::cloud_commands::cloud_examine_package(app.state(), arg(&args, "req")?).await?)?,
        "cloud_fetch_cloud_print" => v(commands::cloud_commands::cloud_fetch_cloud_print(app.state(), app.clone(), arg(&args, "req")?).await?)?,
        "cloud_fetch_label" => v(commands::cloud_commands::cloud_fetch_label(app.state(), app.clone(), arg(&args, "req")?).await?)?,
        "cloud_field_operation_monitor" => v(commands::cloud_commands::cloud_field_operation_monitor(app.state(), arg(&args, "req")?).await?)?,
        "cloud_login" => v(commands::cloud_commands::cloud_login(app.state(), arg(&args, "req")?).await?)?,
        "cloud_logout" => v(commands::cloud_commands::cloud_logout(app.state()).await?)?,
        "cloud_orders_by_date" => v(commands::cloud_commands::cloud_orders_by_date(app.state(), arg(&args, "req")?).await?)?,
        "cloud_package_orders" => v(commands::cloud_commands::cloud_package_orders(app.state(), arg(&args, "req")?).await?)?,
        "cloud_ping" => v(commands::cloud_commands::cloud_ping(app.state()).await?)?,
        "cloud_session" => v(commands::cloud_commands::cloud_session(app.state()).await?)?,
        "progress_set_dates" => v(commands::cloud_commands::progress_set_dates(app.state(), arg(&args, "dates")?).await?)?,
        "warehouse_create_package" => v(commands::cloud_commands::warehouse_create_package(app.state(), arg(&args, "req")?).await?)?,
        "warehouse_examine" => v(commands::cloud_commands::warehouse_examine(app.state(), arg(&args, "req")?).await?)?,
        "warehouse_label_data" => v(commands::cloud_commands::warehouse_label_data(app.state(), arg(&args, "req")?).await?)?,
        "warehouse_options" => v(commands::cloud_commands::warehouse_options(app.state()).await?)?,
        "warehouse_remove_goods" => v(commands::cloud_commands::warehouse_remove_goods(app.state(), arg(&args, "req")?).await?)?,
        "warehouse_remove_package" => v(commands::cloud_commands::warehouse_remove_package(app.state(), arg(&args, "req")?).await?)?,
        "get_config" => v(commands::config_commands::get_config(app.state()).await?)?,
        "get_pregen_status" => v(commands::config_commands::get_pregen_status(app.state()).await?)?,
        "pregen_clear_done" => v(commands::config_commands::pregen_clear_done(app.state()).await?)?,
        "pregen_done_snapshot" => v(commands::config_commands::pregen_done_snapshot(app.state()).await?)?,
        "pregen_mark_done" => v(commands::config_commands::pregen_mark_done(app.state(), arg(&args, "order_sns")?).await?)?,
        "update_config" => v(commands::config_commands::update_config(app.clone(), app.state(), arg(&args, "new_config")?).await?)?,
        "dispatch_provider_delete" => v(commands::dispatch_commands::dispatch_provider_delete(app.state(), arg(&args, "code")?).await?)?,
        "dispatch_provider_list" => v(commands::dispatch_commands::dispatch_provider_list(app.state()).await?)?,
        "dispatch_provider_upsert" => v(commands::dispatch_commands::dispatch_provider_upsert(app.state(), arg(&args, "req")?).await?)?,
        "network_health_check" => v(commands::health_commands::network_health_check(app.state()).await?)?,
        "network_health_get" => v(commands::health_commands::network_health_get(app.state()).await?)?,
        "daily_stats" => v(commands::log_commands::daily_stats(app.state(), arg(&args, "req")?).await?)?,
        "event_log_list" => v(commands::log_commands::event_log_list(app.state(), arg(&args, "req")?).await?)?,
        "ping" => v(commands::ping())?,
        "parcel_alert_list" => v(commands::parcel_alert_commands::parcel_alert_list(app.state(), arg(&args, "req")?).await?)?,
        "parcel_query_log_list" => v(commands::parcel_query_log_commands::parcel_query_log_list(app.state(), arg(&args, "req")?).await?)?,
        "print_stats_by_channel" => v(commands::print_stats_commands::print_stats_by_channel(app.state(), arg(&args, "req")?).await?)?,
        "print_stats_by_provider" => v(commands::print_stats_commands::print_stats_by_provider(app.state(), arg(&args, "req")?).await?)?,
        "print_stats_by_scanner" => v(commands::print_stats_commands::print_stats_by_scanner(app.state(), arg(&args, "req")?).await?)?,
        "print_stats_by_sticker" => v(commands::print_stats_commands::print_stats_by_sticker(app.state(), arg(&args, "req")?).await?)?,
        "print_stats_compare" => v(commands::print_stats_commands::print_stats_compare(app.state()).await?)?,
        "print_stats_daily" => v(commands::print_stats_commands::print_stats_daily(app.state(), arg(&args, "req")?).await?)?,
        "print_stats_failure" => v(commands::print_stats_commands::print_stats_failure(app.state(), arg(&args, "req")?).await?)?,
        "print_stats_heatmap" => v(commands::print_stats_commands::print_stats_heatmap(app.state(), arg(&args, "req")?).await?)?,
        "print_stats_hourly" => v(commands::print_stats_commands::print_stats_hourly(app.state()).await?)?,
        "print_stats_hourly_range" => v(commands::print_stats_commands::print_stats_hourly_range(app.state(), arg(&args, "req")?).await?)?,
        "print_stats_provider_source" => v(commands::print_stats_commands::print_stats_provider_source(app.state(), arg(&args, "req")?).await?)?,
        "print_stats_reprint" => v(commands::print_stats_commands::print_stats_reprint(app.state(), arg(&args, "req")?).await?)?,
        "print_stats_summary" => v(commands::print_stats_commands::print_stats_summary(app.state(), arg(&args, "req")?).await?)?,
        "work_session_reset" => v(commands::print_stats_commands::work_session_reset(app.state()).await?)?,
        "list_printers" => v(commands::printer_commands::list_printers(app.state()).await?)?,
        "print_image" => v(commands::printer_commands::print_image(app.state(), arg(&args, "req")?).await?)?,
        "warehouse_print_labels" => v(commands::printer_commands::warehouse_print_labels(app.state(), arg(&args, "req")?).await?)?,
        "queue_list" => v(commands::queue_commands::queue_list(app.state(), arg(&args, "req")?).await?)?,
        "queue_purge" => v(commands::queue_commands::queue_purge(app.state(), arg(&args, "req")?).await?)?,
        "queue_retry_failed" => v(commands::queue_commands::queue_retry_failed(app.state()).await?)?,
        "queue_stats" => v(commands::queue_commands::queue_stats(app.state()).await?)?,
        "local_lan_ips" => v(commands::server_commands::local_lan_ips(app.state()).await?)?,
        "server_restart" => v(commands::server_commands::server_restart(app.state(), app.clone()).await?)?,
        "server_status" => v(commands::server_commands::server_status(app.state()).await?)?,
        "sort_channel_list" => v(commands::sort_channel_commands::sort_channel_list(app.state()).await?)?,
        "sort_channel_save" => v(commands::sort_channel_commands::sort_channel_save(app.state(), arg(&args, "req")?).await?)?,
        "sort_channel_set_enabled" => v(commands::sort_channel_commands::sort_channel_set_enabled(app.state(), app.clone(), arg(&args, "position")?, arg(&args, "enabled")?).await?)?,
        "sort_channel_unassigned_get" => v(commands::sort_channel_commands::sort_channel_unassigned_get(app.state()).await?)?,
        "sort_channel_unassigned_save" => v(commands::sort_channel_commands::sort_channel_unassigned_save(app.state(), arg(&args, "code")?).await?)?,
        "sticker_history_add" => v(commands::sort_channel_commands::sticker_history_add(app.state(), arg(&args, "name")?).await?)?,
        "sticker_history_delete" => v(commands::sort_channel_commands::sticker_history_delete(app.state(), arg(&args, "name")?).await?)?,
        "sticker_history_list" => v(commands::sort_channel_commands::sticker_history_list(app.state()).await?)?,
        "web_auth_status" => v(commands::web_auth_commands::web_auth_status(app.state()).await?)?,
        "web_auth_set_password" => v(commands::web_auth_commands::web_auth_set_password(app.state(), arg(&args, "req")?).await?)?,

        // 兜底臂要獨佔一行:下面的 registry_sync 是逐行比對分派表與 generate_handler!，
        // 若有人照著上一行的樣子把新指令接在這行後面，那支會被守門測試靜默略過 ——
        // 而那個測試正是為了擋「桌面能用、網頁 404」而寫的。
        other => return Err(RpcError::UnknownCommand(other.to_string())),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camel_轉換對齊_tauri_傳參慣例() {
        assert_eq!(to_camel("new_config"), "newConfig");
        assert_eq!(to_camel("order_sns"), "orderSns");
        assert_eq!(to_camel("req"), "req");
    }

    #[test]
    fn arg_同時吃_snake_與_camel() {
        let args = serde_json::json!({ "newConfig": { "a": 1 } });
        let got: Value = arg(&args, "new_config").unwrap();
        assert_eq!(got, serde_json::json!({ "a": 1 }));

        let args = serde_json::json!({ "new_config": { "a": 2 } });
        let got: Value = arg(&args, "new_config").unwrap();
        assert_eq!(got, serde_json::json!({ "a": 2 }));
    }

    #[test]
    fn arg_缺席時_option_仍可解析() {
        let args = serde_json::json!({});
        let got: Option<String> = arg(&args, "code").unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn arg_缺席必填參數會回報參數名() {
        let args = serde_json::json!({});
        let got: Result<String, _> = arg(&args, "name");
        match got {
            Err(RpcError::BadArgs(m)) => assert!(m.contains("name"), "訊息應指出缺哪個參數: {m}"),
            _ => panic!("必填參數缺席時應回 BadArgs"),
        }
    }
}

/// 分派表與 Tauri 註冊清單的一致性守門。
///
/// 新增 command 時只改 `lib.rs` 的 `generate_handler!` 而忘了補 `dispatch`,
/// 症狀是桌面正常、網頁版該功能回 404 —— 而且要等實際點到那一頁才會發現。
/// 這裡在編譯期把兩份清單讀進來比對,漏補就直接測試失敗。
#[cfg(test)]
mod registry_sync {
    /// 從 `generate_handler![...]` 區塊抽出 command 名(取 `::` 後最後一段)
    fn tauri_registered() -> Vec<String> {
        let src = include_str!("../lib.rs");
        let start = src
            .find("generate_handler![")
            .expect("lib.rs 應有 generate_handler!");
        let rest = &src[start..];
        let end = rest.find(']').expect("generate_handler! 應有結尾 ]");
        rest[..end]
            .lines()
            .skip(1)
            .filter_map(|l| {
                let l = l.trim().trim_end_matches(',');
                if l.is_empty() || l.starts_with("//") {
                    return None;
                }
                l.rsplit("::").next().map(|s| s.to_string())
            })
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// 從 dispatch 的 match arm 抽出 command 名
    fn rpc_dispatched() -> Vec<String> {
        let src = include_str!("rpc.rs");
        src.lines()
            .filter_map(|l| {
                let l = l.trim();
                let rest = l.strip_prefix('"')?;
                let (name, after) = rest.split_once('"')?;
                after.trim_start().starts_with("=>").then(|| name.to_string())
            })
            .collect()
    }

    #[test]
    fn 每支_tauri_command_都有對應的_rpc_分派() {
        let registered = tauri_registered();
        let dispatched = rpc_dispatched();
        assert!(
            registered.len() > 50,
            "解析 generate_handler! 失敗,只抓到 {} 筆",
            registered.len()
        );

        let missing: Vec<_> = registered
            .iter()
            .filter(|c| !dispatched.contains(c))
            .collect();
        assert!(
            missing.is_empty(),
            "以下 command 只有桌面能用,網頁版會回 404 —— 請在 dispatch 補上分派:{missing:?}"
        );
    }

    #[test]
    fn rpc_分派沒有多出不存在的_command() {
        let registered = tauri_registered();
        let extra: Vec<_> = rpc_dispatched()
            .into_iter()
            .filter(|c| !registered.contains(c))
            .collect();
        assert!(extra.is_empty(), "分派表有未註冊的 command:{extra:?}");
    }
}
