//! 事件的網頁出口:把 `event_bridge` 匯流排轉成 SSE。
//!
//! 桌面前端用 Tauri 的 `listen('xxx')`,網頁前端沒有那條進程內通道,
//! 改為訂閱 `/events/stream`,由 `src/api/events.js` 包裝成同樣介面。
//! 頁面程式碼因此兩邊共用,不必為網頁另寫一套輪詢。

use std::net::SocketAddr;

use axum::{
    extract::{ConnectInfo, Query, State},
    http::HeaderMap,
    response::sse::{Event, KeepAlive, Sse},
    response::IntoResponse,
};
use serde::Deserialize;
use tokio::sync::broadcast;

/// 長連線多久回頭確認一次還能不能繼續收。
/// 取 60 秒:夠短,換密碼或收窄網段後最多再收一分鐘;也夠長,不會為了長連線一直查資料庫。
const RECHECK_SECS: u64 = 60;

use crate::event_bridge::{self, BridgedEvent};

#[derive(Debug, Deserialize)]
pub(super) struct StreamQuery {
    /// 只要這幾個事件(逗號分隔);省略代表全收。
    /// 看板這類單一用途的頁面用它把無關事件擋在連線之外,不必在瀏覽器端過濾。
    #[serde(default)]
    only: Option<String>,
}

/// `GET /events/stream` —— 網頁端的事件總管道
pub(super) async fn events_stream(
    State(state): State<super::ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(q): Query<StreamQuery>,
) -> impl IntoResponse {
    let recheck = (state.clone(), peer.ip(), super::auth::stream_token(&headers));
    let filter: Option<Vec<String>> = q.only.map(|s| {
        s.split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect()
    });

    sse_from(event_bridge::subscribe(), state.close_tx.subscribe(), recheck, move |ev| {
        let wanted = filter
            .as_ref()
            .is_none_or(|f| f.contains(&ev.event));
        wanted.then(|| Event::default().json_data(ev).ok()).flatten()
    })
}

/// `GET /board/stream` —— 看板專用,只推 `sort-board` 的 payload。
///
/// 格式與匯流排統一前一致(直接是 BoardEvent 的 JSON,不含 event 名),
/// 既有的網頁看板不必跟著改。
pub(super) async fn board_stream(
    State(state): State<super::ServerState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let recheck = (state.clone(), peer.ip(), super::auth::stream_token(&headers));

    sse_from(event_bridge::subscribe(), state.close_tx.subscribe(), recheck, |ev| {
        (ev.event == "sort-board")
            .then(|| Event::default().json_data(&ev.payload).ok())
            .flatten()
    })
}

/// 共用的 SSE 骨架:訂閱匯流排,逐則套用 `pick` 決定要不要送出。
///
/// `close_rx` 收到通知就結束這條串流。SSE 本身永遠不會自己結束,而 axum 的
/// graceful shutdown 會等所有連線收工 —— 少了這條路,只要有人開著網頁看板,
/// 「重啟伺服器」就會一直等下去(見 ServerHandle::shutdown)。
fn sse_from<F>(
    rx: broadcast::Receiver<BridgedEvent>,
    close_rx: broadcast::Receiver<()>,
    recheck: (super::ServerState, std::net::IpAddr, String),
    pick: F,
) -> impl IntoResponse
where
    F: Fn(&BridgedEvent) -> Option<Event> + Send + Sync + 'static,
{
    let pick = std::sync::Arc::new(pick);
    let recheck = std::sync::Arc::new(recheck);
    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(RECHECK_SECS));
    ticker.reset();

    let stream = futures::stream::unfold(
        (rx, close_rx, ticker),
        move |(mut rx, mut close_rx, mut ticker)| {
        let pick = pick.clone();
        let recheck = recheck.clone();
        async move {
            loop {
                tokio::select! {
                    // server 要關了,主動收線
                    _ = close_rx.recv() => return None,
                    // 定期回頭確認這條連線還能不能收(內外網與 session 都重算)
                    _ = ticker.tick() => {
                        let (st, ip, token) = recheck.as_ref();
                        if !super::auth::stream_still_allowed(st, *ip, token).await {
                            tracing::info!(%ip, "事件串流已不再被允許,收線");
                            return None;
                        }
                    }
                    got = rx.recv() => match got {
                        Ok(ev) => {
                            if let Some(sse) = pick(&ev) {
                                return Some((
                                    Ok::<_, std::convert::Infallible>(sse),
                                    (rx, close_rx, ticker),
                                ));
                            }
                            // 不是這條連線要的事件,繼續等下一則
                        }
                        // 落後時只補最新的,舊事件直接丟 —— 現場要看的是「現在這件」
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(broadcast::error::RecvError::Closed) => return None,
                    },
                }
            }
        }
    },
    );

    Sse::new(stream).keep_alive(KeepAlive::default())
}
