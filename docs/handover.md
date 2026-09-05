# 交接紀錄

> **接手前先讀這份，但不可憑它直接動手** —— 它可能落後於程式碼。先跑測試與實查確認現況，與現況不符時**以現況為準並回頭修正這份文件**。
>
> 這是「快速接手」用的單一位置，持續更新同一份、不另開新檔。
> Roadmap 與歷史經驗在 `docs/next-steps.md`；工控機對外契約在 `docs/local-http-api.md`。

最後更新：**2026-09-02（v0.20.0 已公開發佈）**　目前版本：**v0.20.0（已發佈，九項產物齊全，`latest.json` 生效）**

---

## ⚠️ 立刻要處理的事（未完成，會影響使用者）

### 1. 工控機廠商需配合修改（外部依賴，中介端無法自行解決）

直印模式下工控機**仍必須** `POST /api/report`。目前它多半綁在「有沒有拿到 `label_path`」上，該欄位在直印模式不存在 → 整段跳過。

- 契約已寫進 `docs/local-http-api.md` 第 2、3 節（含完整時序表），可直接給廠商。
- **`.docx` 已在 2026-08-10 下午用 pandoc 重產**（舊檔備份於 `backups/20260810135454/`）。先前它停在 v0.12.0、**不含直印回報契約** —— 若照舊檔寄出，廠商會照錯的規格實作。要寄檔案版就寄現在這份。
- 廠商未修好之前，桌面 App「佇列歷史」頁的**回報來源欄會整批顯示黃色「僅中介機自印」** —— 這是預期現象，不是故障。修好後會轉綠色「工控機已回報」。

---

## 2026-09-05：現場作業監控「每日貼單」可指定業務日（v0.20.1）

### 做了什麼
- 主人反映雲端頁「每日貼單」無法查某日；雲端 cix3752iWeb 已修（`d18f0d1c`，已 push）：`FieldOperationMonitorService::payload(from, to, stickerDate)`，網頁與 `api/v1/local-middleware/field-operation-monitor` 都多收 `sticker_date`（空／格式錯／不存在日期→今日業務日，晚於今日→裁回今日），回應多 `stickerDate`。
- 中介端同步：`FieldOperationMonitorRequest`（from/to/sticker_date）取代原本借用的 `ClearanceProgressRequest`；`CloudClient::fetch_field_operation_monitor` 只在 `sticker_date` 非空時才送該參數（舊版雲端照常回應）；`api/tauri.js` 第三參數 + mock 回 `stickerDate`；`AppDatePicker` 新增 `max` prop；頁面「每日貼單」標題列加日期欄位＋查詢鈕，回應的 `stickerDate` 寫回欄位；i18n 雙語補 `businessDayHint`、`noRows` 改「此業務日尚無貼單資料」。
- CHANGELOG `## v0.20.1` 段落、三檔版本號已改，tag `v0.20.1` 已 push 觸發 Release CI；五平台 CI 全綠，release 已公開，`latest.json` 版本 0.20.1、notes 正確。

### 驗證結果
- `cargo check` 綠、`vite build` 綠、兩份 i18n JSON 可解析。
- 瀏覽器 mock 模式（`yarn dev --port 5188`）實機看頁面：日期欄位與查詢鈕位置、選日期後標籤跟著換、輪詢沿用所選日期（見下方補記）。
- **Tauri 實機未跑**（雲端正式站尚未部署 `d18f0d1c`，就算跑了也只會固定今日）。

### 尚未處理
- 雲端正式站部署 `d18f0d1c`（要 `yarn vite-build`）。

## 2026-09-02：異常件提示面單改為可切換開關（v0.20.0）

**需求**：工控機刷碼遇雲端業務錯誤（門市關轉 / 未確認 / 訂單異常 …）原本一律印錯誤面單並回分揀通道，
現場希望改成不印、也不回通道。做成**開關**而非直接砍掉，避免日後要恢復時再改一次程式。

### 做了什麼

| 檔案 | 改動 |
|---|---|
| `src-tauri/src/config/mod.rs` | 新增 `ErrorLabelConfig`（`error_label.enabled`，**預設 false**） |
| `src-tauri/src/server/mod.rs` | `LabelPathResolver` 加 `error_label` 旗標 + `is_error_label_enabled()`（與 `sort_only` 同一個熱套用出口）；`get_parcel` 的 `Err(e)` 分支依開關決定要不要解析通道、產面單、給 `response_id` |
| `src/pages/SortChannelsPage.vue` | 「分揀通道」頁純分揀卡片下方新增「異常件提示面單」開關卡片 |
| `src/plugins/i18n/locales/*.json` | 新增 `label.settings.errorLabel(Hint)` 與 `page.sort.errorLabel.*`；**順手修正**純分揀 hint 原本寫死的「異常件的錯誤面單仍會印」（開關關閉後已不成立） |
| `src/api/tauri.js` | mock config 補 `error_label`（否則 web preview 會壞） |
| `docs/local-http-api.md` + `.docx` | 廠商契約文件：新增兩種開關狀態的回應對照表與範例；**`.docx` 已用 pandoc 同步重產**（舊檔備份於 `backups/20260902102356/docs/`） |
| `CLAUDE.md` / `README.md` | 補開關說明 |

**只作用於工控機 `GET /api/parcel`**；桌面掃描列印 / 自動印單（`cloud_commands.rs`）刻意不動，那條路自有出口。

### 行為對照（實測值）

| 開關 | `channel_code` | `print_profile` | `label_path` | `response_id` | `is_error_label` | `should_print` |
|---|---|---|---|---|---|---|
| **關（預設）** | `null` | `null` | 不回傳 | `null` | `false` | `0` |
| 開 | 照舊解析（`L1` / fallback `LS`） | 照舊 | 有 | 負數 | `true` | `1` |

兩種狀態下 `parcel_alert`、`parcel_query_log`、`daily_stats`、件數核對都照常寫 —— 稽核資料不因開關缺漏。

### 驗證結果

- `cargo check` / `cargo test`（67 項）/ `yarn build` 全綠。
- **實機打本地 server**（App 跑起來、雲端已登入）：
  - 開關關 → `GET /api/parcel/TESTNOTEXIST0001` 回 200，`channel_code`/`print_profile`/`response_id` 皆 `null`、無 `label_path`、`error_code=NOT_FOUND`；DB 寫入 `should_print=0`、`sort_channel=null`、`label_key=null`；`cache/labels/@error/` 未產生任何檔案。
  - 開關開 → 查無訂單回 fallback 通道 `LS`、訂單異常（`74Z01112337`）回 `L1` + `PAPER-01#100x150`，兩者都產出面單 PNG、`response_id` 為 `-10` / `-11`、`should_print=1`。
  - 測試造出的查詢紀錄、異常紀錄、當日統計與面單檔**驗完已清除**，`config.toml` 已還原（`direct_print`、無 `[error_label]` 區塊）。
- UI：以 `vite preview` + headless Chrome 截圖確認新卡片版面與文案正常（灰色關閉狀態）。

### 已知限制

- **UI 切換開關的熱套用未實機點過** —— Tauri WebView 不吃合成點擊事件，改用改 `config.toml` + 重啟驗後端行為。
  熱套用走的是 `update_config` → `label_resolver.apply_config()`，與純分揀開關**同一行出口**（旗標就寫在該函式內），
  但「點下去立即生效」這件事本身沒有實機證據。下次有人操作 App 時順手確認一次即可。
- 開關開啟 + `direct_print` 模式的**本機送印**分支未實測（本機接著實體印表機，測會真的吐紙）。
  該分支程式碼本次只是整段包進 `if`、內容未改動。

### 發版狀態（v0.20.0）

