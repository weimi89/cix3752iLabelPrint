use serde::Serialize;
use tauri::{AppHandle, State};

use crate::config::AppConfig;
use crate::server;
use crate::{AppResult, SharedState};

/// 用指定設定重啟 server(先 start 新的綁定、成功才 swap 並關舊的)。
/// start 失敗(例:埠被占用、IP 無效)會回 Err 且不動既有 server,呼叫端據此判定套用失敗。
/// 供 `server_restart` 與 `update_config`(listen_ip/port 變更時自動重綁)共用。
pub async fn restart_server(
    state: &SharedState,
    config: &AppConfig,
    app: AppHandle,
) -> AppResult<()> {
    let new_handle = {
        let mut guard = state.server.write().await;
        let new = server::start(
            config,
            state.db.clone(),
            state.cloud.clone(),
            state.cache.clone(),
            state.queue.clone(),
            state.label_resolver.clone(),
            state.watermark.clone(),
            state.bag_check.clone(),
            app,
        )
        .await?;
        std::mem::replace(&mut *guard, new)
    };
    new_handle.shutdown().await;
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct ServerStatus {
    pub running: bool,
    pub bind_addr: String,
}

#[tauri::command]
pub async fn server_status(state: State<'_, SharedState>) -> AppResult<ServerStatus> {
    let guard = state.server.read().await;
    Ok(ServerStatus {
        running: true,
        bind_addr: guard.bind_addr.clone(),
    })
}

/// 重啟 server（套用新設定後呼叫）
#[tauri::command]
pub async fn server_restart(
    state: State<'_, SharedState>,
    app: AppHandle,
) -> AppResult<ServerStatus> {
    let config = state.config.read().await.clone();
    restart_server(state.inner(), &config, app).await?;

    let guard = state.server.read().await;
    Ok(ServerStatus {
        running: true,
        bind_addr: guard.bind_addr.clone(),
    })
}

/// 本機一張網卡的 LAN IPv4
#[derive(Debug, Serialize)]
pub struct LanIp {
    /// 網卡名稱(en0 / 乙太網路 …)
    pub name: String,
    pub ip: String,
}

/// 本機可供工控機連線的 LAN 位址清單 + server port
#[derive(Debug, Serialize)]
pub struct LanIpsResult {
    pub ips: Vec<LanIp>,
    pub port: u16,
}

/// 列出本機所有網卡的 LAN IPv4(排除 loopback / link-local / 0.0.0.0),
/// 工控機需以其中一個 `ip:port` 連線(server 監聽 0.0.0.0 無法直接被連)。
#[tauri::command]
pub async fn local_lan_ips(state: State<'_, SharedState>) -> AppResult<LanIpsResult> {
    let port = state.config.read().await.server.port;
    let mut ips = Vec::new();
    if let Ok(ifas) = local_ip_address::list_afinet_netifas() {
        for (name, ip) in ifas {
            if let std::net::IpAddr::V4(v4) = ip {
                if v4.is_loopback() || v4.is_link_local() || v4.is_unspecified() {
                    continue;
                }
                ips.push(LanIp {
                    name,
                    ip: v4.to_string(),
                });
            }
        }
    }
    Ok(LanIpsResult { ips, port })
}
