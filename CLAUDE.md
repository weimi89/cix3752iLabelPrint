# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 系統定位

**智配通 面單列印** 是一個跨平台 Tauri v2 桌面 App,定位為「分揀工控機 ↔ 雲端 API」之間的本地中介服務。三個世界共存於同一進程:

- **本地 HTTP server**(axum, 預設 `0.0.0.0:18080`)— 工控機 PLC 透過 HTTP 呼叫,四支 endpoint:`/healthz`、`GET /api/parcel/{queryNo}`、`POST /api/report`、`POST /api/device-alert`(設備異常語音廣播)
- **雲端 API client**(reqwest + Bearer Token)— 對外打雲端 Laravel API,Token 存 OS keyring
- **桌面 GUI**(Vue 3 / Vuetify)— 操作員 UI,透過 Tauri command 跟 Rust 後端 IPC

### 三大設計原則(動修改前先讀)

1. **不回 base64** — 面單一律回本機路徑或 `/images/{key}` URL,不傳檔案內容
2. **不讓工控機等雲端** — `POST /api/report` 立即回 200,雲端 webhook 推送由背景 worker 處理
3. **面單一律走本地** — 本地未命中時 Middleware 同步下載到完成,工控機統一存取點

## 常用指令

```bash
# 開發(Vite HMR + Rust 改動自動重編 + dev codesign)
npm run tauri:dev

# 打包(平台對應 .dmg / .exe / .deb / .AppImage)
npm run tauri:build

# Rust 端編譯檢查(快,不打包)— 改 Rust 後優先用
cd src-tauri && cargo check

# 前端純 build(不打包成桌面 App,用於 web preview)
npm run build && npm run preview

# 跨 Linux distro 本地驗證(需 OrbStack / Docker)
bash tests/docker-ubuntu-build.sh 22.04 install   # 或 20.04 / 24.04
```

### macOS 開發者首次設定

預設情況下,每次 `cargo build` 重編譯產生的 binary ad-hoc 簽章都不同,macOS keychain ACL 會失效導致**每次重編都跳「想存取 keychain」對話框**。解法:

```bash
security find-identity -p codesigning           # 列出可用 identity
echo 'export CIX3752I_DEV_SIGN_IDENTITY="<cert hash>"' >> ~/.zshrc
```

實作見 `.cargo/config.toml` + `scripts/dev-codesign-run.sh`(cargo runner wrapper 固定 codesign + 包 dev `.app` bundle 讓 Dock 顯示「智配通 面單列印」)。

## 架構大圖

### 後端(Rust)— `src-tauri/src/`

```
lib.rs                    AppState + bootstrap(initial migration / server start / worker spawn)
├── config/               TOML 設定檔(熱套用機制)
├── db/                   sqlx Pool + 編譯期 migrations
├── models/               共用資料結構(ParcelData / envelopes …)
├── server/               axum HTTP server + LabelPathResolver(local/share/http/direct_print 四模式)
│   ├── rpc.rs            網頁版資料通道:POST /rpc/{cmd} → 既有 Tauri command(分派表)
│   ├── events.rs         網頁版事件通道:GET /events/stream(SSE)
│   ├── auth.rs           存取控制:內網免登入 / 外網共用密碼 / 工控機端點只認內網
│   └── assets.rs         網頁版前端(編譯期嵌入 dist)+ SPA fallback
├── event_bridge.rs       事件雙軌出口:同一則事件同時送桌面(Tauri)與網頁(SSE)
├── cloud/                雲端 API client + LabelFetchMode(download/cloud_print/web_print)
├── cache/                面單快取(LRU 清理 + hit/miss 統計)
├── camera/               讀碼站相機(nokhwa 擷取 + MJPEG 預覽 + 快照存證 captures)
├── queue/                report_queue + background worker(指數退避 retry)
├── pregen/               面單預產(批次預下載 + pregen_done 去重單一來源)
├── bag_check/            分揀袋件核對(常駐記憶體清單 + 袋件連續性偵測)
├── sync/                 跨機同步(Reverb/WebSocket 訂閱:件數核對 / 清關進度即時廣播)
├── watermark.rs          列印次數浮水印(字型編譯期內嵌)
├── error_label.rs        錯誤面單提示圖產生(雲端業務錯誤時;工控機路徑由 `error_label.enabled` 開關控制)
├── printer/              系統印表機列舉與列印
├── health/               三層網路偵測(OS / Anchor / Cloud API)+ 抖動緩衝
├── log/ + event_log.rs   分類事件 log(category × level)
└── commands/             Tauri IPC commands(前端可呼叫的 API)
```