| 步驟 | 狀態 |
|---|---|
| `CHANGELOG.md` 寫 v0.20.0 段落 | ✅ |
| 三處版本號同步（package.json / tauri.conf.json / Cargo.toml） | ✅ `cargo check` 綠、Cargo.lock 同步 |
| commit + tag + push | ✅ |
| GitHub Actions 建置（run `33584518592`） | ✅ 七個 job 全綠，`verify-assets` 通過，全程約 19 分 |
| 公開 draft release | ✅ 2026-09-02 03:07 UTC 公開，`releases/latest` 已指向 v0.20.0 |

`latest.json` 實查：version `0.20.0`、四個平台簽章齊全、notes 正確帶入 CHANGELOG（342 字元），
App 內「發現新版本」會正常顯示。九項產物：三條 Linux tarball（20.04 86MB / 22.04 32MB / 24.04 32MB）、
macOS arm64 dmg + app.tar.gz + sig、Windows NSIS + sig、latest.json。

**順帶驗證到：9/1 建的 20.04 快取預熱機制確實生效** —— 這次 tag 觸發的 run，20.04 只花 **7 分**
（v0.19.0 首次發版時是 1 小時 13 分，全程從原始碼編 webkit）。「快取按 ref 隔離、tag 永遠讀不到」
那個問題已由排程在 main 分支預熱解決，實測有效。

---

## 2026-08-31：v0.19.0 發版狀態

| 步驟 | 狀態 |
|---|---|
| CHANGELOG 寫 v0.19.0 段落 | ✅ 完成（原未發佈的 v0.18.1 段落已併入本版） |
| 三處版本號同步（package.json / tauri.conf.json / Cargo.toml） | ✅ 完成，`cargo check` 綠、Cargo.lock 同步 |
| commit + tag + push | ✅ `f50a23a`，tag `v0.19.0` 已推送 |
| GitHub Actions 建置（第一次，run `33400965877`） | ⚠️ **macOS / Windows 成功，三個 Linux tarball 全部沒上傳** —— 見下方「Linux 上傳失敗」 |
| CI 修正 | ✅ `8092ba0` 已推 main（container job 的 shell 修正 + 產物驗收 job） |
| GitHub Actions 補跑 Linux（run `33460047068`） | ✅ 三個 tarball 全部上傳成功（20.04 86MB / 22.04 32MB / 24.04 32MB），`verify-assets` 綠燈 |
| **公開 draft release** | ✅ 已於 2026-09-01 04:00 UTC 公開。`latest.json` 驗過：version `0.19.0`、四個平台簽章齊全、notes 正確帶入 CHANGELOG（1531 字元），App 內「發現新版本」會正常顯示 |

### 順帶查出：20.04 的 webkit 快取從來沒生效過（尚未處理）

20.04 每次發版都要花 **1 小時 13 分**從原始碼編 webkit2gtk。workflow 有做 `focal-upstream-stack`
快取要避免這件事，但它**一次都沒命中過** —— key 是固定字串 `focal-upstream-stack-glib2.78-libsoup3.4-webkit2.42-v1`，
三次建立的 key 完全相同，卻各存一份：

| 建立 | ref |
|---|---|
| 8/29 | `refs/tags/v0.18.0` |
| 8/31 | `refs/tags/v0.19.0` |
| 9/1（本次補跑） | **`refs/heads/main`** |

**根因**：GitHub Actions 的快取**按 ref 隔離** —— 一個 run 只讀得到「自己這個 ref」與「預設分支」的快取。
發版一律 push tag 觸發，每次都把成果存進那個 tag 專屬的格子，下一個 tag 換新格子就再也讀不到。

**現況**：本次補跑是從 main 觸發（`workflow_dispatch --ref main`），快取已存進 `refs/heads/main`，
而 main 是預設分支 → **之後 tag 觸發的 run 讀得到了**，v0.20.0 發版時 20.04 應可省下這 1 小時多。

**已處理**（commit `ab5f036`）：新增 `.github/workflows/warm-focal-cache.yml`，
每週日 / 週三 18:00 UTC（台北週一 / 週四 02:00）在 main 上碰一次快取 ——
命中就只刷新存取時間（實測 2 分 21 秒跑完），沒命中就重編存回。間隔 3-4 天，
穩穩踩在 7 天過期線內側。10GB LRU 那條也一併緩解：每週被存取兩次，是最不容易被擠掉的一份。

200 行的 stack 建置步驟抽成 composite action `.github/actions/build-focal-stack`，
`release.yml` 與預熱 workflow 共用，不留第二份會漂移的複本。三個要點：

- composite action 的 run step 一律 **`shell: sh`**，維持與 container job 預設 `sh -e` 相同行為。
  改 bash 會帶 `-eo pipefail`，踩爆裡面多處 `gcc --version | head -1`、`cmake --version | head -1 | grep -q`
  （`head` 提早關管線 → 上游收 SIGPIPE → 整條 pipeline 判失敗），跟這次修的是同一類坑。
- `release.yml` 的 `checkout` 提前到 steps 最前面 —— local composite action 要先 checkout 才存在。
- action 有 `force_rebuild` input：平常快取一直命中、**建置那條路根本不會執行**，
  必須能定期強制跑一次，否則上游 tarball 搬家之類的問題會等到快取失效那天、在發版當下才爆。

**已驗證**：搬移等價性（7 個 step 的 run / uses / with / id / if 條件逐字比對相同）；
actionlint 改動前後同樣 8 個 shellcheck 提示、無新增；實跑一次快取命中路徑（run `33468624579`）
—— checkout 在 bare container 最前面可行、action 正確載入、`Cache hit` + 還原後
`pkg-config` 回報 glib 2.78.6 / libsoup 3.4.4 / webkit2gtk 2.42.5 版本全對。

### Linux 上傳失敗（已修，`8092ba0`）

三個 Linux job 都**建置成功**（tarball 都打好了，33M），死在最後一步「上傳 asset」：

```
/__w/_temp/xxx.sh: 3: set: Illegal option -o pipefail
##[error]Process completed with exit code 2.
```

**根因**：`release-linux` 跑在 `container:` 裡，GitHub 對 container job 的預設 shell 是 `sh -e {0}`（dash），
不是 runner 本體的 bash；dash 沒有 `set -o pipefail`。同一份 workflow 的 `create-release` 寫法相同卻沒事，
是因為它跑在 `runs-on: ubuntu-latest`（runner 本體預設 bash）—— **同樣一行在 container 內外命運不同**。

**修法**：該步明寫 `shell: bash`。刻意**不**在 job 層級統一改：GitHub 的 `shell: bash` 預設帶 `-eo pipefail`，
而這個 job 其他步驟有多處 `gcc --version | head -1`、`ls *.deb | head -1` 等寫法，在 pipefail 下
`head` 提早關管線會讓上游收 SIGPIPE(141)、整條 pipeline 被判失敗 —— 整批改過去等於一次踩滿新坑。

### 為什麼沒被擋下來（已補 `verify-assets` job）

`release-linux` 帶 `continue-on-error: true`（讓單一 distro 掛掉不阻其他 matrix），
代價是**三個 Linux 全掛、整個 run 仍是綠燈**，只能靠人工去看 annotations 才發現。

新增的 `verify-assets` job 直接比對 release 上實際有哪些 asset，缺一項就紅燈。
刻意**不看** `needs.*.result` —— `continue-on-error` 的 job 失敗後傳給 `needs` 的結果仍是 `success`，
看 result 驗不到任何東西；驗的是「產物在不在」，不是「job 綠不綠」。

Linux 期望檔名的 distro 清單由 `scripts/list-linux-distros.py` 從 `release-linux` 的 matrix 解析，
不另抄一份清單，增減 distro 時驗收自動跟上；解析失敗一律非 0 退出，不會退化成空清單而靜默通過。

**已驗證**（用真實 release 資料跑過腳本，四個情境）：v0.19.0 `only=all` 與 `only=linux` 都精準抓出缺的三個 tarball；
`only=desktop` 綠燈（6 項）；產物齊全的 v0.18.0 `only=all` 綠燈（9 項，證明期望清單與歷史成功發版吻合）。
腳本本身另測四種反例（job 改名 / `distro` 欄位改名 / matrix 清空 / workflow 檔不存在）全部非 0 退出。

