use tauri::{AppHandle, State};

use crate::config::AppConfig;
use crate::{AppResult, SharedState};

/// 取得目前設定（不含敏感 token）
#[tauri::command]
pub async fn get_config(state: State<'_, SharedState>) -> AppResult<AppConfig> {
    Ok(state.config.read().await.clone())
}

/// 更新設定並持久化
#[tauri::command]
pub async fn update_config(
    handle: AppHandle,
    state: State<'_, SharedState>,
    new_config: AppConfig,
) -> AppResult<AppConfig> {
    // server.listen_ip / port 不是熱套用欄位(要重綁 socket)。先比對是否變更,
    // 變更則用新設定重啟 server —— start 會驗證新 addr 可綁,失敗就整個 update 中止、
    // 不持久化也不動其他設定,避免「設定存了卻沒生效」的斷鏈。
    // 需重綁 server 的欄位:listen_ip / port(換 socket)、存證目錄(/captures ServeDir)、
    // **快取目錄**(/images ServeDir 於 server 啟動時依 config 定版;不重啟會供舊目錄 →
    // http 模式工控機拿到的 label_path 全 404)。keep_days 變更不需重啟(清理器讀 config)。
    let (server_changed, camera_changed, sync_changed, cache_dir_changed) = {
        let cur = state.config.read().await;
        (
            cur.server.listen_ip != new_config.server.listen_ip
                || cur.server.port != new_config.server.port
                || cur.camera.captures_dir != new_config.camera.captures_dir
                || cur.cache.dir != new_config.cache.dir,
            cur.camera != new_config.camera,
            cur.sync != new_config.sync,
            cur.cache.dir != new_config.cache.dir,
        )
    };
    // 預檢:**僅 cache.dir 有變時**驗證新快取目錄可用(非受保護資料夾/磁碟根 + 可建立),
    // 失敗即整個 update 中止(不留「server 已換、設定已存、cache 套用失敗」斷鏈)。
    // 未變時不驗 —— legacy 壞目錄(runtime 已跑在 new() 的安全 fallback 上)不可
    // 卡死所有無關設定的儲存(預產排程/提示音/雲端 URL…全走本 command 全量寫入)。
    if cache_dir_changed {
        crate::cache::CacheManager::validate_dir(&handle, &new_config)?;
    }

    // cache 先套用、再重啟 server:兩個目錄消費者(下載寫檔 / /images ServeDir)才無間隙一致
    // —— 反過來會留「server 已供新目錄、下載仍寫舊目錄」的 404 競態窗。
    // apply_config 對目錄近乎不會失敗(有變已預檢、未變沿用現況),僅 build_http 可失敗即中止。
    // 若後續 restart 失敗:cache 已套新設定但 TOML 未存(重啟 App 即復原),非破壞性。
    state.cache.apply_config(&handle, &new_config)?;

    if server_changed {
        crate::commands::server_commands::restart_server(state.inner(), &new_config, handle.clone())
            .await?;
    }

    new_config.save(&handle).await?;

    state.cloud.apply_config(&new_config);
    state.health.apply_config(&new_config);
    state.label_resolver.apply_config(&new_config);
    // 讀碼站相機熱套用 —— **僅 camera 區塊有變才重啟擷取執行緒**:
    // apply_config 會清空最新幀重開相機,重啟窗內存證會漏拍(photo_path 永久 NULL);
    // 改無關設定(cloud timeout、cache keep_days …)不該中斷存證。
    if camera_changed {
        state.camera.apply_config(&new_config.camera);
    }
    // 件數核對跨機同步熱套用 —— 同理,僅 sync 區塊有變才重連:
    // 無條件 abort 重建 WebSocket 會讓無關設定儲存造成斷線窗、跨機廣播遺失。
    if sync_changed {
        state.sync.apply_config(&new_config.sync);
    }

    *state.config.write().await = new_config.clone();
    Ok(new_config)
}

/// 取得面單預產自動排程的可觀測狀態(排程啟動時間、上次執行結果)。
/// 前端據此顯示「上次執行 / 排程啟動」常駐狀態,確認排程是否正常運作。
#[tauri::command]
pub async fn get_pregen_status(
    state: State<'_, SharedState>,
) -> AppResult<crate::pregen::PregenStatus> {
    Ok(state.pregen_status.read().await.clone())
}

/// 面單預產「今日已預產的 order_sn」快照(cache_day + 清單)。
/// 手動頁批次開始時取回,在前端記憶體判斷略過 —— 與自動排程共用同一份(見 pregen::PregenDoneStore)。
#[derive(serde::Serialize)]
pub struct PregenDoneSnapshot {
    pub cache_day: String,
    pub order_sns: Vec<String>,
}

#[tauri::command]
pub async fn pregen_done_snapshot(
    state: State<'_, SharedState>,
) -> AppResult<PregenDoneSnapshot> {
    let (cache_day, order_sns) = state.pregen_done.snapshot(&state.db).await;
    Ok(PregenDoneSnapshot { cache_day, order_sns })
}

/// 手動頁標記一批 order_sn 為「今日已預產」(寫入共用去重 + DB)。
#[tauri::command]
pub async fn pregen_mark_done(
    state: State<'_, SharedState>,
    order_sns: Vec<String>,
) -> AppResult<()> {
    state.pregen_done.mark(&state.db, &order_sns).await
}

/// 清除「今日已預產」記憶(記憶體 + DB)。供手動頁「清除已預產記錄」與清空快取連帶呼叫。
#[tauri::command]
pub async fn pregen_clear_done(state: State<'_, SharedState>) -> AppResult<()> {
    state.pregen_done.clear(&state.db).await
}