`AppState` 是 `Arc<...>` 的共享狀態,由所有 Tauri command、axum handler、background worker 透過 `tauri::State` / clone 取用。新增功能時通常先決定 state 該放在哪個 sub-module。

### 前端(Vue 3)— `src/`

```
pages/                    22 個功能頁(Dashboard / ScanPrint / AutoPrint / PreGenerate / BagCheck / SortChannels / ClearanceAdd / ClearanceDispatch / WarehouseScanner / PrintStats / ParcelQueryLog / ParcelAlertLog …)
components/               共用元件(AppNavbar / NetworkStatusIndicator / LocaleSwitcher …)
composables/              組合式邏輯(useNetworkStatus / useLabelStatus …)
stores/                   Pinia(status.js 集中管理 server/cloud/queue/cache/today/printStats)
config/navConfig.js       Sidebar 結構(主要 / 列印 / 日誌 / 設定 四群)
plugins/i18n/locales/     zh-Hant.json + vi-VN.json(雙語介面熱切換)
api/tauri.js              資料 API 單一出入口(桌面走 IPC / 網頁走 RPC / 預覽走 mock)
api/runtime.js            三種執行環境的判斷(桌面 / 網頁 / 純瀏覽器預覽)
api/rpc.js                網頁版的 HTTP 通道 + 401 導向掛勾
api/events.js             事件訂閱抽象,介面與 Tauri listen 相同(網頁走共用 EventSource)
api/media.js              面單圖 / 存證照 / 相機串流的網址(桌面絕對、網頁相對)
@core/ @layouts/          Materio Vuetify Admin 樣板基礎
```

### Rust ↔ Vue 通訊兩條路

1. **Request/Response** — 前端 `invoke('command_name', args)` 呼叫 Rust `#[tauri::command]`,await 結果
2. **Server Push (事件)** — Rust `event_bridge::emit(&app, 'event-name', payload)` → 前端 `listen('event-name', cb)`。
   **一律走 `event_bridge`,不要直接 `app.emit()`** —— 後者只送得到桌面,網頁端會靜默漏收。目前事件:
   - `print-stats-updated`(三個寫入點 emit,前端 `DefaultLayout` listen,Navbar chip + 儀表板毫秒級同步)
   - `network-status`(`HealthChecker` worker 每輪檢查結束 emit)
   - `parcel-alert`(`GET /api/parcel` 失敗 / NoRead 時 emit,前端 `useParcelAlert` 依 kind 播提示音 + toast;`noread` kind 只 toast 不出聲)
   - `device-alert`(`POST /api/device-alert` 收到設備異常時 emit,前端 `useDeviceAlert` 播中越雙語語音 + toast)
   - `bag-check-updated`(`bag_check` 清單每次變動時 emit 完整快照,前端 `BagCheckPage` 直接替換,不輪詢)
   - `parcel-query-logged`(`/api/parcel` 寫入查詢紀錄後 emit,前端請求記錄頁去抖後 reload)
   - 跨機同步:`sync` 訂閱雲端 Reverb/WebSocket 廣播(`ParcelPrinted` / 清關進度),套用到本機 `bag_check` / 清關浮動框

**「不夠即時就 WebSocket」是錯方向** — 桌面 App 後端與前端在同一進程,Tauri IPC event 走進程內通道、毫秒級、不用 socket server。WebSocket 適合「跨網路、跨機器」,在這裡反而繞遠路。

### 三種執行環境(`api/runtime.js`)

同一份前端跑在三個地方,靠**明確旗標**分辨,不用「試打後端失敗就退 mock」—— 後者會把真正的連線錯誤吞成假資料,讓壞掉的頁面看起來像正常的。

