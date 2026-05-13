mod cache;
mod cloud;
mod commands;
mod config;
mod db;
mod error;
mod log;
mod models;
mod printer;
mod queue;
mod server;

use std::sync::Arc;

use tauri::Manager;
use tokio::sync::RwLock;

pub use error::{AppError, AppResult};

/// 全域應用狀態，所有 Tauri command / 背景 worker / HTTP handler 共享
pub struct AppState {
    pub config: RwLock<config::AppConfig>,
    pub db: db::DbPool,
    pub cloud: cloud::CloudClient,
    pub cache: cache::CacheManager,
    pub queue: queue::QueueManager,
    pub server: RwLock<server::ServerHandle>,
}

pub type SharedState = Arc<AppState>;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    log::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let handle = app.handle().clone();

            tauri::async_runtime::spawn(async move {
                match bootstrap(handle.clone()).await {
                    Ok(state) => {
                        handle.manage(state);
                        tracing::info!("應用啟動完成");
                    }
                    Err(e) => {
                        tracing::error!(?e, "應用啟動失敗");
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::config_commands::get_config,
            commands::config_commands::update_config,
            commands::printer_commands::list_printers,
            commands::printer_commands::print_image,
            commands::server_commands::server_status,
            commands::server_commands::server_restart,
            commands::queue_commands::queue_stats,
            commands::cloud_commands::cloud_ping,
            commands::cloud_commands::cloud_login,
            commands::cloud_commands::cloud_logout,
            commands::cloud_commands::cloud_session,
            commands::cloud_commands::cloud_fetch_label,
        ])
        .run(tauri::generate_context!())
        .expect("執行 Tauri 應用時發生未預期錯誤");
}

/// 啟動初始化流程：載入設定、開啟 DB、跑 migration、啟動 HTTP server 與背景 worker
async fn bootstrap(handle: tauri::AppHandle) -> AppResult<SharedState> {
    let app_config = config::AppConfig::load(&handle).await?;
    let db_pool = db::init(&handle).await?;
    let cloud = cloud::CloudClient::new(&app_config);
    let cache = cache::CacheManager::new(&handle, &app_config, db_pool.clone())?;
    let queue = queue::QueueManager::new(db_pool.clone(), cloud.clone());

    queue.start_worker();
    cache.start_cleaner();

    let server_handle = server::start(
        &app_config,
        db_pool.clone(),
        cloud.clone(),
        cache.clone(),
        queue.clone(),
    )
    .await?;

    Ok(Arc::new(AppState {
        config: RwLock::new(app_config),
        db: db_pool,
        cloud,
        cache,
        queue,
        server: RwLock::new(server_handle),
    }))
}
