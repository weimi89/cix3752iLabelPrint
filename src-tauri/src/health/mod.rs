//! 網路健康狀態偵測:三層架構
//! - OS 網路層:由前端 navigator.onLine 偵測,不在此模組
//! - 公網層(anchor):對固定 host:port 發 TCP connect 試水
//! - 雲端 API 層:對 cloud.api_base + cloud.session_path 發 HEAD
//!
//! 特性:
//! - 動態 config(apply_config 可熱套用 interval / anchor / threshold)
//! - 抖動緩衝(連續失敗達 fail_threshold 才標 effective fail)
//! - 自動降頻(連續失敗時拉長到 degrade_interval,一旦成功立刻復原)
//! - 透過 Notify 可被前端 checkNow 立即觸發,打斷 sleep

use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::net::TcpStream;
use tokio::sync::Notify;

use crate::cloud::{CloudClient, CloudReachResult};
use crate::config::{AppConfig, NetworkConfig};

pub const NETWORK_STATUS_EVENT: &str = "network-status";

/// 一輪檢查的結果快照(emit 給前端的 payload)
#[derive(Debug, Clone, Serialize)]
pub struct NetHealthSnapshot {
    /// 公網 TCP 試水的原始結果
    pub anchor: LayerStatus,
    /// 雲端 API HEAD 的原始結果
    pub cloud_api: CloudReachResult,

    /// 公網連續失敗次數(成功歸 0);UI 顯示「重試 X/N 中」
    pub anchor_fail_streak: u32,
    /// 雲端連續失敗次數
    pub cloud_fail_streak: u32,

    /// 經抖動緩衝後的公網有效狀態(streak < threshold 時即使單次失敗仍為 true)
    pub anchor_effective_ok: bool,
    /// 經抖動緩衝後的雲端有效狀態;None 代表 not_configured(不算 ok 也不算 fail)
    pub cloud_effective_ok: Option<bool>,

    /// 當前生效的 fail_threshold,前端用來顯示「重試 X/N」
    pub fail_threshold: u32,
    /// 當前生效的巡檢間隔(秒),斷網時為 degrade_interval_secs
    pub effective_interval_secs: u64,

    /// epoch millis;前端用 Date(checked_at_ms) 渲染
    pub checked_at_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LayerStatus {
    Ok { latency_ms: u64 },
    Fail { error: String, latency_ms: u64 },
}

impl NetHealthSnapshot {
    fn placeholder(cfg: &NetworkConfig) -> Self {
        Self {
            anchor: LayerStatus::Fail {
                error: "尚未檢查".to_string(),
                latency_ms: 0,
            },
            cloud_api: CloudReachResult::NotConfigured,
            anchor_fail_streak: 0,
            cloud_fail_streak: 0,
            anchor_effective_ok: true,
            cloud_effective_ok: None,
            fail_threshold: cfg.fail_threshold,
            effective_interval_secs: cfg.interval_secs,
            checked_at_ms: 0,
        }
    }
}

/// 對外的網路健康檢查器
#[derive(Clone)]
pub struct HealthChecker {
    inner: Arc<Inner>,
}

struct Inner {
    cfg: RwLock<NetworkConfig>,
    cloud: CloudClient,
    app: AppHandle,
    snapshot: RwLock<NetHealthSnapshot>,
    /// 連續失敗計數(獨立於 snapshot 用於下一輪計算)
    anchor_fail_streak: RwLock<u32>,
    cloud_fail_streak: RwLock<u32>,
    /// 觸發立即檢查(中斷目前 sleep)
    trigger: Notify,
}

impl HealthChecker {
    pub fn new(cloud: CloudClient, app: AppHandle, cfg: NetworkConfig) -> Self {
        let snapshot = NetHealthSnapshot::placeholder(&cfg);
        Self {
            inner: Arc::new(Inner {
                cfg: RwLock::new(cfg),
                cloud,
                app,
                snapshot: RwLock::new(snapshot),
                anchor_fail_streak: RwLock::new(0),
                cloud_fail_streak: RwLock::new(0),
                trigger: Notify::new(),
            }),
        }
    }

    /// 啟動 worker;首輪立即執行
    pub fn start_worker(&self) {
        let this = self.clone();
        tokio::spawn(async move {
            // 啟動先跑一輪,讓前端立即拿到合理快照
            this.check_now().await;
            loop {
                let interval = this.next_interval();
                tokio::select! {
                    _ = tokio::time::sleep(interval) => {}
                    _ = this.inner.trigger.notified() => {}
                }
                this.check_now().await;
            }
        });
    }