| 環境 | 判斷依據 | 資料來源 |
|---|---|---|
| 桌面 App | `window.__TAURI_INTERNALS__` | Tauri IPC |
| 網頁版 | `window.__CIX_WEB__`(後端 / dev plugin 注入) | HTTP RPC |
| 純瀏覽器預覽(`npm run preview`) | 兩者皆無 | `api/tauri.js` 內建 mock |

判斷式用 `hasBackend`(有後端可呼叫)或 `isTauriRuntime`(真的需要桌面能力,如開機自動啟動、選資料夾對話框、多螢幕開窗)。
兩者混用會出兩種錯:該用 `hasBackend` 卻寫 `isTauriRuntime` → 網頁版整頁空白;反過來 → 網頁版出現按了沒反應的開關。

新增 command 時要同步補三處:`generate_handler!`、`server/rpc.rs` 的分派表、`api/tauri.js` 的 wrapper(含 mock)。
**分派表漏補會讓桌面正常、網頁 404** —— `rpc.rs` 的 `registry_sync` 測試會直接擋下來。

開發網頁版:`yarn dev` 後用瀏覽器開 `http://localhost:11420`(Vite 已代理 `/rpc`、`/events` 等到 18080,有熱更新)。

## 重要設計細節

### 網頁版與對外存取(`server/auth.rs`、`config.web_access`)

桌面版的頁面都能在瀏覽器跑,用的是同一份 Vue 前端與同一批 Rust 業務邏輯 —— **不存在第二套 UI 或第二份業務邏輯**。

**例外:現場作業頁只在桌面版提供**(掃描列印、自動印單、入倉驗單 —— 接掃描槍、用這台機器的印表機出單)。
以 `desktopOnly: true` 標記,**導覽列(`config/navConfig.js`)與路由(`router/index.js`)兩處都要標**:
只標導覽列 → 網址打得進去;只標路由 → 導覽列看得到、點了卻被踢回首頁。兩處一致性由 `tests/maintenance-guards.test.mjs` 守著。

存取控制只有一道門,判斷**只認 TCP 連線的對端位址**:

| 來源 | 待遇 |
|---|---|
| 內網網段(`web_access.lan_cidrs`) | 免登入,完整權限 |
| 非內網 | 要輸入共用密碼(argon2 存 `app_setting`),通過後同等權限 |
| 非內網打工控機端點 | 一律 403,即使已登入 |
| 未設密碼 | 外網一律拒絕 |

**絕不採信 `X-Forwarded-For`** —— 這台機器直接對外、前面沒有反向代理,該標頭任何人都能自己填,
一旦拿來判斷內外網,外部送一行標頭就能繞過整道門。日後若真的擺代理在前面,要先確認代理會覆寫該標頭再改。

**目前沒有 TLS**:對外開放時密碼、session cookie、面單上的收件人資料都是明文傳輸。
接點已預留在 `server/mod.rs` 建立 listener 那段(換 `axum-server` + rustls),啟用後要把 `auth.rs` 的 cookie 補上 `Secure`。

**兩套網頁介面並存,對象不同**:`/`(完整網頁版)是給主管的後台,功能多、什麼都能改;
`/control` 是給**現場貼單人員**的簡單版手機遙控頁(獨立 HTML `server/control_page.html`,只有通道狀態與暫停 / 恢復 / 跳過),
桌面 App 的「手機遙控」QR 與網址指的是 `/control`。**不要把 /control 改成導向完整網頁版** —— 現場人員看不懂完整後台,也不該碰到設定頁。
`/board` 舊網址導向 Vue 分揀看板頁。

**手機版**:記錄類表格在窄螢幕會變成卡片式(`.table-cards`,樣式在 `styles/main.scss`),
欄名取自各 `<td>` 的 `data-label` —— **加新欄位時要一併補上**,否則手機上那格會沒有名稱。
畫面驗證用 `npx playwright@1.49.0 screenshot --channel=chrome`(用系統 Chrome,可開 localhost);
改到版面後跑 `node tests/mobile-layout-audit.mjs <輸出目錄> 390 vi-VN`(`yarn dev` 先開著)可一次截全部頁面並列出超出、被切的元素,
越南文字比中文長,**手機版一定要用 vi-VN 也跑一次**。

