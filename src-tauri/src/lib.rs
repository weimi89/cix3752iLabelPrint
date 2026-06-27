mod bag_check;
mod cache;
mod camera;
mod cloud;
mod commands;
mod config;
mod db;
mod error;
pub mod error_label;
pub mod event_log;
mod health;
mod log;
mod models;
mod pregen;
mod printer;
mod queue;
mod server;
mod sync;
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
    /// HTTP server handle;啟動時若 port 被占用會是 None —— App 仍可開、server 顯示未啟動,
    /// 排除占用後可從設定頁手動重啟,而非讓綁定失敗整個 App panic(見 bootstrap 與 server_status)
    pub server: RwLock<Option<server::ServerHandle>>,
    pub health: health::HealthChecker,
    pub label_resolver: server::LabelPathResolver,
    pub watermark: watermark::WatermarkRenderer,
    pub bag_check: bag_check::BagCheckState,
    pub camera: camera::CameraManager,
    pub sync: sync::SyncManager,
    /// 面單預產自動排程的可觀測狀態(啟動 / 上次執行),供前端 get_pregen_status 查詢
    pub pregen_status: RwLock<pregen::PregenStatus>,
}

pub type SharedState = Arc<AppState>;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    log::init();

    // 安裝 rustls CryptoProvider(件數核對同步的 wss 連線需要;用 ring 對齊 reqwest/sqlx,單一 provider)。
    // reqwest/sqlx 若已自行安裝則此呼叫回 Err,忽略即可。
    let _ = rustls::crypto::ring::default_provider().install_default();

    tauri::Builder::default()
        // single-instance 必須最先註冊:第二個實例啟動時聚焦既有視窗後正常退出,
        // 從源頭擋掉「重複啟動自己搶不到 18080 → bootstrap 回 Err → did_finish_launching panic」的反覆崩潰
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
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
            commands::config_commands::get_pregen_status,
            commands::camera_commands::camera_set_zoom,
            commands::camera_commands::camera_capture_now,
            commands::printer_commands::list_printers,
            commands::printer_commands::print_image,
            commands::server_commands::server_status,
            commands::server_commands::server_restart,
            commands::server_commands::local_lan_ips,
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
            commands::cloud_commands::cloud_package_orders,
            commands::cloud_commands::cloud_orders_by_date,
            commands::cloud_commands::cloud_clearance_options,
            commands::cloud_commands::cloud_clearance_progress,
            commands::cloud_commands::progress_set_dates,
            commands::cloud_commands::cloud_clearance_store,
            commands::cloud_commands::cloud_clearance_dispatch,
            commands::dispatch_commands::dispatch_provider_list,
            commands::dispatch_commands::dispatch_provider_upsert,
            commands::dispatch_commands::dispatch_provider_delete,
            commands::sort_channel_commands::sort_channel_list,
            commands::sort_channel_commands::sort_channel_save,
            commands::sort_channel_commands::sort_channel_set_enabled,
            commands::sort_channel_commands::sort_channel_unassigned_get,
            commands::sort_channel_commands::sort_channel_unassigned_save,
            commands::sort_channel_commands::sticker_history_list,
            commands::sort_channel_commands::sticker_history_add,
            commands::sort_channel_commands::sticker_history_delete,
            commands::parcel_alert_commands::recent_parcel_alerts,
            commands::health_commands::network_health_get,
            commands::health_commands::network_health_check,
            commands::parcel_query_log_commands::parcel_query_log_list,
            commands::print_stats_commands::print_stats_summary,
            commands::print_stats_commands::print_stats_daily,
            commands::print_stats_commands::print_stats_hourly,
            commands::print_stats_commands::print_stats_by_provider,
            commands::print_stats_commands::print_stats_by_sticker,
            commands::print_stats_commands::print_stats_by_scanner,
            commands::print_stats_commands::print_stats_by_channel,
            commands::print_stats_commands::print_stats_heatmap,
            commands::print_stats_commands::print_stats_reprint,
            commands::print_stats_commands::print_stats_provider_source,
            commands::print_stats_commands::print_stats_failure,
            commands::print_stats_commands::print_stats_compare,
            commands::print_stats_commands::work_session_reset,
            commands::bag_check_commands::bag_check_snapshot,
            commands::bag_check_commands::bag_check_clear,
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
    let bag_check = bag_check::BagCheckState::new(cloud.clone(), handle.clone());

    // 件數核對跨機同步:訂閱 Reverb 廣播,讓別台處理的同袋包裹也即時更新本機核對清單。
    // 用 SyncManager 管理,設定頁可熱套用;未啟用(預設)則不連線,不影響既有單機流程。
    let sync = sync::SyncManager::new(bag_check.clone(), handle.clone());
    sync.apply_config(&app_config.sync);

    // 讀碼站快照相機:依設定常駐抓幀(enabled=false 則不啟動)
    let camera = camera::CameraManager::new();
    camera.start(&app_config.camera);

    // server 綁定失敗(例:18080 被占用)不致命:記錄 + emit 事件,App 照常開、server 顯示未啟動,
    // 排除占用後可從設定頁手動重啟(配合 single-instance,「重複啟動自己」這個最常見占用來源已被擋掉)
    let server_handle = match server::start(
        &app_config,
        db_pool.clone(),
        cloud.clone(),
        cache.clone(),
        queue.clone(),
        label_resolver.clone(),
        watermark.clone(),
        bag_check.clone(),
        camera.clone(),
        handle.clone(),
    )
    .await
    {
        Ok(h) => Some(h),
        Err(e) => {
            tracing::error!(?e, "HTTP server 啟動失敗(port 可能被占用),App 將以 server 未啟動狀態繼續");
            use tauri::Emitter;
            let _ = handle.emit("server-bind-failed", e.to_string());
            None
        }
    };

    let health = health::HealthChecker::new(
        cloud.clone(),
        handle.clone(),
        app_config.network.clone(),
    );
    health.start_worker();

    let state = Arc::new(AppState {
        config: RwLock::new(app_config),
        db: db_pool,
        cloud,
        cache,
        queue,
        server: RwLock::new(server_handle),
        health,
        label_resolver,
        watermark,
        bag_check,
        camera,
        sync,
        pregen_status: RwLock::new(pregen::PregenStatus::default()),
    });

    // 面單預產自動排程 worker(需完整 AppState,故在此啟動)
    pregen::start_scheduler(state.clone());

    // 讀碼站存證清理器:每小時依「目前」camera.keep_days 清過期存證(讀 RwLock 自動跟上設定變更)。
    // 刻意獨立於面單快取清理器 —— 存證壽命由 camera.keep_days 單獨決定,不受 cache.keep_days 影響。
    {
        let state = state.clone();
        let handle = handle.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                let (dir, keep_days) = {
                    let cfg = state.config.read().await;
                    (cfg.resolved_captures_dir(&handle).ok(), cfg.camera.keep_days)
                };
                if let Some(dir) = dir {
                    let _ = tauri::async_runtime::spawn_blocking(move || {
                        camera::cleanup_captures(&dir, keep_days)
                    })
                    .await;
                }
            }
        });
    }

    Ok(state)
}
