use serde::Serialize;
use tauri::{AppHandle, State};

use crate::config::AppConfig;
use crate::server;
use crate::{AppResult, SharedState};

/// 用指定設定重啟 server:**先關舊的釋放 port,再綁新的**。
///
/// 不可「先綁新的、成功才關舊的」—— 那樣在**同一個 port 重啟**(只改了存證目錄等設定、port 沒變)時,
/// 舊 server 還占著該 port,新的會撞 `Address already in use`。先關後綁讓「儲存設定觸發重啟」與
/// 「UI 手動重啟」在同 port 都能成功(listening socket 釋放後 SO_REUSEADDR 可立即重綁,無 TIME_WAIT 問題)。
/// 代價:若新的綁定失敗(例:新 IP/port 無效或被別的程式占),server 會處於未啟動狀態,呼叫端收到 Err;
/// 配合啟動時的「綁定失敗不致命」,App 仍可運作,使用者修正設定後可再重啟。
/// 供 `server_restart` 與 `update_config`(listen_ip / port / 存證目錄 變更時自動重綁)共用。
pub async fn restart_server(
    state: &SharedState,
    config: &AppConfig,
    app: AppHandle,
) -> AppResult<()> {
    let mut guard = state.server.write().await;
    // 先關舊 server 釋放 port(同 port 重啟的關鍵)
    if let Some(old) = guard.take() {
        old.shutdown().await;
    }
    // 再綁新的;失敗則 guard 維持 None(server 未啟動),Err 上拋
    let new = server::start(
        config,
        state.db.clone(),
        state.cloud.clone(),
        state.cache.clone(),
        state.queue.clone(),
        state.label_resolver.clone(),
        state.watermark.clone(),
        state.bag_check.clone(),
        state.camera.clone(),
        app,
    )
    .await?;
    *guard = Some(new);
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
    Ok(match guard.as_ref() {
        Some(h) => ServerStatus {
            running: true,
            bind_addr: h.bind_addr.clone(),
        },
        None => {
            // server 未綁定(啟動時 port 被占):回未啟動 + 設定中的目標位址供 UI 顯示與重試
            let cfg = state.config.read().await;
            ServerStatus {
                running: false,
                bind_addr: format!("{}:{}", cfg.server.listen_ip, cfg.server.port),
            }
        }
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
        running: guard.is_some(),
        bind_addr: guard
            .as_ref()
            .map(|h| h.bind_addr.clone())
            .unwrap_or_else(|| format!("{}:{}", config.server.listen_ip, config.server.port)),
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