**改字級要注意的坑**:Vuetify 的字級 class(`text-display-*`、`text-headline-*` …)在 `main.scss` 是鎖在 `@layer vuetify-utilities` 的 `!important`,
分層的 `!important` 永遠壓過元件內未分層的 `!important`,元件裡的媒體查詢改不動它。手機要縮字的數字 / 標題不要掛這些 class,字級由元件自己寫。

**圖示離線化**:圖示資料打包在 `src/plugins/icons-offline.json`(只含實際用到的 176 個)。
沒有它,`@iconify/vue` 會去 `api.iconify.design` 線上抓 —— 外網一斷畫面上每個圖示都變空白。
**新增圖示後要跑 `yarn icons` 重產**,忘了會被 `tests/maintenance-guards.test.mjs` 擋下。

**跨檔案對應的守門**:`tests/maintenance-guards.test.mjs` 守著三件容易漏的事 ——
前端呼叫的 command 後端要有、表格欄名要對得上表頭、圖示要打包進來。
這類東西寫在註解裡遲早會被漏掉,而漏掉的症狀都不好聯想(網頁 404、手機某格沒名稱、圖示空白)。

### 面單路徑四模式(`label_path.mode`)

工控機讀面單四種拓撲:
- `local` — 回本機絕對路徑(同機部署)
- `share` — 回 SMB / NFS 共用目錄路徑(跨機 + 共用 NAS)
- `http` — 回 `http://{host}/images/{key}` URL(跨機,無檔案系統存取權)
- `direct_print` — **中介 PC 本機直接列印**,回應不含 `label_path`;圖檔下載 + 浮水印 + 送印全丟背景有序佇列(單一 FIFO worker,保證列印順序 = 請求順序、不並發打 spooler)

設定頁可熱切換,**不需重啟** server。實作在 `src-tauri/src/server/mod.rs` 的 `LabelPathResolver`(`direct_print` 走 `DirectPrintJob` 佇列,不經 resolver)。

### 印單統計即時推播

三個寫入點(`server/mod.rs` IPC、`cloud_commands.rs` scan/auto)寫完 `print_event` 後立即呼叫 `print_stats_commands::emit_print_stats_updated(&app, source, shipping_no)`,前端 `DefaultLayout` 透過 `listen('print-stats-updated')` 觸發 `status.refreshPrintStats()`,端到端 < 100ms。

系統狀態(server/queue/cache/today/cloud)仍走 5s 輪詢(`DefaultLayout.onMounted` 啟動,各頁讀 `useStatusStore()`)。

### 三層網路健康偵測

`src-tauri/src/health/mod.rs` 的 `HealthChecker` 每 15s(降級 60s)依序檢測:
1. **OS** — 網卡連線
2. **Anchor** — 公網 anchor host(預設 `1.1.1.1:443`)TCP 試連
3. **Cloud API** — 對 session endpoint 發 HEAD(帶 Bearer);401/403 標「未登入」、其他 4xx 標「業務錯誤」(降級為黃燈)、5xx 與連線失敗視為 Unreachable

支援抖動緩衝(連續失敗達 threshold 才標 down)。

### 列印次數浮水印

雲端回傳 `print_num > 1` 時,自動在面單右上角(順豐右下角)疊加 `(N)`。字型 **DejaVu Sans Bold**(OFL) 透過 `include_bytes!` 編譯期內嵌進 binary,**無須額外部署字型檔**。實作:`src-tauri/src/watermark.rs`。

### 讀碼站快照存證 + NoRead 處理(`GET /api/parcel`)