**本版包含**：手機遙控頁指派功能、清關浮動框分廠、錯誤提示人話化、提示版面收斂、Rust 10 個 major 升級。

**注意**：清關浮動框的分廠統計需雲端同步部署（雲端程式碼已 commit 並推上 origin/main，是否已部署到現場那台連的環境未確認 —— 測試站 local-18001.build-site.dev 當時整站 502，無法實打驗證）。

---

## 2026-08-31：套件升級（Rust 10 個 major 全數升上、前端已無可升）

### 結果

| 套件 | 舊 → 新 | 需要處理什麼 |
|---|---|---|
| base64 | 0.22 → 0.23 | 無，直接編過 |
| tower-http | 0.6 → 0.7 | 無 |
| toml | 0.8 → 1.1 | 無 |
| tokio-tungstenite | 0.24 → 0.30 | 無 |
| barcoders | 1 → 2 | 無 |
| imageproc | 0.25 → 0.27 | 文字繪製拆成 `text` feature，而專案是 `default-features = false` → 要明確補上 |
| axum | 0.7 → 0.8 | 路由參數語法 `:name` → `{name}`；移除了 `extract::Host` |
| reqwest | 0.12 → 0.13 | `rustls-tls` 改名 `rustls`；`query()` 拆成獨立 feature |
| keyring | 3 → 4 | features 全部重整，`default` 已含三平台原生 store，改為不指定 |
| sqlx | 0.8 → 0.9 | `runtime-tokio-rustls` 拆成兩個 feature；**動態 SQL 要人工審核後標記** |

前端：約束內全部升到最新（tauri plugins、vue-echarts）；`yarn outdated` 現在無輸出。

### 幾個要記住的點

- **`axum` 0.8 拿掉 `extract::Host`** 是因為它會採信 `X-Forwarded-Host`（可被假冒）。本服務只在區網內給工控機呼叫，改成直接讀 `Host` header、不看任何 forwarded 標頭，不必為此引入 axum-extra。
- **`sqlx` 0.9 會擋下所有非字面常數的 SQL**（6 處）。逐一審過：動態的只有「組進哪幾個條件」與「`IN (?)` 有幾個佔位符」，欄位名寫死、值全部走 bind，確認無注入風險後才用 `AssertSqlSafe()` 標記。**不可無腦包，那等於把這個機制關掉。**
- **`keyring` 4 編得過不代表存得住** —— v3 少 feature 會靜默退回記憶體實作，只有「重啟後 Token 不見」才會發現。新增測試：寫入後用 macOS `security find-generic-password` 從外部確認真的落進 keychain。
- **`reqwest` 0.13 一度解不開相依**（要 `aws-lc-rs ^1.18`，cargo 說 crates.io 只有 1.17）。實際是**本機索引快取過舊**，重整後就找得到 1.18.0 —— 遇到「上游版本明明存在卻找不到」先重整索引，不要急著判定是上游沒發。
- **`nanoid` 的 `resolutions` 已可移除**：當初（v0.17.1）是為修安全通報而鎖 `^3.3.17`，現在 postcss 8.5.26 自己就要求 `^3.3.17`，覆寫沒有作用了。`nanoid` 顯示「落後 6.0.1」是假象 —— 真正的使用者是 postcss，它走 3.x legacy 線。

### MSRV 拉高到 1.94（要注意）

`sqlx` 0.9 要求 rustc 1.94，`Cargo.toml` 的 `rust-version` 已從 1.77 改為 1.94。本機工具鏈已 `rustup update` 到 1.98.0；CI 用 `dtolnay/rust-toolchain@stable`（自動最新），不受影響。**若有其他環境用舊 rustc 編譯，會編不過。**

### 驗證

| 項目 | 結果 |
|---|---|
| `cargo test` | 67 項全綠（含新增的 keyring 落地測試） |
| `yarn build` / `yarn audit` | 綠 / 0 漏洞 |
| `yarn outdated` | 無輸出 |

---

## 2026-08-31：提示訊息的圖示不再是佔空間的實心色塊

### 問題

提示（VAlert）左邊那顆圖示是**實心紅圓 + 白 X**，看起來像多了一顆有底色的按鈕，把短訊息擠成兩行、整條提示被撐高。toast 也一樣（綠圓 + 白勾）。

### 追根

- 圖示不是 CSS 加的底色，**是圖示本身的造型**：`src/plugins/vuetify.js` 用的是 `vuetify/iconsets/mdi` 的 aliases，其中 `error: i-mdi:close-circle`（實心）。
- 過程中發現兩個**樣板殘留檔案根本沒被使用**，改了不會有任何效果：
  - `src/plugins/vuetify-materio/icons.js`（整組 tabler 線條 alias）—— `vuetify.js` 沒 import 它，直接用 mdi 那組。
  - `src/styles/@core/template/_components.scss`（Materio 的 VAlert 等元件樣式）—— `@core/template/index.scss` 沒有 `@use "components"`。
  這解釋了為什麼「Materio 的樣式覆寫看起來存在、實際沒作用」。**動這兩個檔前先確認它有沒有被引用。**

### 修法

- `src/plugins/vuetify.js`：在 mdi aliases 之上覆寫四個語氣圖示為 tabler 線條版（`circle-check` / `info-circle` / `alert-triangle` / `alert-circle`），其餘 alias（展開、排序、分頁…）維持 mdi 不動。
- `src/styles/main.scss`：VAlert 的 `__prepend` 兩件事 ——
  - **尺寸**：從「28px 圖示 + 16px 間距 + 垂直置中」改成「與內文同高 1.125rem + 6px 間距」。
  - **排版**：Vuetify 預設把圖示放在獨立一欄（grid `prepend content append close`），文字只能用右半邊，**窄版面時每行都被截短、右邊空一塊、短句被拆成好幾行**。改成 `display: block` + 圖示 `float`，讓圖示併進文字流，第二行起流到圖示下方，整條提示的寬度用得完。
  - 脫離 grid 後關閉鈕會掉到文字後面，改釘在右上角（`.v-alert` 本身已是 `position: relative`）；只有真的有關閉鈕的提示（`:has(.v-alert__close)`）才把內容右邊空出來。
  - **只做上面兩步會沒有效果**：`.v-alert__content` 帶著 `overflow: hidden`，那會建立 BFC —— 整塊內容被推到浮動圖示右側，每一行照樣縮排，float 等於白做。必須一併把它改回 `overflow: visible`（外框 `.v-alert` 自己仍有 `overflow: hidden`，圓角裁得住）。這個現象從畫面上看就是「改了跟沒改一樣」，很容易誤判成樣式沒生效去追 CSS layer。
- `src/plugins/toastify.js`：toast 內建圖示同樣是實心色塊，改成傳入同一組 tabler 線條圖示；顏色靠 `main.scss` 依語氣給（用 toastify 自己的 `--toastify-icon-color-*` 變數）。

### 驗證

| 項目 | 方式 | 結果 |
|---|---|---|
| 提示圖示 | dev App 實機截圖（浮動框雲端 502 的錯誤提示） | 線條圖示、與文字同高、無底色，文字空間變大 |
| toast 圖示 | 實機觸發「複製位址」的成功 toast | 綠色線條圈勾，無實心色塊 |
| 窄版面文字是否用滿寬度 | `yarn preview` + headless Chrome 以 360px / 520px 寬截圖 | 文字用滿整寬、第二行起沒有縮排（該環境載不到 iconify 圖示，圖示本身以 dev App 實機那張為準） |
| 第二行是否流到圖示下方 | dev App 實機放大截圖（浮動框寬約 230px，本來就是窄版面） | 修掉 BFC 後第二行貼齊最左邊；修之前每行都縮排 |
| 圖示是否對齊第一行字中 | 對實機截圖做像素量測（找出圖示與第一行文字各自的上下緣算中線） | 圖示與文字可見高度同為 14px，中線差 1px（圖示偏上）|

