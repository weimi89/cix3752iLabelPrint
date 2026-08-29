# 交接紀錄

> **接手前先讀這份，但不可憑它直接動手** —— 它可能落後於程式碼。先跑測試與實查確認現況，與現況不符時**以現況為準並回頭修正這份文件**。
>
> 這是「快速接手」用的單一位置，持續更新同一份、不另開新檔。
> Roadmap 與歷史經驗在 `docs/next-steps.md`；工控機對外契約在 `docs/local-http-api.md`。

最後更新：**2026-08-13（註解全面清理）**　目前版本：**v0.17.1（已正式公開）**

---

## ⚠️ 立刻要處理的事（未完成，會影響使用者）

### 1. 工控機廠商需配合修改（外部依賴，中介端無法自行解決）

直印模式下工控機**仍必須** `POST /api/report`。目前它多半綁在「有沒有拿到 `label_path`」上，該欄位在直印模式不存在 → 整段跳過。

- 契約已寫進 `docs/local-http-api.md` 第 2、3 節（含完整時序表），可直接給廠商。
- **`.docx` 已在 2026-08-10 下午用 pandoc 重產**（舊檔備份於 `backups/20260810135454/`）。先前它停在 v0.12.0、**不含直印回報契約** —— 若照舊檔寄出，廠商會照錯的規格實作。要寄檔案版就寄現在這份。
- 廠商未修好之前，桌面 App「佇列歷史」頁的**回報來源欄會整批顯示黃色「僅中介機自印」** —— 這是預期現象，不是故障。修好後會轉綠色「工控機已回報」。

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

### 尚未處理
- 兩 repo 都未 commit；雲端要先部署，中介端再發版。
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