- **快照存證** — `get_parcel` 一收到請求就「釘住」`camera` 最新一幀(純記憶體複製,不擋回應),查得到訂單才丟背景寫檔到**存證目錄**(獨立於面單快取,`camera.keep_days` 單獨清理),回寫 `parcel_query_log.photo_path`,前端請求記錄頁以 `/captures/{key}` 檢視。相機由後端 `nokhwa` 獨佔,`/camera/preview/stream` 出 MJPEG 給設定頁預覽(預覽=存證同一畫面)。
- **NoRead(相機讀不到單號)** — 工控機以 `queryNo=NoRead`(正規化去符號小寫比對 `noread`,容錯 `NO_READ`/`no read`)呼叫時 `is_noread` 短路:**不打雲端**、仍拍照存證(檔名 `NoRead_{時間}_{進程序號}.jpg`,序號避免同秒覆蓋)、記 `parcel_query_log`(負數 `response_id`、`photo_path`,先寫 NULL 背景回填不阻塞回應)、`daily_stats` `request_count + noread_count`(不計 success)、emit `parcel-alert` kind=`noread`(前端只 toast 不出聲、空 message 用 i18n 標題)、回 200 `error_code:"NOREAD"` 無面單無通道。實作:`server/mod.rs` 的 `is_noread` / `handle_noread`。

### 異常件提示面單開關(`error_label.enabled`,預設關)

工控機 `GET /api/parcel` 遇雲端業務錯誤(門市關轉 / 未確認 / 訂單異常 / 查無訂單 / 非代寄 / 非轉寄 /
取單失敗 / 雲端不通等所有非 401 錯誤)時要不要出提示面單的總開關,設定頁「分揀通道」切換、熱套用不需重啟。

- **關閉(預設)** — 只回 `error_code` + `message`,`channel_code` / `print_profile` / `label_path` /
  `response_id` **全為 null**、`is_error_label=false`;工控機不列印、不回報,異常包裹自行走預設格口。
  `parcel_query_log` 仍寫一筆(`should_print=0`)不留診斷盲區。
- **開啟** — 回復舊行為:產提示圖 → 依面單路徑模式回 `label_path`(`direct_print` 走本機直印)、
  照正常面單流程解析分揀通道、`response_id` 給本地負數供工控機回報。

兩種狀態下 `parcel_alert`、`daily_stats`、件數核對(`bag_check`)皆照常 —— 包裹實體仍過機分揀。
**只作用於工控機路徑**;桌面掃描列印 / 自動印單(`cloud_commands.rs`)另有出口,不受此開關控制。

### 分揀通道分配:公平輪替 + 後兩碼迴避

`server/mod.rs` 的 `resolve_channel_code` 依物流商挑格口,兩條規則疊在「跳過本輪」額度之上。

- **公平輪替**:候選中挑**最久沒收件**的格口(平手照 L1→R5)。排序依據是格口實際收了什麼,**不分物流** ——
  一個格口同時掛多家物流時,剛被其中一家用掉就該讓給比較閒的格口,否則共用格口會比別人累。
  (舊版是每家物流各自一份輪替指標,共用格口會連續收件。)
- **後兩碼迴避**:輪到的格口若「上一件」配送單號後兩碼與本件相同就讓開 —— 現場貼單靠後兩碼認包裹,
  連兩張尾碼一樣會貼錯。「上一件」同樣不分物流。**前提**是該物流有 2 個以上可用通道,只有一格時無處可換。
- **只跑一輪**:繞完一圈每個候選都撞尾碼,就退讓回排序第一個 —— 寧可尾碼撞,不可沒格口。
- **額度不浪費**:因撞尾碼而讓過的通道**不消耗** skip_count,那是操作員按的「跳過本輪」,只該花在真的輪到它的那次。
- **狀態**:各格口的「最近收件序號 + 單號」共用同一把鎖(`SortRouting`);進程剛啟動時回查 `print_event`
  補值(序號取 `id`),否則重開 App 後剛收過件的格口會被當成整天沒收件、連著再收一件。

### 分揀袋件核對(常駐清單 + 袋件連續性偵測)

`bag_check/mod.rs` 維護袋件核對常駐清單(進程生命週期,切頁保留):新袋背景 `examine_package` 取整袋 manifest,舊袋就地更新列印時間,每次變動 emit `bag-check-updated`。

