# 智配通 面單列印(cix3752iLabelPrint)

> 介於分揀機工控機與雲端 API 之間的桌面中介 App,提供面單即時拉取、本機列印、分揀通道分配與佇列回報。

跨平台 Tauri v2 桌面應用,內建本地 HTTP server 給工控機呼叫、SQLite 持久化、雲端 API 代理、面單快取、列印工作流。

---

## 系統定位

```
┌─────────────┐   HTTP(LAN)   ┌─────────────────────┐   HTTPS    ┌──────────────┐
│  分揀工控機  │ ────────────▶ │ cix3752iLabelPrint  │ ─────────▶ │  雲端 API    │
│   (PLC)     │ ◀──────────── │   (本機桌面 App)    │ ◀───────── │  (Laravel)   │
└─────────────┘  /api/parcel  └─────────────────────┘  v2 print  └──────────────┘
                 /api/report           │
                                       ▼
                              ┌────────────────┐
                              │  本機印表機    │
                              │  (USB/網路)    │
                              └────────────────┘
```

**三大設計原則**:

1. **不回 base64** — 面單一律回本機路徑(或 `/images/{key}` URL),不直接傳檔案內容
2. **不讓工控機等雲端** — `POST /api/report` 立即回 200,雲端 webhook 推送由背景 worker 處理
3. **面單一律走本地** — 本地未命中時 Middleware 同步下載到完成,工控機統一存取點

---

## 主要功能

| 模組 | 說明 |
|---|---|
| **本地 HTTP API** | 給工控機呼叫的四支 endpoint(健康檢查、查包裹、回報結果、設備異常通知)。詳見 [`docs/local-http-api.md`](docs/local-http-api.md) |
| **掃描列印** | 操作員手動掃碼出單(對齊雲端 web 端 `scan-print` 體驗) |
| **自動印單** | 掃包裹條碼 → 列訂單清單 → 逐筆呼叫 cloud-print + 浮水印 + 本機列印 |
| **面單預產** | 批次預下載面單到本機快取 |
| **分揀通道** | 8 個固定位置(L1–L4 / R1–R4),指派物流與貼標人員 |
| **指派物流** | 物流商主檔 + 對應的印表機 `print_profile` |
| **印表機設定** | 列舉系統印表機、紙張尺寸、預覽列印 |
| **三層網路偵測** | OS 網卡 → 公網 anchor → 雲端 API HEAD(帶 Bearer),頂部顯示綜合狀態 |
| **印單統計** | 三來源(scan / auto / ipc)的 `shipping_no` 去重計數;每日 / 每小時 / 物流商 / 貼標人員四種拆分 |
| **佇列歷史** | `report_queue` 推送狀態(pending / sending / success / failed)與重試 |
| **請求記錄** | `/api/parcel` 查詢 log(物流商、通道、追蹤號) |
| **事件記錄** | 系統各層級事件 log(category × level 篩選) |
| **儀表板** | Middleware / 雲端 / 印單統計 三卡 + 當日 request / success / cache hit/miss + 網路狀態 |
| **全頁印單統計**| Navbar 右上常駐 chip(今日 / 昨日),任何頁面都看得到件數,點擊跳統計頁 |
| **設備異常廣播** | 工控機回報異常(卡包裹 / USB 斷線 …),桌面 App 用中越雙語**預錄語音**喊話現場人員 + toast。詳見 [`docs/device-alert-api.md`](docs/device-alert-api.md) |
| **雙語切換** | 繁體中文 + Tiếng Việt(vue-i18n,介面熱切換) |

---

## 技術棧

| 層級 | 技術 |
|---|---|
| 桌面殼 | Tauri v2(Rust) |
| 後端 | axum、sqlx(SQLite)、tokio、reqwest、image + imageproc + ab_glyph(浮水印) |
| 前端 | Vue 3 + Vite + Vuetify 3 + Pinia + vue-i18n + VueUse(Materio 設計語言) |
| 認證儲存 | keyring v3(macOS Keychain / Windows Credential Manager / Linux Secret Service) |

---

## 系統需求

