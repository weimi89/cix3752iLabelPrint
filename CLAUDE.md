# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 系統定位

**智配通 面單列印** 是一個跨平台 Tauri v2 桌面 App,定位為「分揀工控機 ↔ 雲端 API」之間的本地中介服務。三個世界共存於同一進程:

- **本地 HTTP server**(axum, 預設 `0.0.0.0:18080`)— 工控機 PLC 透過 HTTP 呼叫,三支 endpoint:`/healthz`、`GET /api/parcel/{queryNo}`、`POST /api/report`
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
├── server/               axum HTTP server(給工控機)+ LabelPathResolver(local/share/http 三模式)
├── cloud/                雲端 API client + LabelFetchMode(download/cloud_print/web_print)
├── cache/                面單快取(LRU 清理 + hit/miss 統計)
├── queue/                report_queue + background worker(指數退避 retry)
├── watermark.rs          列印次數浮水印(字型編譯期內嵌)
├── printer/              系統印表機列舉與列印
├── health/               三層網路偵測(OS / Anchor / Cloud API)+ 抖動緩衝
├── log/                  分類事件 log(category × level)
└── commands/             Tauri IPC commands(前端可呼叫的 API)
```

`AppState` 是 `Arc<...>` 的共享狀態,由所有 Tauri command、axum handler、background worker 透過 `tauri::State` / clone 取用。新增功能時通常先決定 state 該放在哪個 sub-module。

### 前端(Vue 3)— `src/`

```
pages/                    14 個功能頁(Dashboard / ScanPrint / AutoPrint / PrintStats / SortChannels …)
components/               共用元件(AppNavbar / NetworkStatusIndicator / LocaleSwitcher …)
composables/              組合式邏輯(useNetworkStatus / useLabelStatus …)
stores/                   Pinia(status.js 集中管理 server/cloud/queue/cache/today/printStats)
config/navConfig.js       Sidebar 結構(主要 / 列印 / 日誌 / 設定 四群)
plugins/i18n/locales/     zh-Hant.json + vi-VN.json(雙語介面熱切換)
api/tauri.js              Tauri command wrapper + 非 Tauri 環境的 mock(支援純瀏覽器 preview)
@core/ @layouts/          Materio Vuetify Admin 樣板基礎
```

### Rust ↔ Vue 通訊兩條路

1. **Request/Response** — 前端 `invoke('command_name', args)` 呼叫 Rust `#[tauri::command]`,await 結果
2. **Server Push (事件)** — Rust `app.emit('event-name', payload)` → 前端 `listen('event-name', cb)`。目前事件:
   - `print-stats-updated`(三個寫入點 emit,前端 `DefaultLayout` listen,Navbar chip + 儀表板毫秒級同步)
   - `network-status`(`HealthChecker` worker 每輪檢查結束 emit)

**「不夠即時就 WebSocket」是錯方向** — 桌面 App 後端與前端在同一進程,Tauri IPC event 走進程內通道、毫秒級、不用 socket server。WebSocket 適合「跨網路、跨機器」,在這裡反而繞遠路。

### `non-Tauri` runtime guard

前端用 `typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__` 判斷是否為 Tauri runtime。若為純瀏覽器預覽(`npm run preview`),`api/tauri.js` 的 wrapper 會回傳 mock 資料,讓設計 / 排版可在瀏覽器迭代。新增 command 時記得同步補 mock 路徑,否則 web preview 會壞。

## 重要設計細節

### 面單路徑三模式(`label_path.mode`)

工控機讀面單三種拓撲:
- `local` — 回本機絕對路徑(同機部署)
- `share` — 回 SMB / NFS 共用目錄路徑(跨機 + 共用 NAS)
- `http` — 回 `http://{host}/images/{key}` URL(跨機,無檔案系統存取權)

設定頁可熱切換,**不需重啟** server。實作在 `src-tauri/src/server/mod.rs` 的 `LabelPathResolver`。

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

### 認證 Token 儲存

`keyring` v3 crate,**必須**啟用對應平台 features(`apple-native`、`windows-native`、`sync-secret-service`),否則 fallback 成 in-memory mock,重啟 Token 即失。Token **不寫設定檔**。

## 資料庫 migration

`src-tauri/migrations/000X_xxx.sql` 編譯期執行(`sqlx::migrate!()`),不需手動跑。修改 schema 一律新增 migration 檔,**不要改舊檔**(已部署環境會跳過 hash 已變的 migration)。

當前 schema 重點表:
- `print_event` — 三來源(scan/auto/ipc)印單事件,統計用 `COUNT(DISTINCT shipping_no)` 去重
- `parcel_query_log` — `/api/parcel` 請求 log
- `report_queue` — 雲端回報佇列(pending / sending / success / failed)
- `sort_channels` — 8 個固定通道(L1-L4 / R1-R4)
- `daily_stats` — 每日 request / success / cache 統計

## 發佈與 release

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
- **`package-lock.json` 不入版控**(已加入 `.gitignore`,避免不同 host 平台 optional deps 互相干擾 docker build)
- **PHP/Laravel 陷阱**(若有觸碰雲端後端):`??` 優先級、`foreach &$var` 後 unset、`SoapClient` 不可並行(讀取逾時受 PHP ini `default_socket_timeout` 控制)、並行 HTTP 用 `Http::pool()`

## 文件入口

- `README.md` — 用戶 / 部署面向的完整說明
- `docs/local-http-api.md` — 工控機 API 規範(廠商整合文件,亦提供 `.docx`)
- `docs/next-steps.md` — Roadmap + release iteration 經驗摘錄(供 `/reflect` 參考)
