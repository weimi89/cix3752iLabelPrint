mod cache;
mod cloud;
mod commands;
mod config;
mod db;
mod error;
mod health;
mod log;
mod models;
mod printer;
mod queue;
mod server;
mod watermark;

use std::sync::Arc;

use tauri::Manager;
use tokio::sync::RwLock;

pub use error::{AppError, AppResult};

/// 全域應用狀態,所有 Tauri command / 背景 worker / HTTP handler 共享
pub struct AppState {
    pub config: RwLock<config::AppConfig>,
    pub db: db::DbPool,
    pub cloud: cloud::CloudClient,
    pub cache: cache::CacheManager,
    pub queue: queue::QueueManager,
    pub server: RwLock<server::ServerHandle>,
    pub health: health::HealthChecker,
    pub label_resolver: server::LabelPathResolver,
    pub watermark: watermark::WatermarkRenderer,
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
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let handle = app.handle().clone();

            // 同步等 bootstrap 完成才開放 invoke handler,避免前端先呼叫到
            // 「state not managed」的競態
            let state = tauri::async_runtime::block_on(bootstrap(handle.clone()))
                .map_err(|e| {
                    tracing::error!(?e, "應用啟動失敗");
                    Box::<dyn std::error::Error>::from(e.to_string())
                })?;
            handle.manage(state);
            tracing::info!("應用啟動完成");

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
            commands::queue_commands::queue_list,
            commands::queue_commands::queue_retry_failed,
            commands::queue_commands::queue_purge,
            commands::cache_commands::cache_stats,
            commands::cache_commands::cache_clear,
            commands::log_commands::event_log_list,
            commands::log_commands::daily_stats,
            commands::cloud_commands::cloud_ping,
            commands::cloud_commands::cloud_login,
            commands::cloud_commands::cloud_logout,
            commands::cloud_commands::cloud_session,
            commands::cloud_commands::cloud_fetch_label,
            commands::cloud_commands::cloud_fetch_cloud_print,
            commands::cloud_commands::cloud_examine_package,
            commands::dispatch_commands::dispatch_provider_list,
            commands::dispatch_commands::dispatch_provider_upsert,
            commands::dispatch_commands::dispatch_provider_delete,
            commands::sort_channel_commands::sort_channel_list,
            commands::sort_channel_commands::sort_channel_save,
            commands::sort_channel_commands::sticker_history_list,
            commands::sort_channel_commands::sticker_history_delete,
            commands::health_commands::network_health_get,
            commands::health_commands::network_health_check,
            commands::parcel_query_log_commands::parcel_query_log_list,
            commands::print_stats_commands::print_stats_summary,
            commands::print_stats_commands::print_stats_daily,
            commands::print_stats_commands::print_stats_hourly,
            commands::print_stats_commands::print_stats_by_provider,
            commands::print_stats_commands::print_stats_by_sticker,
            commands::print_stats_commands::print_stats_by_scanner,
            commands::print_stats_commands::print_stats_heatmap,
            commands::print_stats_commands::print_stats_reprint,
            commands::print_stats_commands::print_stats_provider_source,
            commands::print_stats_commands::print_stats_failure,
            commands::print_stats_commands::print_stats_compare,
            commands::print_stats_commands::work_session_reset,
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

    let label_resolver = server::LabelPathResolver::new(&app_config);
    let watermark = watermark::WatermarkRenderer::new();

    let server_handle = server::start(
        &app_config,
        db_pool.clone(),
        cloud.clone(),
        cache.clone(),
        queue.clone(),
        label_resolver.clone(),
        watermark.clone(),
        handle.clone(),
    )
    .await?;

    let health = health::HealthChecker::new(
        cloud.clone(),
        handle.clone(),
        app_config.network.clone(),
    );
    health.start_worker();

    Ok(Arc::new(AppState {
        config: RwLock::new(app_config),
        db: db_pool,
        cloud,
        cache,
        queue,
        server: RwLock::new(server_handle),
        health,
        label_resolver,
        watermark,
    }))
}