- **保留策略** — 有未印件(missing>0)的袋全留;已完成的袋只留最新一個(`prune`),避免剛補印完的舊袋瞬間消失。
- **袋件連續性偵測** — 追蹤 `active_bag`:成功查得換到不同袋號、且前一袋仍缺件 → 標前一袋 `interrupted`(前端紅色「中途被打斷」徽章)。NoRead / 雲端錯誤 / 散單不動 `active_bag`(= 連續不中斷,對齊「沒袋號含在連續次數內」)。**回補已完成袋(含已被 `prune` 淘汰、記於有界 `recently_completed`)不算開新袋**,不誤打斷;補齊(missing=0)`recount` 自動清旗標(最終補齊就算正確)。前一袋 entry 尚未建(背景 examine 未回)時記入有界 `abandoned`,待建立補標。
- **跨機同步** — `sync` 訂閱雲端 `bag.{package_sn}` 廣播,別台印的件即時套用到本機清單(僅本機已持有該袋時)。

### 設備異常雙語語音廣播(`POST /api/device-alert`)

工控機回報設備異常(卡包裹 / USB 斷線 / 掃描器 / 印表機故障)時,後端 `server/mod.rs` 的 `post_device_alert` **立即回 200**(不讓工控機等),emit `device-alert` 事件 `{ alert_type, message }`,前端 `useDeviceAlert` 廣播。設計細節:

- **預錄音檔,非即時 TTS** — 內建 5 種分類碼(`PARCEL_JAM` / `USB_DISCONNECT` / `SCANNER_ERROR` / `PRINTER_ERROR` / `ERROR`)的中越雙語語音**預先錄製**內嵌於 `public/sounds/alert/{type小寫}-zh.mp3` / `-vi.mp3`(中文 `HsiaoChen`、越南語 `HoaiMy` neural,以 edge-tts 產生)。**主因:Windows 工控機預設無越南語語音包**,即時 TTS 唸不出越南語且音色不可控;預錄內嵌 → 每台音色一致、離線可用、越南語免裝語音包。
- **未知 type 才退回 TTS** — 工控機傳未錄音的自訂 `type` 時,`useDeviceAlert` 退回 `useSpeech`(瀏覽器 `speechSynthesis`),此時越南語仍需機器自備語音包。
- **`type` 大寫正規化** — 後端 `to_uppercase()`,工控機傳大小寫皆可,canonical 一律大寫(對齊雲端機器碼風格)。
- **固定廣播一次** — 每次呼叫雙語廣播一次(2026-07-01 移除 `repeat` 次數控制;舊工控機仍傳 `repeat` 會被 serde 忽略、不報錯)。持續性異常由工控機自行定時重發。
- **前端去抖 20s** — 同一 `alert_type` 在 `useDeviceAlert` 的 `DEDUP_WINDOW_MS`(20s)內只廣播 + toast 一次(不同 type 各自計窗)。**根因**:持續性異常工控機會狂丟同一訊號,不去抖會讓每筆都 `stopCurrent()` 打斷前一筆語音 → 一個字都聽不完 + toast 洗版。後端仍每筆記 event_log(保留診斷)。
- **`message` 只進 toast** — 自訂補充字無法即時合成語音,僅顯示於 toast。
- 新增固定分類:在 `public/sounds/alert/` 補對應 mp3 + i18n `deviceAlert.{TYPE}`(zh-Hant / vi-VN)+ `useDeviceAlert` 的 `PRERECORDED` set,工控機端不需改。重產音檔用 edge-tts(venv 安裝)。
- 廠商整合文件:`docs/device-alert-api.md`(+ `.docx`)。

### 認證 Token 儲存

`keyring` v3 crate,**必須**啟用對應平台 features(`apple-native`、`windows-native`、`sync-secret-service`),否則 fallback 成 in-memory mock,重啟 Token 即失。Token **不寫設定檔**。

## 資料庫 migration

`src-tauri/migrations/000X_xxx.sql` 編譯期執行(`sqlx::migrate!()`),不需手動跑。修改 schema 一律新增 migration 檔,**不要改舊檔**(已部署環境會跳過 hash 已變的 migration)。