圖示的垂直對齊做法是「容器高度 = 一行行高（1.5em）＋ 圖示在其中置中」，幾何上即為正中。殘餘的 1px 來自 tabler 圖示 SVG 自身的內部留白（圓圈沒畫滿方框），**刻意不加固定位移修正** —— 各語氣圖示的造型重心不同（`alert-circle` 偏上、`alert-triangle` 偏下），統一位移會讓其他語氣跑掉。
| 建置 | `yarn build` | 綠 |

**未驗證**：唯一那個「可關閉」的提示（掃描列印頁的列印結果摘要）—— 要跑完一次列印流程才會出現，雲端目前不通、查不到件所以觸發不了。關閉鈕改成絕對定位＋內容右側留白，規則已在編譯產物中確認存在，但視覺未實看。

### 檢查過、判定不用動的

- **VSnackbar / VBanner**：專案完全沒用。
- **儀表板網路狀態、各卡片圖示**：本來就是 tabler 線條版（小尺寸看起來像實心而已）。
- **3 處刻意用 filled 的狀態圖示**：自動印單頁的完成勾（`tabler-circle-check-filled`）、分揀通道卡片的播放／暫停（`tabler-player-*-filled`）。那是狀態指示不是提示訊息，未動 —— 要不要一併改成線條版待決定。

---

## 2026-08-31：錯誤訊息不再把技術原文丟給操作員

### 起因

清關進度浮動框在雲端不通時，整片紅字顯示 `HTTP 錯誤: HTTP status server error (502 Bad Gateway) for url (https://…/clearance/progress?from=…&print_type=)` —— 操作員看不懂，也不知道要做什麼。

追下去發現**不只浮動框**：後端 `AppError` 序列化給前端時只丟 `to_string()`，前端 16 個檔案共 31 處一律 `String(e?.message || e)` 直接顯示，全站都是這個樣子。

### 修法（根因）

**後端 `src-tauri/src/error.rs`**

- 新增 `AppError::kind()`，把錯誤分成 5 類：`network`（連不上雲端）、`unauthorized`（雲端登入失效）、`cloud`（雲端業務錯誤，訊息本來就是中文）、`input`（本機擋下的輸入／設定問題，訊息是我們自己寫的）、`internal`（IO／DB 等內部故障）。
- `Serialize` 從單一字串改成 `{ kind, message, detail }`。`detail` 保留完整技術訊息供診斷；`message` 欄位仍在，**舊的 `e?.message` 取法照樣拿得到值**，不會因為改成物件而讓沒改到的地方變 undefined。

**前端**

- 沿用既有的 `errorMessageFromException()`（原本靠字串比對猜 UNAUTHORIZED／timeout，脆弱），改成**優先看 `kind`**：`cloud`／`input` 原樣顯示訊息，其餘翻成 i18n 文案；技術原文一律進 `console.warn`，不進畫面。沒有 `kind` 的例外（前端自己丟的、Tauri runtime 的）仍走舊的字串判斷。
- 16 個檔案 31 處 `String(e?.message || e)` 全部換成 `errorMessageFromException(e)`。
- 新增雙語文案 `error.network` / `error.unauthorized` / `error.internal`。

### 驗證結果

| 項目 | 方式 | 結果 |
|---|---|---|
| 浮動框在雲端 502 時的顯示 | dev App 實機截圖（測試站正好整站 502） | 顯示「目前連不上雲端，稍後會自動重試」，原本的英文與網址不再出現 |
| 分類正確性 | `cargo test`（新增 `error::tests` 2 支） | `input` / `unauthorized` / `cloud` / `internal` 皆符合；訊息內容保留 |
| 全部測試 | `cargo test`、`node tests/control-page.test.mjs` | 全綠（Rust 無 failed；手機頁 35 項） |
| 前端建置 | `yarn build` | 綠 |

**未實機觸發的分支**：`cloud` / `input` 類（需要造雲端業務錯誤或存一筆非法通道代碼）。這兩類的前端邏輯是「原樣顯示 `message`」，且 `message` 內容由 Rust 測試涵蓋。

### 同一個問題還有一處沒動

儀表板「網路連線狀態」卡片仍顯示 **`重試中 1/2 · HTTP 502`**。那一區本來就是給人看連線診斷的，狀態碼有其用途，但 `HTTP 502` 對操作員一樣是術語 —— 待決定要不要也換成人話。

---

## 2026-08-31：手機遙控頁可設定貼標人員與指派物流

### 已完成（未 commit，只動中介端）

現場換人／換線時要改「這個通道由誰貼標、接哪幾家物流」，原本只能走到桌面 App 的「分揀通道」頁。
現在同區網手機開 `/control` → 點通道 → **設定貼標與物流**，即可直接改，桌面畫面同步跟著變。

**後端 `src-tauri/src/server/mod.rs`（新增三支手機遙控用 endpoint）**

| Endpoint | 用途 |
|---|---|
| `GET /api/dispatch-providers` | 物流商清單（手機的選項來源） |
| `GET /api/sticker-history` | 人員歷史名單（與桌面共用 `sticker_history` 同一份） |
| `POST /api/channels/:position/assign` | 寫入該通道的 `job_sticker` + `dispatch_codes` |

`assign` 的把關（與桌面 `sort_channel_save` 同語意，只是改由後端擋）：

- **刻意不含通道代碼與印表機** —— 那兩項改錯會整條線分錯格口或靜默漏印，留桌面統一管理。
- 物流代碼去重去空白；**代碼必須存在於 `dispatch_provider`**（手機清單可能是刪除物流商前抓的，放行會讓通道指到不存在的物流、看起來有指派卻永遠分不到件）。
- **direct_print 模式**下該通道有代碼、要指派物流、卻沒設印表機 → 400 擋下（否則每件都靜默漏印）。
- 通道列與多對多指派同一個交易寫入；驗證失敗時完全不動 DB。
- 錯誤同時回機器碼與中文訊息（手機頁是中越雙語，用 code 翻成自己的語言，對不到才顯示中文原文）。

**手機頁 `src-tauri/src/server/control_page.html`**

- 詳情頁資訊卡下方多一顆「設定貼標與物流」→ 進設定頁（走 history，手機返回手勢可用）。
- 設定頁：姓名輸入框 + 最近使用名單快選（再點一次同一人＝清空）、物流多選 chips、儲存。
- 貼標人員未指派時詳情頁也顯示一列「未指派」（原本空白就整列不見，看不出可以設定）。
- **設定頁停止輪詢重繪** —— 每 3 秒重繪會把正在輸入的姓名與未存的勾選沖掉，等於打不完字。
- 中越雙語完整；標題只放「設定 / Cài đặt」（帶通道名再加長文案會被截斷）。

**桌面 `src/pages/SortChannelsPage.vue`**

- `sort-channel-updated` 事件的 payload 多帶 `job_sticker` / `dispatch_codes`，桌面即時套用；
  **該通道正在編輯（dirty）時不覆蓋** —— 蓋掉的是操作員螢幕上看得到的內容，他不會知道值被換過。

### 驗證結果（實測）

| 項目 | 方式 | 結果 |
|---|---|---|
| 三支 endpoint 正常流程 | 對本機 dev server curl | 200，DB 與回應一致 |
| 無效通道位置 / 不存在的物流代碼 | curl | 400，且 DB 完全沒動 |
| direct_print 缺印表機 → 擋下 | 暫時清掉 R4 印表機後 curl | 400 `PRINTER_REQUIRED`；只改人員不指派物流仍放行；補回印表機即放行（已還原） |
| 姓名只有空白 → 存 NULL、重複物流代碼去重 | curl | 皆正確 |
| 手機改 → 桌面即時同步 | 桌面開分揀通道頁 + curl 改 L1 → 截圖 | 物流與人員即時變更，未被標成未存檔 |
| 桌面正在編輯時不被覆蓋 | 桌面 L1 打字後 curl 改同一通道 → 截圖 | 欄位保留使用者輸入、標「未儲存」 |
| 手機頁版面（中／越）| headless Chrome 以 390px 寬截圖 | 版面正常、chips 換行正常、標題不截斷 |
| 手機頁邏輯 35 項 | `node tests/control-page.test.mjs`（新增，jsdom 已入 devDependencies） | 全過；含 XSS 跳脫、輪詢不覆蓋草稿、錯誤碼翻譯、存檔後返回 |
| Rust / 前端 | `cargo check`、`node --check` | 綠 |

