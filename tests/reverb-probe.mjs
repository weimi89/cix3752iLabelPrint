// 件數核對跨機同步 — Reverb 廣播探針(零依賴,需 Node 18+ 內建 WebSocket;Node 22+ 穩定)
//
// 用途:訂閱某袋頻道,即時印出雲端發來的 parcel.printed 事件,用來驗 P1 雲端廣播有沒有送出。
// 只訂閱、不發佈,不會污染任何資料。
//
// host / key 不寫死(本檔為公開 repo),由參數或環境變數提供:
//   REVERB_HOST=<host> REVERB_KEY=<app_key> node tests/reverb-probe.mjs bag.XXXXXXXX
//   或  node tests/reverb-probe.mjs bag.XXXXXXXX <host> <app_key>
//   其他可選環境變數:REVERB_PORT(預設 443)、REVERB_SCHEME(wss/ws)
// host / app_key 請向部署負責人索取(對應該環境的 Reverb 與廣播 app)。

const channel = process.argv[2];
const host = process.argv[3] || process.env.REVERB_HOST;
const key = process.argv[4] || process.env.REVERB_KEY;
const port = process.env.REVERB_PORT || 443;
const scheme = (port == 443 || process.env.REVERB_SCHEME === 'wss') ? 'wss' : 'ws';

if (!channel || !host || !key) {
  console.error('用法: REVERB_HOST=<host> REVERB_KEY=<app_key> node tests/reverb-probe.mjs <channel,例 bag.XXXXXXXX>');
  console.error('      (host / app_key 也可改用第 3、4 個參數傳入)');
  process.exit(1);
}

const url = `${scheme}://${host}:${port}/app/${key}?protocol=7&client=probe&version=1.0`;
const ts = () => new Date().toLocaleTimeString('zh-TW', { hour12: false });

function connect() {
  console.log(`[${ts()}] 連線 ${url}`);
  const ws = new WebSocket(url);

  ws.addEventListener('open', () => console.log(`[${ts()}] WS 已開,等 connection_established…`));

  ws.addEventListener('message', ev => {
    let msg;
    try { msg = JSON.parse(ev.data); } catch { return; }
    switch (msg.event) {
      case 'pusher:connection_established':
        ws.send(JSON.stringify({ event: 'pusher:subscribe', data: { channel } }));
        console.log(`[${ts()}] => 訂閱 ${channel}`);
        break;
      case 'pusher_internal:subscription_succeeded':
        console.log(`[${ts()}] ✅ 訂閱成功 ${channel} — 監聽中,去出單看看…`);
        break;
      case 'pusher:ping':
        ws.send(JSON.stringify({ event: 'pusher:pong' }));
        break;
      case 'parcel.printed': {
        const d = typeof msg.data === 'string' ? JSON.parse(msg.data) : msg.data;
        console.log(`[${ts()}] 🎯 parcel.printed`, d);
        break;
      }
      case 'pusher:error':
        console.error(`[${ts()}] ❌ pusher:error`, msg.data);
        break;
    }
  });

  ws.addEventListener('close', () => {
    console.log(`[${ts()}] 連線關閉,3 秒後重連…`);
    setTimeout(connect, 3000);
  });
  ws.addEventListener('error', e => console.error(`[${ts()}] WS error:`, e.message || e));
}

connect();