當前 schema 重點表:
- `print_event` — 三來源(scan/auto/ipc)印單事件,統計用 `COUNT(DISTINCT shipping_no)` 去重
- `parcel_query_log` — `/api/parcel` 請求 log(含 `photo_path` 讀碼站存證、負數 `response_id` 表錯誤面單 / NoRead)
- `parcel_alert` — 雲端查件異常記錄(門市關轉 / 未確認 …,供手機 + 桌面回看)
- `report_queue` — 雲端回報佇列(pending / sending / success / failed)
- `sort_channels` / `sort_channel_dispatch` / `dispatch_provider` — 10 固定通道(L1-L5 / R1-R5)× 指派物流(多對多)
- `daily_stats` — 每日 request / success / **noread** / cache 統計(NoRead 計入 request、獨立 noread、不計 success)
- `pregen_done` — 面單預產去重單一來源(自動 + 手動共用,取代舊 localStorage)

## 發佈與 release

### 發版步驟(依序)

1. **先更新 `CHANGELOG.md`** — 在最上方新增 `## vX.Y.Z` 段落,寫本次修改/調整內容。**這步不可跳過**:`release.yml` 會依 tag 抽出對應段落注入 release 說明與 `latest.json` 的 `notes`,in-app「發現新版本」對話框顯示的就是它。沒寫 = 用戶只看到安裝包樣板、看不到變更。標題格式須為 `## vX.Y.Z`(對齊 tag,抽取靠此 regex)。
2. **三檔版本號一起改** — `package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`(三處都要,Cargo.toml 改完跑 `cd src-tauri && cargo check` 驗證)。
3. **commit + tag + push** — `git commit` → `git tag vX.Y.Z` → `git push origin main && git push origin vX.Y.Z`。push tag 觸發 Actions。
4. **手動公開** — workflow 產出的是 **draft release**(`releaseDraft: true`),需 `gh release edit vX.Y.Z --draft=false` 才公開、`latest.json` 才生效供自動更新。
5. **已發佈版本要補 changelog** — 若 release 已 publish 才發現 notes 漏了變更:`latest.json` 的 `notes` 已烤死,直接下載該 asset、改 `notes` 欄(平台 `signature` 不要動,簽的是 binary)、`gh release upload --clobber` 重傳,再 `gh release edit --notes-file` 更新頁面 body。

- **Tag `v*` 觸發** `.github/workflows/release.yml` GitHub Actions
- 三平台 runner:`macos-latest` (arm64) / `windows-latest` (NSIS) / `ubuntu:20.04|22.04|24.04` container(三條獨立 tarball)
- **macOS Intel 不再 native build** — v0.2.0 起 `macos-13` runner 移除(queue 3h+ 是常態),Intel Mac 啟用 Rosetta 2 跑 ARM64 dmg
- **Linux 跨 distro tarball 不能互通** — 20.04 自編 glib 2.78 + webkit 2.42 與系統 2.36 ABI 衝突,認對 distro。20.04 是 self-contained 離線可裝;22.04 / 24.04 走系統 `libwebkit2gtk-4.1-0`,需網路 apt
- **Windows MSI 跳過** — WiX `light.exe` 對中文路徑有 bug,只出 NSIS

完整 release iteration 經驗(source build、cache 策略、`.deb` Depends per-distro 等)見 `docs/next-steps.md`。

## 規範(來自 `~/.claude/CLAUDE.md` user-level instructions)

- **總是用繁體中文回覆**(連 PR 描述、commit 訊息也是)
- **禁止偷懶簡化** — 找真正根因,不只處理表面;多處同結構問題要全部一起改;不要默默跳過或標 TODO
- **commit 訊息不加 `Co-Authored-By` 行**
- **測試檔案放 `tests/`**,不堆專案根目錄
- **檔案不直接刪除** — 搬到 `backups/{YYYYMMDDHHMMSS}/{原路徑}` 保留原始結構
- **套件管理用 `yarn`**(非 npm),CI(`release.yml`)與 `tests/docker-ubuntu-build.sh` 皆走 `yarn install`
  - **`yarn.lock` 入版控**(供 Dependabot 掃漏洞 + 可重現建置)。它含全部平台的 optional binding(darwin / linux / win32),linux 與 windows CI 用同一份 lock 照樣裝得到 `@tauri-apps/cli-linux-x64-gnu` 等,不會缺席
  - **`package-lock.json` 不入版控**(已加入 `.gitignore`,避免不同 host 平台 optional deps 互相干擾 docker build:host 跑過 npm 後 lock 會鎖死 `darwin-arm64`,其他平台 binding entry 被移除)
  - ⚠️ **yarn 1 不自動安裝 peer dependencies**(npm 7+ 會)。pinia / vue-router 宣告 `@vue/devtools-api` 為非 optional peer,但會被 vue-i18n 的舊版 hoist 蓋掉 → `package.json` 已明確加 `@vue/devtools-api` devDependency 鎖住正確 major,**勿移除**
  - 非託管 runner(nodesource 裝的 nodejs)不含 yarn,需先 `npm i -g yarn`