**過程中抓到並修掉的真 bug**：輸入框原本寫 `oninput="draft.sticker = this.value"` —— 內嵌事件處理器看不到模組內的 `let`，會 ReferenceError 且畫面毫無徵兆（打字完全不進草稿）。已改成呼叫函式寫入。

### 覆檢（`/code-review medium`）找到並已修掉的 4 項

| # | 問題 | 現場會怎樣 | 修法 |
|---|---|---|---|
| 1 | `refresh()` 的 `catch` 仍會重繪設定頁，抵銷了「輪詢不重繪」保護 | 工控房 Wi-Fi 斷續時每 3 秒把輸入框砍掉重建，名字永遠打不完 | catch 內同樣在 `view === 'assign'` 時直接 return |
| 2 | 點物流 chip / 歷史名單會整段重繪，中越輸入法**組字中**尚未上字的內容被還原掉 | 打了名字還沒上字就先點物流 → 名字整段消失 | 抽 `flushStickerInput()`，重繪與存檔前都先把輸入框現值收回草稿 |
| 3 | 存檔後的 `fetchChannels()` 失敗會被當成「儲存失敗」 | 後端其實已寫入，操作員以為沒存到而重存，訊息與事實矛盾 | POST 成功即視為成功，後續重抓失敗只吞掉 |
| 4 | `PRINTER_REQUIRED` 連「只改貼標人員」都擋 | 手機草稿會原樣送回既有物流清單；站點切成 direct_print 但印表機還沒設時，換班的人連改個名字都被擋、還被叫去設印表機 | 收斂成「這次真的**新增**了物流」才擋；只改人員、移除物流一律放行（少接件不會生出漏印） |

四項修完皆已重測：前端 35 項全過（新增第 12–14 組專測這些情境）；後端以 curl 實測「只改人員放行 / 新增物流擋下 / 移除物流放行 / 空通道再新增擋下」四種組合，測試用的 R4 印表機與指派已還原。

### 尚未做

- **未 commit**，working tree 保留。
- 手機頁未用真手機開過（版面以 390px headless 截圖驗證，互動以 jsdom 驗證）。
- 這三支 endpoint 屬手機遙控內部用途，**未寫入 `docs/local-http-api.md`**（該文件只放工控機對外契約，既有的 `/api/channels`、`/api/alerts` 同樣沒寫）。
- 浮動框貼單那列的文案已從「貼單單數(去重)」改為 **「貼單單數」**（中越雙語）—— 「去重」是工程術語。作業監控頁的「貼單袋數(去重)」「貼單單數(去重)」「去重總計」**未動**（該頁整頁對照雲端網頁版，改了會兩邊不一致），待決定。
- 覆檢另外指出 **`src/components/ClearanceProgressWidget.vue:67`**（切廠別時 `setStickerScope` 沒 await 就接 `loadRange`：多一次雲端往返，且 sticker 請求較晚失敗時，清關進度明明載入成功畫面卻掛著錯誤訊息）。**該檔是先前留在 working tree 的未提交變更、不屬本次任務，未動**，待決定要不要一併修。

---

## 2026-08-29：查件異常頁補分頁／搜尋 + 清關進度框加「貼單單數（去重）」

### 已完成（未 commit，跨 2 repo）

**A. 查件異常頁（中介端）**
- 後端 `parcel_alert_list` 取代寫死 100 筆的 `recent_parcel_alerts`：關鍵字（查詢單號／物流單號／訊息）＋類別篩選＋分頁＋total；查詢本體 `list_parcel_alerts(db, req)` 可獨立測試（`lib.rs` 的 `commands` 改 `pub mod`）。
- 前端 `ParcelAlertLogPage.vue` 比照請求記錄頁：進階查詢面板 + `TablePagination`（頭尾）+ 事件去抖 400ms。
- 驗證：`cargo check`、`cargo test --test parcel_alert_list`（3 passed）、`vite build` 綠。**畫面未實機看**（Chrome 擴充開不了 localhost），元件與請求記錄頁同一套。
- 手機端 `GET /api/alerts` 仍走 `fetch_recent_alerts`，未動。

**A2. 三頁單號欄位獨立、多組、精確比對**（查件異常／請求記錄／佇列歷史）
- 共用 `commands/search_terms.rs`：`split_nos`（逗號／分號／頓號／空白／換行切分、去重）+ `in_clause`；三支 list 指令各加 `query_no`／`shipping_no`／`tracking_no` 參數走 `IN (...)`。關鍵字欄位維持 LIKE。
- 前端共用 `components/MultiNoField.vue`：攔 paste 把多行貼上轉空白（瀏覽器對單行輸入會直接吃掉換行、單號黏成一串）；`api/tauri.js` 的 `splitNos` 供 mock 同規則。
- 驗證：`search_terms` 單元測試 2、`parcel_alert_list` 整合測試 4（含精確不命中前綴、多組混分隔、疊加關鍵字／狀態）、`parcel_query_log_keyword` 3 全綠；`cargo check`、`vite build` 綠。**三頁畫面與貼上行為未實機看**（GUI 自動化已停）。

**C. 現場作業監控頁整合進桌面 App**（主人明確要求把 ship.cix3752i.com/field-operation-monitor 整頁搬進來，不只一個數字）
- 雲端 cix3752iWeb：控制器邏輯抽成 `App\Services\FieldOperationMonitorService`（`payload(from,to)` / `clampRange` / 進度看板 / 作業人員統計），網頁 `FieldOperationMonitorController` 變薄殼；新 API `GET api/v1/local-middleware/field-operation-monitor?from&to`（`LocalMiddlewareController::fieldOperationMonitor`）回同一份結構外加 `respond_code`。
- 中介端：config `cloud.field_operation_monitor_path`（預設 `/api/v1/local-middleware/field-operation-monitor`）、`CloudClient::fetch_field_operation_monitor`、command `cloud_field_operation_monitor`、`FieldOperationMonitorPage.vue`（路由 `/field-operation-monitor`、清關作業群組導覽）、雙語 i18n、web mock。
- 驗證：tinker 交易內造資料 → 網頁 `data()` 與 API 的 scopes 完全相等、區間裁到 3 天、rollback 後 0 筆；本機雲端新路由回 401（存在、需 token）；`cargo check`、`vite build` 綠。**頁面未實機看**。

**B. 清關進度浮動框加「貼單單數（去重）」**
- 口徑＝雲端「現場作業監控」頁的貼單單數：**今日業務日（06:00 起算）`order_print_log` distinct `order_sn`，不分廠別**；與浮動框的報關日區間無關。
- 雲端 cix3752iWeb（**未 commit、未部署**）：`OrderPrintService::stickerBusinessDayWindow()` / `stickerDistinctTotals()` 抽成共用，`FieldOperationMonitorController` 改用（口徑單一來源）；`clearance/progress` 回應多 `sticker: {business_date, package_num, order_num}`。
- 中介端：store `stickerOrderNum/stickerDate`、widget 多一列（綠色）＋業務日提示、mock、雙語 i18n。區間內某件**首次**列印 → 本機 +1；區間外的件要等下次重抓（重整／重開／重連）。
- 驗證：三檔 `php -l` 綠；tinker 交易內造資料（同單補印、物流貓、台中、非 U/E、業務日外）→ 去重／廠別／API `sticker` 全對，已 rollback；中介端 `vite build` 綠。**畫面未實機看**。
- **雲端未部署前**，浮動框拿不到 `sticker` 會顯示 0（不會報錯）。