    /// 套用新 config(熱更新),並觸發一次立即檢查
    pub fn apply_config(&self, cfg: &AppConfig) {
        *self.inner.cfg.write() = cfg.network.clone();
        self.inner.trigger.notify_one();
    }

    /// 立即跑一輪檢查;結果寫入 snapshot 並 emit Tauri event
    pub async fn check_now(&self) -> NetHealthSnapshot {
        let (anchor_addr, anchor_timeout_ms, cloud_timeout_secs, threshold) = {
            let c = self.inner.cfg.read();
            (
                c.anchor_addr.clone(),
                c.anchor_timeout_ms,
                c.cloud_timeout_secs,
                c.fail_threshold,
            )
        };

        let anchor_fut = check_anchor(&anchor_addr, anchor_timeout_ms);
        let cloud_fut = self.inner.cloud.head_session(cloud_timeout_secs);
        let (anchor, cloud_api) = tokio::join!(anchor_fut, cloud_fut);

        // 更新連續失敗計數
        let anchor_fail_streak = {
            let mut s = self.inner.anchor_fail_streak.write();
            match &anchor {
                LayerStatus::Ok { .. } => *s = 0,
                LayerStatus::Fail { .. } => *s = s.saturating_add(1),
            }
            *s
        };
        let cloud_fail_streak = {
            let mut s = self.inner.cloud_fail_streak.write();
            match &cloud_api {
                CloudReachResult::Reachable { .. } | CloudReachResult::NotConfigured => *s = 0,
                CloudReachResult::Unreachable { .. } => *s = s.saturating_add(1),
            }
            *s
        };

        // 抖動緩衝:streak < threshold 時即使這次失敗仍視為有效 ok
        let anchor_effective_ok = !matches!(anchor, LayerStatus::Fail { .. })
            || anchor_fail_streak < threshold;
        let cloud_effective_ok = match &cloud_api {
            CloudReachResult::Reachable { .. } => Some(true),
            CloudReachResult::NotConfigured => None,
            CloudReachResult::Unreachable { .. } => {
                Some(cloud_fail_streak < threshold)
            }
        };

        // 決定下次間隔(寫進 snapshot 給前端顯示;next_interval 也讀同一份)
        let effective_interval_secs = {
            let c = self.inner.cfg.read();
            if !anchor_effective_ok || matches!(cloud_effective_ok, Some(false)) {
                c.degrade_interval_secs.max(c.interval_secs)
            } else {
                c.interval_secs
            }
        };

        let snapshot = NetHealthSnapshot {
            anchor,
            cloud_api,
            anchor_fail_streak,
            cloud_fail_streak,
            anchor_effective_ok,
            cloud_effective_ok,
            fail_threshold: threshold,
            effective_interval_secs,
            checked_at_ms: chrono::Utc::now().timestamp_millis(),
        };

        *self.inner.snapshot.write() = snapshot.clone();

        if let Err(e) = self.inner.app.emit(NETWORK_STATUS_EVENT, snapshot.clone()) {
            tracing::warn!(?e, "emit network-status 失敗");
        }

        snapshot
    }

    /// 讀目前 snapshot(不觸發新檢查)
    pub fn snapshot(&self) -> NetHealthSnapshot {
        self.inner.snapshot.read().clone()
    }

    /// 給 worker 用:依目前狀態決定下次 sleep 時間
    fn next_interval(&self) -> Duration {
        let secs = self.inner.snapshot.read().effective_interval_secs.max(5);
        Duration::from_secs(secs)
    }
}

/// 對 anchor host:port 發 TCP connect 試水。
/// timeout 內成功 = 公網通;超時 / 拒絕 / unreachable = 公網斷
async fn check_anchor(addr: &str, timeout_ms: u64) -> LayerStatus {
    let started = Instant::now();
    let connect_fut = TcpStream::connect(addr);
    match tokio::time::timeout(Duration::from_millis(timeout_ms), connect_fut).await {
        Ok(Ok(_stream)) => LayerStatus::Ok {
            latency_ms: started.elapsed().as_millis() as u64,
        },
        Ok(Err(e)) => LayerStatus::Fail {
            error: e.to_string(),
            latency_ms: started.elapsed().as_millis() as u64,
        },
        Err(_) => LayerStatus::Fail {
            error: format!("timeout {timeout_ms}ms"),
            latency_ms: timeout_ms,
        },
    }
}