- **PHP/Laravel 陷阱**(若有觸碰雲端後端):`??` 優先級、`foreach &$var` 後 unset、`SoapClient` 不可並行(讀取逾時受 PHP ini `default_socket_timeout` 控制)、並行 HTTP 用 `Http::pool()`

## 文件入口

- `README.md` — 用戶 / 部署面向的完整說明
- `docs/local-http-api.md` — 工控機 API 規範(廠商整合文件,亦提供 `.docx`)
- `docs/next-steps.md` — Roadmap + release iteration 經驗摘錄(供 `/reflect` 參考)

<!-- gitnexus:start -->
# GitNexus — Code Intelligence

This project is indexed by GitNexus as **cix3752iLabelPrint** (4204 symbols, 7627 relationships, 300 execution flows). Use the GitNexus MCP tools to understand code, assess impact, and navigate safely.

> Index stale? Run `node .gitnexus/run.cjs analyze` from the project root — it auto-selects an available runner. No `.gitnexus/run.cjs` yet? `npx gitnexus analyze` (npm 11 crash → `npm i -g gitnexus`; #1939).

## Always Do

- **MUST run impact analysis before editing any symbol.** Before modifying a function, class, or method, run `impact({target: "symbolName", direction: "upstream"})` and report the blast radius (direct callers, affected processes, risk level) to the user.
- **MUST run `detect_changes()` before committing** to verify your changes only affect expected symbols and execution flows. For regression review, compare against the default branch: `detect_changes({scope: "compare", base_ref: "main"})`.
- **MUST warn the user** if impact analysis returns HIGH or CRITICAL risk before proceeding with edits.
- When exploring unfamiliar code, use `query({query: "concept"})` to find execution flows instead of grepping. It returns process-grouped results ranked by relevance.
- When you need full context on a specific symbol — callers, callees, which execution flows it participates in — use `context({name: "symbolName"})`.

## Never Do

- NEVER edit a function, class, or method without first running `impact` on it.
- NEVER ignore HIGH or CRITICAL risk warnings from impact analysis.
- NEVER rename symbols with find-and-replace — use `rename` which understands the call graph.
- NEVER commit changes without running `detect_changes()` to check affected scope.

## Resources

| Resource | Use for |
|----------|---------|
| `gitnexus://repo/cix3752iLabelPrint/context` | Codebase overview, check index freshness |
| `gitnexus://repo/cix3752iLabelPrint/clusters` | All functional areas |
| `gitnexus://repo/cix3752iLabelPrint/processes` | All execution flows |
| `gitnexus://repo/cix3752iLabelPrint/process/{name}` | Step-by-step execution trace |

## CLI

| Task | Read this skill file |
|------|---------------------|
| Understand architecture / "How does X work?" | `.claude/skills/gitnexus/gitnexus-exploring/SKILL.md` |
| Blast radius / "What breaks if I change X?" | `.claude/skills/gitnexus/gitnexus-impact-analysis/SKILL.md` |
| Trace bugs / "Why is X failing?" | `.claude/skills/gitnexus/gitnexus-debugging/SKILL.md` |
| Rename / extract / split / refactor | `.claude/skills/gitnexus/gitnexus-refactoring/SKILL.md` |
| Tools, resources, schema reference | `.claude/skills/gitnexus/gitnexus-guide/SKILL.md` |
| Index, status, clean, wiki CLI commands | `.claude/skills/gitnexus/gitnexus-cli/SKILL.md` |

<!-- gitnexus:end -->