**D. 套件升級與 Rust 稽核工具**（2026-08-29）
- `yarn upgrade` + `cargo update`（相容版本內）已提交;`cargo test` 64 綠、`vite build` 綠、`yarn audit` 0。
- 本機已裝 `cargo-audit`、`cargo-outdated`（Homebrew）。`src-tauri/.cargo/audit.toml` 例外 RUSTSEC-2023-0071（rsa,只被未啟用的 sqlx-mysql 宣告,任何 target 都不編進 binary）;其餘 19 筆為 unmaintained/unsound 警告,來源皆為 tauri 上游（glib/gtk Linux、urlpattern 的 unic-*）,本專案側無法處理。
- **跨主版號未升（需各自遷移,未做）**:axum 0.7→0.8、sqlx 0.8→0.9、reqwest 0.12→0.13、keyring 3→4、tokio-tungstenite 0.24→0.30、toml 0.8→1.1、tower-http 0.6→0.7、base64 0.22→0.23、imageproc 0.25→0.27、barcoders 1→2。keyring 4 的 feature 名稱已變（apple-native/windows-native/sync-secret-service 標 obsolete）,升時要對照 [[project_keyring_features]] 的坑。

**E. Vuetify 3.13 → 4.1.12 升級（分支 `vuetify-4`，未併入 main、未列入 v0.18.0）**
- 依官方升級指南（docs 原始檔 `packages/docs/src/pages/en/getting-started/upgrade-guide.md`）＋ `npx vuetify-codemods@latest --files "src/**/*.vue"`（字級 284、grid dense 20、elevation 2、select item slot 1）。
- 手動處理：Materio switch 尺寸 rem→px（v4 用 `- 12px` 算，單位不相容）；`shadow-key-umbra/penumbra/ambient` 25 級 map 改 v4 `$shadow-key/$shadow-ambient` 6 級，`mixins.elevation($z)` 把舊級距折半鎖 0-5；`$typography` map 換 MD3 key；Materio overrides 的 `.text-h*` 等選擇器換名；`vuetify-defaults.js` `color: undefined`→`null`（v4 略過 undefined）；`display.thresholds` 鎖回 v3（840/1145/1545 會讓側欄收合點位移）；`src/styles/vuetify-layers.css` 補回選擇性 CSS reset；**layer 順序宣告放 `index.html` head**（放 bundle 會被 vite-plugin-vuetify 的元件 CSS 搶先、被壓縮器合併，順序倒過來）。
- **關鍵發現**：Materio 的 `$typography`／多數 Vuetify 變數覆寫**從來沒作用到 Vuetify 自己的 utility class**（vite-plugin-vuetify 沒設 `styles.configFile`，`vuetify/styles` 是預編譯 CSS）；所以 v3 時 `.text-h5` 就是 Vuetify 預設 1.5rem。升級後為維持現況，`main.scss` 末段把 title-large／headline-large／display-*／body-large／body-small 鎖回 v3 數值。
- 驗證：`vite build` 綠、`yarn audit` 0、編譯後 CSS 確認 MD3 class 存在／舊 class 0 殘留／layer 宣告在 stylesheet link 之前。**畫面完全未實機看**（App 在主人另一個視窗後面，未強制前景）。`AppDateTimePicker.vue` 仍用 VInput slot 的 `.value`（專案未使用該元件，未動）。
- **二次驗證（2026-08-29 晚）**：把 Vuetify 3.13.2 實際 CSS 拉下來，對升級前 9 個有在用的字級 class（用量 102/101/24/23/21/6/4/2/1）逐一比對「尺寸／粗細／行高／字距／大小寫」→ 全部一致（誤差 ≤0.5px）。過程抓到兩個對照表本身就有損失的地方：h1、h2 併成 display-large（儀表板本場累計被放成 96px，已鎖回 h2 的 60px）；subtitle-1、body-1 併成 body-large（行高 28→24px，6 處改回舊名、由 main.scss 自備）。**教訓：升級／遷移不能只抽查，要逐項列出「舊→新→實測值」對照表，官方對照表是多對一時一定有損失。**
- 合併前必做：主人實機走一遍主要頁面（儀表板、掃描列印、件數核對、設定頁、對話框、深色主題），特別看陰影、標題字級、側欄收合、表格間距。切回 main 後要 `yarn install` 還原 v3 node_modules。

**F. v0.18.0 發版（2026-08-29 深夜）**
- `vuetify-4` 已 `--no-ff` 併回 main（`0b08316`），CHANGELOG 補 Vuetify 4 段落（`eae7766`），`v0.18.0` tag 指向該 commit，main 與 tag 已 push，Release CI run `33261527233` 進行中。
- 雲端 cix3752iWeb 的 `5cf657f7` 已在 origin/main 且正式站已部署（`ship.cix3752i.com/api/v1/local-middleware/field-operation-monitor` 回 401＝路由存在）。
- ✅ 2026-08-30 02:05（台北）五平台 CI 全綠，release 已公開，`latest.json` 版本 0.18.0、notes 正確。20.04 那條因 Actions 快取 7 天未用被清掉、整套重編 webkit 花了 2 小時（快取已存回，下次會快）。
- 8/13 那批註解對齊等未提交改動已從 stash 放回 main 工作區（無衝突），仍未提交。

### 尚未處理
- （v0.18.0 已全部發佈完成）；雲端要先部署，中介端再發版。
- 浮動框兩處視覺（新列、貼單列）需 `npm run tauri:dev` 看一眼。

## 2026-08-10 下午接手後做了什麼

### 已完成（外部狀態，不可回頭）

| 事項 | 結果 |
|---|---|
| 公開 v0.17.1 | 用 release ID `367648071` 指定公開（同 tag 曾有兩個草稿，用 tag 指定會挑錯）。已驗證 `latest.json` 回 `0.17.1`，macOS arm64 + Windows 四個更新目標齊全 |
| 刪除多餘草稿 | `367648069` 已刪。刪前逐一比對三包 Linux tarball 大小與正式那個完全相同，確認沒有獨有資產 |
| 廠商契約 `.docx` | 重產，內含 13 處直印相關段落（見上方第 1 點） |

### 未提交的程式碼變更（working tree，尚未 commit）

| 檔案 | 改了什麼 |
|---|---|
| `.github/workflows/release.yml` | **修掉重複 draft 競態**：新增 `create-release` job 作為唯一建立 release 的地方，desktop job 改吃 `tauri-action` 的 `releaseId`，linux job 改用 curl 打 uploads.github.com（移除 `softprops/action-gh-release`） |
| `src/api/tauri.js` | 補 11 個原本沒有 non-Tauri 分支的 API wrapper mock（cloudLogin / cloudFetchLabel / printImage / listPrinters 等），瀏覽器預覽不再半路壞掉 |
| `src/pages/QueueLogPage.vue` | 頁面內 mock 補上 `cancelled`（已攔下）樣本與中文攔截原因，**只動 mock 資料，未碰 `load()` 與版面** |

**`release.yml` 這次改動的驗證程度（重要，因為它只有下次發版才會真的跑到）**：

- 已讀 **tauri-action 上游原始碼**（`tauri-apps/tauri-action` repo 的 `src/index.ts`，**不是本專案的檔案**）證實有 `releaseId` input，且第 178 行是 `if (tagName && !releaseId)` —— 給了 id 就完全不走建立路徑，競態確實從根消除。
- `tagName` / `releaseBody` **刻意保留**：同檔第 211 行起會把它們傳進 `uploadVersionJSON` 當 `latest.json` 的 `notes` 與下載網址，拿掉會讓 App 內「發現新版本」的說明整個消失。
- 查找既有 release 的那段指令已在本機實跑（repo 現有 41 個 release、跨頁），`--paginate | jq -s 'add'` 合併正確、命中 `367648071`、不存在的 tag 正確回空字串。
- `-F body=@檔案` 語法已用無副作用的 `POST /markdown` 端點實測，確認真的讀進檔案內容（含中文與 markdown 結構）。
- linux job 那段 `node -e` 的 asset 查找已用真實 assets JSON 實測（`node -e` 的 argv 起算位置與一般腳本不同，是常見陷阱，這裡沒踩到）。
- **仍未驗**：整條 workflow 沒有實際跑過（要跑就得真的發一次版）。下次發版時請盯一下有沒有只產生一個 release。

