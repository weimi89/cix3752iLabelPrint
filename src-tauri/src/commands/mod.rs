pub mod cache_commands;
pub mod cloud_commands;
pub mod config_commands;
pub mod log_commands;
pub mod printer_commands;
pub mod queue_commands;
pub mod server_commands;

/// 健康檢查 — 給前端確認 IPC 通了
#[tauri::command]
pub fn ping() -> &'static str {
    "pong"
}