- **作業系統**:macOS 11+、Windows 10+、Linux(GTK 3 + WebKit2GTK **4.1**;Ubuntu 20.04 / 22.04 / 24.04 三版各有對應 tarball)
- **開發工具**:
  - Node.js 18+(專案以 npm 為主,`yarn.lock` 不入版控)
  - Rust 1.78+(`rustup`)
  - 平台額外依賴:見 [Tauri Prerequisites](https://v2.tauri.app/start/prerequisites/)

---

## 快速開始

```bash
# 安裝前端依賴
npm install

# 開發模式(Tauri dev,前端 Vite HMR + Rust 改動自動重編 + 自動簽章)
npm run tauri:dev

# 打包生產版本(macOS .app / .dmg、Windows .msi / .exe、Linux .deb / .AppImage)
npm run tauri:build
```

首次啟動會在 app data 目錄建立 `config.toml` 與 SQLite 資料庫;不需手動初始化。

### macOS 開發者首次設定(免 keychain 重複跳框)

預設情況下,每次 `cargo build` 重編譯 binary 的 ad-hoc 簽章都不同,macOS keychain ACL 會失效導致**每次重編都跳「想存取 keychain」對話框**。

設一個固定的 code-signing identity 即可解掉:

```bash
# 列出可用 identity
security find-identity -p codesigning

# 把其中一個 Apple Development cert hash 寫進 ~/.zshrc(或 .bashrc)
echo 'export CIX3752I_DEV_SIGN_IDENTITY="<你的 cert hash>"' >> ~/.zshrc
```

之後 `npm run tauri:dev` 啟動的 binary 永遠用同一個 identity 簽章。第一次跳框時點「永遠允許」,以後重編都不再跳。詳細機制見 `.cargo/config.toml` + `scripts/dev-codesign-run.sh`。

> 沒設這個環境變數一切也能跑,只是每次重編會跳一次框。
>
> Dev 視窗 Dock 圖示會顯示「智配通 面單列印」(wrapper 自動把 binary 包進 dev `.app` 結構)。

---

## 設定檔位置

| 平台 | 路徑 |
|---|---|
| macOS | `~/Library/Application Support/com.weiminet.cix3752i.labelprint/` |
| Windows | `%APPDATA%\com.weiminet.cix3752i.labelprint\` |
| Linux | `~/.config/com.weiminet.cix3752i.labelprint/` |

包含:`config.toml`、`labelprint.sqlite`、`logs/`。

**雲端 API 設定**:第一次使用需在桌面 App 「雲端設定」頁填入 `api_base` 與 Bearer Token(Token 存 OS keyring,不寫設定檔)。

---

## 本地 HTTP API(給工控機)

預設綁定 `0.0.0.0:18080`。完整規範見 [`docs/local-http-api.md`](docs/local-http-api.md)(亦提供 `.docx` 給整合廠商)。設備異常通知另有獨立整合文件 [`docs/device-alert-api.md`](docs/device-alert-api.md)(亦含 `.docx`)。

| 方法 | Path | 用途 |
|---|---|---|
| `GET` | `/healthz` | 服務存活檢查 |
| `GET` | `/api/parcel/{queryNo}` | 掃碼查包裹 → 通道 / 列印 profile / 面單路徑 / `response_id` |
| `POST` | `/api/report` | 回報執行結果(只需 `response_id`) |
| `POST` | `/api/device-alert` | 回報設備異常 → 觸發中越雙語語音廣播 |
| `GET` | `/images/{label_key}` | 面單圖檔靜態服務 |

---

## 重要功能細節

### 面單路徑三模式(`label_path.mode`)

| 模式 | 回傳內容 | 適用場景 |
|---|---|---|
| `local`(預設) | 本機絕對路徑 | 工控機與本 App 在同一台機器 |
| `share` | 共用目錄路徑(SMB / NFS) | 跨機器、共用 NAS |
| `http` | `http://{host}/images/{key}` URL | 內網部署、跨機器無檔案系統存取權 |

設定頁可熱切換,**不需重啟** server。

### 列印次數浮水印

雲端回傳 `print_num > 1` 時,自動在面單右上角(順豐右下角)疊加 `(N)` 浮水印。字型使用 **DejaVu Sans Bold**(OFL 授權)於編譯期內嵌進 binary,**無須額外部署字型檔**。

### 設備異常語音廣播

工控機透過 `POST /api/device-alert` 回報設備異常(`PARCEL_JAM` 卡包裹、`USB_DISCONNECT` USB 斷線、`SCANNER_ERROR` / `PRINTER_ERROR` 故障等)。後端立即回 200(不讓工控機等),emit `device-alert` 事件,前端 `useDeviceAlert` 用**中文 + 越南語雙語語音**廣播喊話現場人員 + toast 顯示。

- **預錄音檔,非即時 TTS** — 內建分類碼的中越語音已預錄內嵌(中文 `HsiaoChen`、越南語 `HoaiMy` neural,以 edge-tts 產生於 `public/sounds/alert/`)。每台機音色一致、發音標準、**離線可用、越南語免在 Windows 裝語音包**。僅未錄音的自訂 `type` 才退回系統 TTS(`useSpeech`)。
- **次數可控** — body 的 `repeat` 控制廣播遍數,預設 1、後端 clamp 上限 3。
- **自訂補充字** — `message` 欄位顯示於 toast(語音只唸固定雙語文案)。

新增固定分類只需在 App 端補語音與 i18n,工控機端不需改動。詳見 [`docs/device-alert-api.md`](docs/device-alert-api.md)。

### 三層網路健康偵測

頂部 wifi icon 反映綜合狀態(綠 / 黃 / 紅),tooltip 展開三層細節:

1. **OS** — 網卡是否連線
2. **Anchor** — 公網 anchor host(預設 `1.1.1.1:443`)TCP 試連
3. **雲端 API** — 對 session endpoint 發 HEAD(帶 Bearer);401/403 顯示「未登入」、其他 4xx 顯示「業務錯誤」(降級為黃燈)、5xx 與連線失敗視為 Unreachable

支援抖動緩衝(連續失敗達 threshold 才標 down),正常 15s / 降級 60s 兩段間隔。

### 即時印單統計推播

印單統計**不靠輪詢**,改走 Tauri 內建 IPC event(`emit` / `listen`)。三個寫入點(掃描 / 自動 / 工控機 IPC)寫入 `print_event` 表後立即 `emit('print-stats-updated')`,前端 `DefaultLayout` 的 listener 觸發 `status.refreshPrintStats()`,Navbar chip 與儀表板印單統計卡同步更新,**端到端 < 100ms**,工控機請求進來就會看到件數 +1,不必等下一輪刷新。

系統狀態(server / queue / cache / today / cloud)仍走 5s 輪詢(不需毫秒級即時)。

---

## 資料夾結構

```
.
├── .github/workflows/
│   └── release.yml             # GitHub Actions release(tag v* 觸發,四平台並行 build)
├── .cargo/
│   └── config.toml             # 設 macOS cargo runner → scripts/dev-codesign-run.sh
├── scripts/
│   └── dev-codesign-run.sh     # cargo runner wrapper:固定 codesign + 包 dev .app bundle
├── docs/
│   ├── local-http-api.md       # 工控機 API 規範(廠商整合文件)
│   └── local-http-api.docx     # 同上 .docx 版
├── src/                        # Vue 3 前端
│   ├── pages/                  # 各功能頁(Dashboard、ScanPrint、SortChannels …)
│   ├── components/             # 共用元件(AppNavbar、NetworkStatusIndicator …)
│   ├── composables/            # 組合式邏輯(useNetworkStatus、useLabelStatus …)
│   ├── stores/                 # Pinia stores
│   ├── plugins/i18n/           # zh-Hant + vi-VN 語系包
│   └── @core / @layouts/       # Materio 樣板基礎
├── src-tauri/                  # Rust backend
│   ├── src/
│   │   ├── server/             # axum HTTP server(給工控機呼叫)
│   │   ├── cloud/              # 雲端 API client(Bearer + 重試)
│   │   ├── cache/              # 面單快取(命中/補下載/LRU 清理)
│   │   ├── queue/              # 回報佇列 + 背景 worker
│   │   ├── watermark.rs        # 列印次數浮水印(字型內嵌)
│   │   ├── printer/            # 系統印表機列舉與列印
│   │   ├── health/             # 三層網路健康偵測
│   │   ├── db/                 # sqlx + migrations
│   │   ├── commands/           # Tauri commands(前後端 IPC)
│   │   └── …
│   ├── migrations/             # SQLite migrations(編譯期執行)
│   ├── assets/fonts/           # 內嵌字型(DejaVu Sans Bold, OFL)
│   ├── Info.plist              # dev binary 嵌入 plist(macOS Dock 顯示中文名)
│   ├── tauri.conf.json
│   └── Cargo.toml
├── tests/                      # 測試(不放專案根目錄)
├── package.json
└── README.md
```

---

## 發佈打包

`.github/workflows/release.yml` 透過 GitHub Actions 自動 build 四平台、上傳到 draft release。

```bash
# 1. 確認版本同步(Cargo.toml + tauri.conf.json + package.json)
# 2. 打 tag 推上去
git tag v0.2.0
git push origin v0.2.0

# 3. 至 https://github.com/weimi89/cix3752iLabelPrint/actions 看進度
# 4. 完成後到 Releases 頁面,編輯 release notes 後按 Publish
```

| Runner | 產物 |
|---|---|
| `macos-latest` (arm64) | `cix3752iLabelPrint_{version}_aarch64.dmg`、`*_aarch64.app.tar.gz`(Intel Mac 透過 Rosetta 2 執行) |
| `windows-latest` | `cix3752iLabelPrint_{version}_x64-setup.exe` (NSIS only,跳過 MSI 避中文路徑 bug) |
| `ubuntu:20.04` container | `cix3752iLabelPrint-{version}-ubuntu-20.04.tar.gz`(離線:含自編 webkit2gtk-4.1 + glib 2.78 + libsoup3 + runtime .so + 主程式.deb + install.sh) |
| `ubuntu:22.04` container | `cix3752iLabelPrint-{version}-ubuntu-22.04.tar.gz`(走系統 `libwebkit2gtk-4.1-0`,install.sh 跑 apt) |
| `ubuntu:24.04` container | `cix3752iLabelPrint-{version}-ubuntu-24.04.tar.gz`(走系統 `libwebkit2gtk-4.1-0`,install.sh 跑 apt) |

**Linux tarball 客戶端安裝**:

```bash
# Ubuntu 20.04(離線可裝,內含完整 webkit2gtk-4.1 stack)
tar xzf cix3752iLabelPrint-0.2.0-ubuntu-20.04.tar.gz
cd cix3752iLabelPrint-0.2.0-ubuntu-20.04 && sudo bash install.sh

# Ubuntu 22.04 / 24.04(需網路,apt 自動解 webkit2gtk-4.1 等系統依賴)
tar xzf cix3752iLabelPrint-0.2.0-ubuntu-22.04.tar.gz   # 或 ubuntu-24.04
cd cix3752iLabelPrint-0.2.0-ubuntu-22.04 && sudo bash install.sh
```

後續主程式升級只換主 `.deb`(`sudo dpkg -i` 或 `sudo apt install ./*.deb`)。

### Release 實測狀態(v0.2.0)

| 平台 | GHA build | 本地實測 | 備註 |
|---|---|---|---|
| macOS arm64 | ✅ | ✅ | M1/M2/M3 開發機驗過;Intel Mac 啟用 Rosetta 2 後可跑同一份 ARM64 dmg |
| Windows x64 (NSIS) | ✅ | — | MSI 跳過(WiX `light.exe` 對中文路徑有 bug) |
| Linux Ubuntu 20.04 | ✅ | ✅ | docker `ubuntu:20.04` 內 ldd 全 link,install.sh 三步完成 |
| Linux Ubuntu 22.04 | ✅ | ✅ | docker `ubuntu:22.04` 內 ldd 全 link 系統 jammy 套件 |
| Linux Ubuntu 24.04 | ✅ | ✅ | docker `ubuntu:24.04` 內 ldd 全 link 系統 noble 套件 |

> 本地 docker 測試腳本:`tests/docker-ubuntu-build.sh 22.04 install`(隔離 host node_modules,需 OrbStack/Docker)。

### Known limitations

- **未簽章**:macOS 首次開啟需「系統設定 → 隱私與安全性 → 仍要開啟」;Windows 首次開啟需 SmartScreen「更多資訊 → 仍要執行」。改善要 Apple Dev ID($99/年)+ notarization 與 Windows EV cert($200~600/年)
- **macOS Intel 不再內建 native binary**:`macos-13` runner queue 嚴重(排 3h+ 是常態),自 v0.2.0 起發佈流程移除 Intel 構建,Intel Mac 啟用 Rosetta 2 後執行同一份 ARM64 `.dmg`即可。需要原生 Intel binary 請本地 `npm run tauri:build -- --target x86_64-apple-darwin` 自編
- **Linux 跨 distro**:不要把 20.04 tarball 拿去 22.04+ 安裝(自編 glib 2.78 / webkit 2.42 會跟系統 2.36 衝突),反之亦然 — 認對 distro
- **Linux 離線部署**:目前只有 20.04 tarball 是完整 self-contained;22.04 / 24.04 仍需網路給 apt
- **Windows MSI**:沒出,只出 NSIS(`light.exe` 對中文 path bug)
- **Linux Ubuntu 18.04 以下 / RHEL / Debian**:沒測,不保證

---

## 開發注意事項

- **測試檔案位置**:一律放 `tests/`,不堆在專案根目錄
- **Git commit**:不加 `Co-Authored-By` 行
- **檔案整理**:不直接刪除,搬到 `backups/{timestamp}/原路徑`
- **設定熱套用**:`cloud` / `cache` / `label_path` / `network` 區塊改動透過桌面 App 即時生效;`server.listen_ip` / `port` 改動需要 server restart(設定頁有按鈕)

---

## 授權與第三方資源

- **字型**:DejaVu Sans Bold,[DejaVu Fonts License](https://dejavu-fonts.github.io/License.html)(OFL 衍生,允許自由散布)
- **UI 樣板**:Materio Vuetify Vue Admin Template(`@core` / `@layouts`)