### 這批變更的 Codex 覆檢結果（已全數處理）

覆檢抓出 1 Medium + 4 Low，逐項獨立驗證後 **4 項成立已修、1 項駁回**：

| 項目 | 判定 | 處置 |
|---|---|---|
| `create-release` 缺 `pipefail` | ✅ 成立（已實跑模擬） | 已修：兩個 step 改 `set -euo pipefail`，`jq \| head -1` 併成 `first(...) // empty` 單一 filter（避免 pipefail 下的 SIGPIPE） |
| `cloudFetchLabel` mock 回 `'success'` | ✅ 成立 | 改 `'LABEL-PROCESS'`（後端 `cloud_commands.rs:275`、前端 `ScanPrintPage.vue:188`），並補非空 `print_file_path` |
| `cloudFetchCloudPrint` / `cloudExaminePackage` 回 `'OK'` | ✅ 成立 | 改 `'PRINT-SUCCESS'` / `'FIND-PACKAGE-ORDER'`（`AutoPrintPage.vue:207,290`） |
| mock 登入狀態沒寫回 | ✅ 成立 | `cloudLogin/cloudLogout` 改為實際更新共用 `MOCK_SESSION` |
| 「文件寫 11 個、實際 14 個」 | ❌ 駁回 | 覆檢抓的是移除三個死碼 mock **之前**的快照；實數就是 11 個 |

**`pipefail` 那項為什麼是 Medium**：`gh api | jq` 沒有 pipefail 時，上游 API 只要暫時 5xx / rate-limit，pipeline 仍會被判成功、`RELEASE_ID` 取到空字串 → 誤判「沒有既有 release」而重建 —— **等於把這次要修的重複 draft 問題換一條路再犯一次**。

**mock 狀態碼那三項的真正教訓**：這個檔案既有的 mock（`FIND-PACKAGE-ORDER` 等）本來就用後端真實的 code，新加的卻用 `'OK'` / `'success'`，**消費端一個都認不得**，等於補了 mock 卻讓瀏覽器預覽全部落進錯誤分支。日後補 mock 一律去對照消費端實際判斷的字串，不要自己編一個看起來合理的值。

**這批變更對 Tauri 實機路徑的影響 = 零**：所有新增都關在 `if (!isTauri)` 分支內，已用 `git diff` 逐字比對 11 個 `invoke()` 呼叫，新舊完全一致。

---

## 2026-08-13：全專案註解對齊「只寫這段在做什麼」規則

### 做了什麼

把 `src-tauri/src/`（39 個 .rs）與前端自寫程式（`pages` / `components` / `composables` / `stores` / `api` / `config` / `utils`）**全部約 2,600 行註解逐行讀過**，改掉其中違反規則的三類寫法：

| 類型 | 原本的寫法 | 改後 |
|---|---|---|
| 變更歷程 | 「原本 X、後來改成 Y」「舊行為是…」「已於某版移除」 | 只留現行約束：「**不可**用 X，會…」 |
| 決策／審查過程 | 測試註解裡的 `finding[4]`、`finding[479a/479b]` 編號；「先 hardcode 驗證 provide 有效」 | 改成描述該測試在驗什麼 |
| 開發階段編號 | 「階段 2:操作人員分組」「階段 3-4 - 失敗率」（後端 4 處 + 前端 5 處） | 直接寫功能名 |

**判斷準則**：把註解遮住只看程式，讀者還需要知道什麼才不會用錯 —— 只留那個。因此以下**刻意保留**：

- 現行相容性（「舊版雲端未回 code 時 fallback 成狀態碼字串」「localStorage 可能存有單值字串格式」）—— 那是現在還得處理的輸入，不是歷史。
- 會靜默失敗的陷阱（`?N` 參數編號、原子寫入、GDI 1-bit DIB 三條件、AAC 在 Linux 無法解碼）。
- 跨檔／跨專案同步紀律（SQL 常數與測試共用同一份、入倉頁與雲端行為需一致）。

順手修掉 3 處簡繁混用（「對齐」→「對齊」）。

### 驗證

- `cd src-tauri && cargo check` 通過。
- `yarn build` 通過。
- `git diff` 逐行過濾確認：本次動到的檔案**只有註解行變更，零程式碼行**（`src/pages/QueueLogPage.vue` 與 `src/api/tauri.js` 的程式碼差異是 8/10 就已存在的未提交變更，非本次）。

### 未處理（有明確原因）

| 範圍 | 為什麼不動 |
|---|---|
| `src-tauri/migrations/*.sql`（0011 / 0013 / 0017 / 0022 / 0024 / 0026 / 0027 有歷程性註解） | **sqlx 會校驗 migration 檔的 checksum**，改註解等於改檔案內容 → 已部署機器啟動時會因 hash 不符報 `VersionMismatch` 而起不來。這些註解只能在未來新增 migration 時避免重蹈，不能回頭改。 |
| `src/@core`、`src/@layouts`、`src/styles/@core`、`src/plugins/vuetify-materio` | Materio 樣板原廠碼（含上游的英文 `TODO`）。改了只會擴大與上游的 diff，且那些 TODO 是上游的現行待辦、不是本專案的歷程。 |

---

## v0.17.0 → v0.17.1 這兩版改了什麼

### 起點問題

面單路徑改成 `direct_print`（本機直接列印）後，雲端完全收不到貼標人員。**根因不是設定掉了**，而是整條回報鏈的唯一觸發點消失：回應不含 `label_path` → 工控機不回報 → 佇列沒東西 → 雲端沒紀錄。

### v0.17.0：補上回報鏈

- 直印**列印成功後**自補一筆回報，但延後送出（寬限秒數，預設 10、上限 600），留時間給工控機回報；等不到才兜底送出。
- `post_report` 與自補走**同一句 UPSERT**（`response_id` 唯一索引），無論誰先到都只有一列、只推一次。
- 佇列歷史頁新增「回報來源」欄與「只看工控機未回報」篩選。
- 順帶修掉：工控機重複回報會推兩次、寬限秒數設過大反而變成立即送出。

### v0.17.1：列印失敗要攔下回報

面單沒印出來，雲端就不該記成完成。五個失敗來源（下載／讀檔／送印／佇列已關閉／通道漏設印表機）全部接上攔截。

- **沒有既有列時先立墓碑** —— 否則工控機稍後才回報會變成一筆全新的待送被推出去。
- **`cancel_requested` 旗標**（migration 0028）把「有人要求攔截」與「推送到哪了」拆成兩個獨立事實。只靠 status 擋不住「攔截時該筆正在 sending」：那次推送若失敗，重試邏輯並不知情會照送。
- worker claim 改**帶條件的 compare-and-set**（原本無條件 UPDATE 會把 cancelled 蓋回 sending 照常送出）。
- 推送失敗→定案攔下不重試；推送成功→維持 success 並發「無法撤回」告警（sending 期間不預判送達與否）。
- 重印成功會**自動解除攔截**（含前次推送尚未收斂的中繼態）。
- 安全通報：`quinn-proto` 0.11.14→0.11.16、`nanoid` 3.3.16→3.3.18（皆為建置工具鏈間接相依、不進最終產物；nanoid 以 `resolutions` 覆蓋，上游升上去後可移除該條目）。

### 重要檔案

