//! 事件雙軌出口:同一則事件同時送桌面(Tauri IPC)與網頁(SSE)。
//!
//! 桌面前端 `listen('xxx')` 走 Tauri 進程內通道;網頁前端沒有這條通道,
//! 改由 `/events/stream` 訂閱。兩邊要看到同一份事件,唯一可靠的作法是
//! **所有 emit 都走這裡** —— 若各處自行呼叫 `app.emit()`,新增的事件會
//! 只有桌面收得到,而且要等使用者回報「網頁上這個沒更新」才會發現。
//!
//! 匯流排刻意做成進程唯一的靜態值:事件本來就不屬於任何一份狀態,
//! 綁進 `AppState` 只會讓每個 emit 呼叫點都得多拿一個參數。

use once_cell::sync::Lazy;
use serde::Serialize;
use tokio::sync::broadcast;

/// 送往網頁端的事件封包
#[derive(Clone, Debug, Serialize)]
pub struct BridgedEvent {
    /// 事件名,與桌面 `listen()` 用的完全相同
    pub event: String,
    pub payload: serde_json::Value,
}

/// 容量取 256:現場尖峰是連續刷件(每件數則事件),256 足以吸收瞬間爆量;
/// 網頁端跟不上時走 `Lagged` 丟舊事件,不阻塞發送端。
static BUS: Lazy<broadcast::Sender<BridgedEvent>> =
    Lazy::new(|| broadcast::channel(256).0);

/// 網頁端訂閱事件流
pub fn subscribe() -> broadcast::Receiver<BridgedEvent> {
    BUS.subscribe()
}

/// 發送事件到桌面與網頁兩端。
///
/// 回傳的是**桌面端**的結果,呼叫端原本怎麼處理 `app.emit()` 的錯誤就怎麼處理。
/// 網頁端沒有訂閱者時 `send` 會回 Err,那是正常狀態(沒人開網頁),不視為錯誤。
pub fn emit<S>(app: &tauri::AppHandle, event: &str, payload: S) -> tauri::Result<()>
where
    S: Serialize + Clone,
{
    let json = serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null);
    let _ = BUS.send(BridgedEvent {
        event: event.to_string(),
        payload: json,
    });

    tauri::Emitter::emit(app, event, payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn 訂閱者收得到送出的事件() {
        let mut rx = subscribe();
        let _ = BUS.send(BridgedEvent {
            event: "sort-board".into(),
            payload: serde_json::json!({ "position": "L1" }),
        });

        let got = rx.recv().await.expect("應收到事件");
        assert_eq!(got.event, "sort-board");
        assert_eq!(got.payload["position"], "L1");
    }

    #[tokio::test]
    async fn 沒有訂閱者時發送不會恐慌() {
        // 現場多數時間沒人開網頁,這條路徑每天會走上萬次
        let _ = BUS.send(BridgedEvent {
            event: "parcel-query-logged".into(),
            payload: serde_json::Value::Null,
        });
    }
}