| 檔案 | 內容 |
|---|---|
| `src-tauri/migrations/0027_report_queue_source.sql` | 加 `source` / `ipc_reported_at`，`response_id` 建唯一索引（先去重） |
| `src-tauri/migrations/0028_report_queue_cancel_requested.sql` | 加 `cancel_requested` 旗標 + `idx_report_queue_sendable` |
| `src-tauri/src/queue/mod.rs` | 四段 SQL 常數（自補／工控機回報／攔截／claim）+ worker 收斂邏輯 |
| `src-tauri/src/server/mod.rs` | `report_direct_print_failed` 接上攔截、`fetch_channel_sticker` 三處共用 |
| `src-tauri/tests/report_queue_direct_print_merge.rs` | 21 個回歸測試，**直接引用實作的 SQL 常數**，不另抄一份 |
| `src/pages/QueueLogPage.vue` | 回報來源欄、已攔下狀態、攔截原因 tooltip、未回報篩選 |
| `docs/local-http-api.md` | 廠商契約：直印仍須回報、攔截行為、完整時序 |

---

## 已驗證 / 未驗證

### 已驗證 ✅

- **21 個新回歸測試** + 34 既有單元 + 3 既有整合，全過。
- **真實 HTTP 端到端**（啟動 App、實際打 `POST /api/report`）：寬限內回報、全新回報、重複回報、遲到回報四種情境。
- **migration 0027 / 0028** 在真實資料庫副本上跑過，含人造重複資料驗證去重規則。
- `cargo check`、前端 `yarn build`、`yarn install --frozen-lockfile` 通過。
- 兩輪獨立 code review（Codex），指出的實質缺陷全數修正並補測試。

### 2026-08-10 下午補驗 ✅

**前端畫面已完成目視**（原列為未驗證第 2 項，現已補上）。

先前卡在「Tauri 視窗截不到圖、Chrome 擴充連不上本機位址」，繞法是**改用 Playwright 直接驅動系統 Chrome**，不經瀏覽器擴充：

```bash
npx vite preview --port 4173 --strictPort
playwright screenshot --channel=chrome --viewport-size=1440,900 --full-page "http://localhost:4173/#/queue-log" /tmp/out.png
```

- `--channel=chrome` 是關鍵：本機 Playwright 版本與已下載的 chromium 版本對不上，走系統 Chrome 直接可用，不必另外 `playwright install`。
- 實際看到的結果：「已攔下」徽章為**灰色**（不是紅色，對齊「這不是出錯，是刻意攔下」的設計）、該列**推送完成欄為空**、**重試次數 0**，語意都正確；回報來源三種樣態（工控機已回報 / 僅中介機自印 / 工控機回報遲到）都能呈現。
- 攔截原因 tooltip 需要 hover 才出現，靜態截圖驗不到，另寫了 Playwright 腳本 hover 後讀取內容，確認確實浮出對應的中文原因。
  - 兩個踩過的坑記著：圖示是 iconify 轉成的 SVG，**class 不帶 icon 名稱**選不到，要改抓 tooltip 觸發器所在的 `td[aria-describedby^="v-tooltip"]`；另外頁面上同時有別的 tooltip（側邊欄收合提示），**不能抓「第一個」**，要順著 `aria-describedby` 的 id 定位，否則會驗到錯的那個。
- **這項驗證的限制**：跑在瀏覽器預覽模式、資料是頁面內建 mock，驗的是**版面與顯示邏輯**，不是 Tauri 實機的真實資料流。

### 未驗證 ⚠️（接手請補）

1. **完整實機直印路徑**（正常訂單 → 直印成功 → 自補入列）**沒跑通** —— 雲端測試站當時手邊的單號都回「訂單狀態異常」，只會走錯誤面單分支。要驗需要一筆狀態正常的訂單，且會**真的印出一張紙**。
2. **攔截機制的現場行為**未在真實印表機故障情境下驗過（測試以 SQL 層模擬）。
3. **修好的 `release.yml` 沒有實際跑過** —— 下次發版時盯一下是不是只產生一個 release（詳見上方「未提交的程式碼變更」）。

---

## 已知問題與風險

| 項目 | 說明 | 嚴重度 |
|---|---|---|
| ~~CI 競態產生重複 draft~~ | **已修但尚未 commit**：統一由 `create-release` job 建立，其餘只上傳。下次發版盯一下是否只產生一個 release | 🔵 待實地驗證 |
| 工控機未回報 | 見上方「立刻要處理」第 1 點 | 🟡 外部依賴 |
| 佇列歷史頁的 mock 不在 API 層 | `QueueLogPage.vue` 頁面內自建 `MOCK_QUEUE`，非 Tauri 時走自己那份、**不會呼叫** `api/tauri.js` 的 `queueList`。所以 `queueList` 那邊**刻意不放 mock**（放了也執行不到，只會變成第二份會腐化的假資料）。專案架構意圖是 mock 集中在 API 層，日後要統一的話：移除頁面的 `if (!isTauriRuntime)` 分支、改走 `queueList`，並把假資料搬過去。本次刻意不動，因為該頁顯示邏輯 v0.17.1 才剛隨版本發出去，為了消重複而動它不划算 | 🔵 |
| 極窄的重複告警 | 推送成功分支與攔截呼叫端可能各發一次「無法撤回」事件記錄，內容幾乎相同。不影響資料，審查判定可接受，未修 | 🔵 觀察面 |
| GitNexus index 過期 | 停在 `ee00be2`，需要時跑 `node .gitnexus/run.cjs analyze` | 🔵 |

### 升級注意

v0.17.1 啟動時會自動跑 **migration 0027 + 0028**。0027 會對 `report_queue.response_id` 建唯一索引，**建立前會刪除重複列**（保留已成功那筆、其餘取最新）。現場佇列通常接近空，影響極小，但升級前建議備份 `app.sqlite`。

---

## 開發機殘留（測試造成，非程式問題）

驗證期間在**開發機**（非現場）留下的痕跡，接手若看到不用緊張：

- 開發機 `app.sqlite` 已跑過 0027 / 0028，測試用的佇列資料**已清乾淨**。
- 測試時誤觸發過一次真實列印（直印模式下打 `GET /api/parcel`，該單回錯誤面單 → 中介機照設計送印）。工作已取消，印表機佇列已清空。**教訓：直印模式下打 `/api/parcel` 會真的出紙。**
- 背景 worker 曾把 3 筆測試回報推到雲端測試站（`local-18001.build-site.dev`），用的是本機既有的舊 `response_id`。

---

## 下一步建議（依優先序）

1. **把 `docs/local-http-api.md`（或重產後的 `.docx`）給工控機廠商**，請其在直印模式下照常回報 —— 這是目前唯一還會實際影響使用者的缺口。
2. **commit 目前 working tree 的變更**（release.yml、兩個前端 mock 檔、本文件、重產的 docx，以及 8/13 那批純註解調整的 37 個原始碼檔）。
3. 現場升級後，觀察「佇列歷史」頁的回報來源分布，作為調整寬限秒數的依據（預設 10 秒；遲到比例高就調長）。
4. 補上剩餘未驗證項（實機直印路徑、印表機故障現場行為）—— 兩項都需要實體印表機與一筆狀態正常的訂單，無法在開發機純軟體驗證。
5. 下次發版時確認 `release.yml` 只產生一個 release。

---

## 執行方式備忘

```bash
npm run tauri:dev                      # 開發（Vite HMR + Rust 自動重編）
cd src-tauri && cargo check            # 改 Rust 後優先跑這個
cd src-tauri && cargo test             # 全部測試（含 21 個回報佇列回歸測試）
yarn build                             # 前端純 build
npx vite preview --port 4173 --strictPort         # 瀏覽器預覽（配下面那行做畫面目視）
playwright screenshot --channel=chrome --full-page "http://localhost:4173/#/queue-log" /tmp/out.png
bash tests/docker-ubuntu-build.sh 22.04 install   # 跨 distro 驗證（需 Docker）
```

- 套件管理一律 **yarn**（非 npm），CI 與 docker 腳本皆走 `yarn install --frozen-lockfile`。
- 發版四步驟見 `CLAUDE.md`：先寫 CHANGELOG → 三檔版本號 → commit/tag/push → **手動公開**。
- 資料庫變更一律新增 migration，**不改舊檔**。
