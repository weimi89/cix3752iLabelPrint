# 交接紀錄

> **接手前先讀這份，但不可憑它直接動手** —— 它可能落後於程式碼。先跑測試與實查確認現況，與現況不符時**以現況為準並回頭修正這份文件**。
>
> 這是「快速接手」用的單一位置，持續更新同一份、不另開新檔。
> Roadmap 與歷史經驗在 `docs/next-steps.md`；工控機對外契約在 `docs/local-http-api.md`。

最後更新：**2026-09-10（v1.0.0 已公開發佈）**　目前版本：**v1.0.0（已發佈，九項產物齊全，`latest.json` 生效）**

---

## 2026-09-10：v1.0.0 發版（網頁版全功能 + 對外存取控制）

版本號選 **v1.0.0** 而非 0.24.0:這一版把產品從「桌面工具」變成「桌面 + 網頁雙入口的服務」,
並首次具備對外開放能力。

### 發版狀態

- CHANGELOG 已寫(標題 `## v1.0.0`,`release.yml` 靠這個 regex 抽段落注入 release notes 與 `latest.json`)
- 三處版本號已改並過 `cargo check`(`package.json` / `tauri.conf.json` / `Cargo.toml`;`Cargo.lock` 自動同步)
- commit `891b1d9`,tag `v1.0.0`,兩者都已推上 origin
- **已公開發佈**(2026-09-10 08:53 UTC):CI 六個 job 全綠含 `verify-assets`,九項產物齊全,
  `latest.json` 匿名可取、四個平台的自動更新已生效。現場機器會開始收到更新提示。
- 這一版第一次驗到:`build.rs` 的 dist 佔位與 rust-embed 嵌入在三平台 CI 上都能正常打包

### 這一版尚未驗證的事（發版後請補）

1. **真正從外網連**:Port Forward + DDNS 這條實際路徑沒走過(先前是用改網段模擬)
2. **桌面 App 點一輪**:動到 19 個檔案的環境判斷與 28 處事件送出點,編譯與量測都過,但畫面要人點
3. **網頁版的完整業務流程**:驗過「頁面開得起來、版面不破、RPC 通、事件收得到、通道開關真的寫進資料庫」,
   **沒有**跑完整的掃描出單 / 自動印單 / 清關 / 入倉驗單
4. **三平台打包產物**:這版新增 `build.rs` 的 dist 佔位與 rust-embed 嵌入,會影響打包,CI 行為第一次驗
5. **越南語沒有母語者校對**(測試只能擋鍵不齊與空字串,擋不了翻譯品質)

---

## 未提交（2026-09-10，續五）：把「已知限制與日後風險」逐項處理掉

### 逐項結果

| 原本的風險 | 處理 |
|---|---|
| 其餘 75 支 command 未逐一檢視 | 依「觸及本機資源」分類掃過全部 76 支,只有 6 支觸及(相機 2、印表機 2、`print_image`、`update_config`)。**掃描本身找到一個 Codex 沒抓到的**:見下 |
| 表格加欄位要補 `data-label` | 改成測試守門(`tests/maintenance-guards.test.mjs`),漏補或標錯欄會直接失敗 |
| 新增 command 要補三處 | 前端呼叫但後端未註冊 → 測試擋下。(後端↔RPC 分派表原本就有 `registry_sync` 守著) |
| 首次載入偏慢 | 打包切分:圖表庫獨立成 `vendor-charts`,統計頁 chunk 從 574 KB 降到 29 KB —— **不進統計頁的人不必載它**。首次載入時間本身沒改善(瓶頸不在 chunk 數),如實記錄 |
| 外網權限有三個例外 | 寫進設定頁畫面,使用者看得到是哪三件事、為什麼 |
| 越南語沒人校對 | 仍未解決(需要母語者)。已加測試擋「鍵不齊」與「非刻意的空字串」,但**翻譯品質本身測不出來** |

### 掃描找到的新問題：`update_config` 能改快取目錄

`assert_not_protected_dir` 只擋磁碟根、掛載點根與 8 個受保護資料夾,**沒有限制在 app_data 內**。
快取清理會**遞迴刪除**設定的目錄,`/images` 又直接服務它 —— 外網登入者把它指到別的目錄,
就能讓清理器把裡面刪光。已比照 `web_access` 一併鎖成只准內網改(存證目錄同理)。

### 順手發現並修掉：外網一斷，畫面上每個圖示都變空白

`@iconify/vue` 找不到本地圖示資料時會去 `api.iconify.design` 線上抓,而專案**完全沒有離線載入設定**。
中介機為了打雲端 API 本來就有外網,現場才一直沒踩到 —— 但這個 App 有三層網路偵測就是為了撐過斷網,
UI 不該在那時候先瞎掉。實測(擋掉所有對外請求):修正前 48 個圖示全空白,導覽列看不出有哪些按鈕。

修法是**只打包實際用到的**:`scripts/build-icon-subset.mjs` 掃描原始碼取出 176 個圖示,
產生 45 KB 的子集。整包 tabler 有數千個、好幾 MB,而首次載入才剛壓下來,不能為了這個又加回去。
修正後對外請求數 **1 → 0**,圖示全部正常。新增圖示後要重跑 `yarn icons`,忘了會被測試擋下。

### 快取策略改成三級（前一版矯枉過正）

前一輪為了「重錄語音後不要讓人聽到舊的」把非 `assets/` 全設成 `max-age=0, must-revalidate`,
結果**字型也被拖下水** —— 字型每頁都要用、檔名又帶版本號,每次重新驗證等於低速網路下白花好幾個來回。

現在分三級:帶雜湊的產物鎖一年、字型 30 天、其餘(音檔等)一小時。
一小時足以解決「重錄後聽到舊的」,又不必每次回頭問。

---

## 未提交（2026-09-10，續四）：第二層（Codex）覆檢修正 5 項

第二層走的是 `codex:codex-rescue` subagent（規範裡 AI 可自行發起的那條管道;
`/codex:review` 斜線指令那次卡死在 graphiti MCP 呼叫,見下）。抓到的都是**前兩輪自審看不到的類型**
—— 站錯的前提與跨情境語意,不是機械性缺陷。

| # | 位置 | 問題 |
|---|---|---|
| 1a | `auth.rs` `login` | **登入鎖定有併發繞過(TOCTOU)**。「查有沒有被鎖 → 驗密碼 → 記失敗」是三次獨立 DB 往返,同一來源同時開 N 條連線會全部在任何一次失敗被寫入前讀到「未鎖定」,等於一口氣免費猜 N 次,`max_fail_attempts` 形同虛設。修法:`LOGIN_LOCK` 序列化整段(登入是低頻操作,argon2 本身就要上百毫秒,不影響現場) |
| 1b | `web_auth_commands.rs` | **換密碼沒鎖 LAN-only,也不驗舊密碼**。這一期刻意沒有 TLS,對外那段 session cookie 是明文 —— 攔到 cookie 的人不只能讀資料,還能把共用密碼換掉、把真正的操作員永久鎖在外面(偷 session 會隨到期失效,換掉密碼不會)。修法:比照 `update_config` 只准內網 |
| 3a | `printer_commands.rs` | **`print_image` 的本地路徑分支等於任意檔案讀取**。`image_path` 非 http(s) 就直接 `std::fs::read`,對本機 IPC 無害,但這支現在也對外開放 —— 拿到密碼的人可以叫伺服器讀它讀得到的任何檔案餵進印表機,而且讀檔失敗訊息原樣回傳,附送路徑探測管道。修法:外網只放行「指向本機 server 的圖片網址」(順帶擋掉叫伺服器打任意外部網址) |
| 3b | `cloud_commands.rs` + 兩個頁面 | **面單預覽在網頁版整個載不出來**。後端產生的 `print_file_path` 寫死 `http://127.0.0.1:{port}`,遠端瀏覽器會把它解成自己那台。掃描列印／預產頁直接把它綁進 `<img :src>`,沒走 `api/media.js`。修法:新增 `toViewableUrl()` **只用在顯示**;送去列印時保持原樣(那是伺服器自己讀,對它而言 127.0.0.1 正是自己) |
| 4 | `events.rs` | SSE 只在**建立連線當下**判斷一次是不是內網。管理員事後收窄 `lan_cidrs`,那條已開著的串流會永遠繼續推包裹資料。修法:內外網與 session 都改成每次 tick 重算 |

### 額外一項（Codex 標為「確認一下是刻意還是副作用」）—— 是副作用，已修

`is_cross_site` 套用在所有路徑,而**從 LINE / Email 點連結進來的頂層導覽,瀏覽器標的就是 `cross-site`**
—— 會被直接 403,連登入頁都看不到。而「把網址傳給主管」正是這套網頁版的主要用法。

放行頂層導覽不會開後門:CSRF 怕的是網頁在背景替使用者發請求,而頂層導覽是使用者主動離開原網站、
把整個分頁換成我們的頁面,結果不會回傳給原網站;cookie 又是 `SameSite=Strict`,跨站導覽本來就不帶,
進來仍要重新登入。條件收緊為 `Sec-Fetch-Dest: document` + `Sec-Fetch-Mode: navigate`,
`<img>`／`<iframe>`／`fetch` 一律照擋(已加測試)。

### 實測

- 分享連結:頂層導覽 200,`<img>`／`<iframe>`／`fetch` 全部 403
- 外網換密碼 403;外網用 `print_image` 讀 `/etc/passwd`、讀設定檔、打外部網址皆 403,
  指向本機的面單網址通過守門
- **併發鎖定**:同時發 20 個錯誤密碼 → 恰好 5 次 401、15 次 429(未修時 20 次都會是 401)
- 測試造的密碼、session、鎖定記錄、安全事件與設定全部還原

### `/codex:review` 斜線指令那次為什麼卡死

卡在 `Calling graphiti-memory/hybrid_search`,41 分鐘無輸出。診斷要點(對齊 `code-review-policy`):
job 狀態欄位仍顯示 `running`,**不可信**;要看的是行程是否存活(存活=卡住,消失=死了)與 log 最後寫入時間。
`Calling X` 沒有配對的 completed,那個就是卡點。

graphiti 服務本身沒壞 —— 停掉 codex 後 `initialize` 立刻回 200(5.6ms),Neo4j 正常,PM2 顯示重啟 0 次。
graphiti 的 log 裡**完全沒有那筆請求的紀錄**,表示卡在 MCP session 銜接,而 codex 對這條呼叫沒有逾時保護。
**不需要重啟 graphiti。** 改走 subagent 管道即順利完成。

---

## 未提交（2026-09-10，續三）：第二輪覆檢修正 8 項

第一輪修完之後又加了手機排版、表格卡片式、壓縮與動態載入,這批送了第二輪覆檢,又抓到 8 項。

| # | 位置 | 問題 |
|---|---|---|
| 1 | `auth.rs` guard 順序 | **高｜外網登入者可以把自己劃進內網**。`guard` 是「內網直接放行」在前,而 `lan_cidrs` 屬於 `AppConfig`、`/rpc/update_config` 有 session 就能呼叫 —— 外網登入一次 → 把 `lan_cidrs` 改成 `0.0.0.0/0` → 從此全世界都是內網,`is_lan` 先行 return,對外開關與「工控機 API 只認內網」兩道門完全不會執行。修法:非內網來源不得修改 `web_access`(桌面走 IPC、內網來源不受限) |
| 2 | `events.rs` | **SSE 只在連線當下驗一次身分**。換密碼會 `DELETE FROM web_session`(註解寫「把所有人請出去」),但對方已開著的串流照樣繼續收包裹資料。修法:外網連線每 60 秒回頭驗一次 token,失效即收線 |
| 3 | `mod.rs` 相機串流 | MJPEG **沒訂閱 `close_tx`**,與 `close_tx` 欄位註解說的不符。設定頁開著預覽時改 port 存檔,graceful shutdown 等不到它,每次都撐滿 3 秒逾時走強制中止 —— 那條路徑是保底用的,不該變成常態 |
| 4 | `assets.rs` | **非雜湊檔名被標 immutable**。`sounds/`、`static/` 來自 `public/`,檔名固定。重錄設備異常語音後,開過網頁版的瀏覽器會繼續播舊音檔長達一年。修法:只有 `assets/` 給 immutable |
| 5 | `auth.rs` `is_cross_site` | **`Origin` 防不了 `<img>` 跨站 GET**。`<img src="http://中介機:18080/api/parcel/XXXX">` 不帶 `Origin`,會讓中介機真的去查件、直印模式下還會送印。修法:改看 `Sec-Fetch-Site`(瀏覽器填、網頁改不掉,而且這類請求一定會帶),`same-origin`/`none` 放行,其餘擋;舊瀏覽器退回比對 `Origin` |
| 6 | `FieldOperationMonitorPage.vue` | 合計列的手機欄名標錯(見下) |
| 7 | `rpc.rs` | 兜底 `other =>` 擠在前一臂同一行。`registry_sync` 守門測試是**逐行**解析的,照著那行加新指令會被靜默略過 |
| 8 | `api/events.js` | 退避重連被繞過:`onerror` 設好計時器後,只要有頁面在視窗內掛新監聽就會立刻再開一條,退避形同虛設 |

### 第 6 項的教訓：驗證工具與被驗證工具共用假設，等於沒驗

合計列第一格是 `colspan="3"`,我的轉換腳本「跳過 colspan 的格子」卻**沒有把欄位索引推進 3**,
後面兩格因此標成第 1、2 欄。真正的問題是:**我寫的檢查腳本用了同一個錯誤假設**,
所以跑出來是「全部對應正確」—— 自己驗自己,錯誤被複製到驗證端就再也看不見。

現在檢查腳本會依 `colspan` 推進索引,而且逐列檢查(不只第一列 —— 合計列的欄數本來就與資料列不同)。
**日後表格若再加跨欄的格子,要確認這兩支腳本都跟著處理。**

### 實測（用區網 IP 模擬外網，127.0.0.1 當退路）

`is_lan` 有「loopback 永遠算內網」的例外,所以無法用 127.0.0.1 模擬外網。
改用本機區網位址(把 `192.168.0.0/16` 移出 `lan_cidrs`),而 127.0.0.1 仍是內網 —— 那就是還原的退路,
不會把自己鎖在門外。

驗到的結果:外網登入者改 `lan_cidrs` 回 403;**但改其他設定、操作通道、查統計全部正常 200**
(使用者定的「填密碼就有完整操作權限」不變,只有門鎖本身不能從門外改);工控機 API 對外網仍 403;
跨站 GET/POST 皆 403,而 `same-origin`、`none`(掃 QR、直接開網址)與工控機(不送 `Sec-Fetch-*`)都正常。

測試造的密碼、session、鎖定記錄、安全事件與被改動的 `cloud.job_user` **已全部還原**。

---

## 未提交（2026-09-10，續二）：手機遙控的去留與載入成本

### 手機遙控功能沒有消失，而且變多

舊的 `/control` 是 810 行手刻頁，能做的事只有：看通道、暫停／跳過通道、看查件異常、看物流商與貼標人員。
現在整套 21 頁都能在手機上操作,上述功能全部涵蓋在「分揀通道」頁裡。

- `/control`、`/board` 舊網址改為 **303 導向**到對應頁,現場貼的 QR 與書籤不必重貼。
- 舊頁靠的那幾支 API(`/api/channels`、`/api/alerts`、`/api/dispatch-providers`、`/api/sticker-history`)
  **原封不動保留**,沒有跟著舊頁一起下架。
- 已用手機尺寸的瀏覽器實際點過:點通道開關 → 觸發 `sort_channel_set_enabled` → 資料庫確實改變(驗完已還原)。

### 這次抓到的 bug：index.html 被 Lazy 快取住

`assets.rs` 原本用 `once_cell::Lazy` 把處理過的 index.html 快取住。Vite 的檔名帶內容雜湊,
**前端一重新建置檔名就換一組**,而記憶體裡那份還指著上一版 —— 請求落到 SPA fallback 拿回 index.html,
瀏覽器把 HTML 當 JS 執行,結果是白畫面而且主控台看不出原因。

只影響開發模式(正式版的 dist 是編譯期嵌入的,兩者同時定版),但開發時每改一次前端就壞一次,很難聯想。
修法:debug 每次重讀、release 維持快取。**並加了測試 `index_指向的資產都取得到`**,
以後 index.html 與資產不同步會直接測試失敗。

### 載入成本（實測，不是估計）

現場手機關心的是開得快不快,量了才知道:

| 情境 | 時間 |
|---|---|
| 本機（區網） | 566 ms |
| 模擬現場 Wi-Fi（4 Mbps / 80 ms），**首次** | 約 7.4 秒 |
| 模擬現場 Wi-Fi，**再次開啟（有快取）** | **392 ms** |
| 有快取後切到分揀通道頁 | 951 ms |

**現場的日常是第二欄以後的數字** —— 資產都帶內容雜湊、快取一年,只有第一次進站要等。

做了兩件事把首次載入壓下來:

1. **回應壓縮**(`CompressionLayer`):JS 1656 KB → 538 KB、CSS 494 KB → 69 KB。
   **刻意排除事件 SSE 與相機 MJPEG 預覽** —— 壓縮器要累積到一定量才吐資料,套在串流上會讓事件延遲、
   預覽一頓一頓,而這兩者都是「即時」才有意義。(內建的 DefaultPredicate 只排掉 SSE,multipart 要自己排。)
2. **頁面改動態載入**:主檔 1656 KB → 463 KB。統計頁含圖表庫獨立成 574 KB 的 chunk,
   不進那一頁就不會載到。

> 高延遲下動態載入的效果有限(請求數從 17 變 31,每個都要多一次往返),4 Mbps 下只差半秒。
> 真正的價值在「只開一兩頁的人不必載入整包」。若日後還要再快,下一步是把 Vuetify 的碎 chunk
> 合併成少數幾個 vendor 檔(目前 51 個 JS),而不是退回去維護第二套輕量頁面。

---

## 未提交（2026-09-10，續）：覆檢修正 7 項 + 手機版排版

### 覆檢抓到的問題（已全部修完）

| # | 位置 | 問題 | 修法 |
|---|---|---|---|
| 1 | `server/mod.rs` `ServerHandle::shutdown` | **重啟伺服器會永久卡死**。axum graceful shutdown 會等所有連線收工,而事件 SSE 與相機預覽永遠不會自己結束;更糟的是觸發重啟的請求往往就跑在那台 server 上(網頁版按重啟、或存下改了 port 的設定),等於自己等自己 | 加 `close_tx` 廣播讓長連線主動收線,再以 3 秒逾時強制中止釋放 port |
| 2 | `ServerSettingsPage.vue` | **內網網段改了存不進去**。畫面顯示「已儲存」但設定檔一個字都沒變 —— 而工控機正是靠這份清單被認定為內網,存不進去等於改了沒用 | 存檔前把多行文字轉回陣列,存完再用後端回傳值灌回畫面 |
| 3 | `server/auth.rs` | **內網瀏覽器可被任意網站借刀**(CSRF)。內網靠 IP 放行、沒有任何權杖,現場電腦開到惡意網頁就能被送出 `cache_clear`、`server_restart` 等不吃參數的指令 | 擋跨站 `Origin`(瀏覽器自己填、網頁改不掉)+ `/rpc` 強制 `application/json`(逼出預檢,而本服務不回 CORS 標頭)。**兩道都放在「內網放行」之前** —— 被攻擊的正是內網使用者 |
| 4 | `server/auth.rs` | **開機瞬間的請求會 panic**。HTTP server 在 bootstrap 早於 `manage(state)` 啟動,空窗期打進來的請求會打爆該連線的 task | 改 `try_state()`,拿不到回 503 |
| 5 | `useNetworkStatus.js` | 網頁版**網路燈號凍住**。12 處守衛都改成 `hasBackend` 了,漏掉這一處 | 改 `hasBackend` |
| 6 | `useWebAuth.js` / `LoginPage.vue` | **後端一重啟,內網使用者也被踢到一個按不下去的登入頁**(連線失敗被當成未登入,且 `passwordSet` 停在 false 讓密碼欄與送出鈕雙雙 disabled) | 區分「連不上」與「未授權」:連不上不動登入狀態,登入頁改顯示連線提示 + 自動每 3 秒重試,連上就自動進入 |
| 7 | `server/auth.rs` | 鎖定查詢失敗時**放行**,資料庫一忙暴力破解保護就靜默失效 | 查不出鎖定狀態改回 503 拒絕;`record_fail` 失敗留 warn |

第 6 項在實測時親眼撞到:改 Rust 觸發重編,瀏覽器就被踢到登入頁 —— 這在現場等於「每次改設定都要重新登入,而且還登不進去」。

### 手機版排版

**病灶集中在共用元件,不是各頁各自的問題:**

- `.app-header-card` 沒有 `flex-wrap`,右側按鈕會把標題壓成一字一行的直排。分揀通道有四顆按鈕(442px),
  直接把整個版面撐到 576px、逼瀏覽器把全頁縮小 —— 這就是「擠在一起」最主要的來源。
  修法:`flex-wrap` + 文字區 `min-inline-size: 0`(flex 子項預設不會縮到比最長詞窄,不歸零就寧可撐破也不折行),
  手機時標題與按鈕各佔一列。
- 導覽列在 390px 塞了七八個項目,**印單統計 chip 被壓成 20px**(188px 的內容),數字完全看不見。
  修法:chip 不參與收縮 + 手機只留數字;縮放與「手機遙控 QR」在手機隱藏(前者瀏覽器自己有,
  後者人已經在手機上了,不需要掃 QR 連自己)。
- `VCardSubtitle` 預設單行截斷,手機上把整段說明截成半句 —— 設定頁的說明正是用它寫的。全域改為可換行。
- 統計頁日期區間兩個固定 170px 的欄位共需 368px,在卡片內(可用 313px)會溢出。手機改成各佔一半。
- **記錄類表格改成卡片式**(`.table-cards`):請求記錄 12 欄、整列 1312px,手機上只看得到前兩欄。
  窄螢幕改成「一筆一張卡、欄名寫在每格左邊」。**刻意不另做一套手機模板** —— 那等於同一份資料維護兩份畫面。

> ⚠️ **表格頁加新欄位時,`<td>` 要一併補 `data-label`**,否則手機上那一格會沒有名稱。
> 欄名取自該屬性,不是自動從表頭推的。

### 這次的驗證方式（重要:之前以為做不到）

記憶裡記著「Chrome 擴充打不開 localhost」,所以原本判定網頁版畫面無法驗證。
**實際上 playwright 驅動系統 Chrome 就能開 localhost**,不受擴充限制:

```
npx playwright@1.49.0 screenshot --channel=chrome --viewport-size=390,844 "http://localhost:11420/#/xxx" out.png
```

`--channel=chrome` 是關鍵(用系統已裝的 Chrome,不必另外下載 playwright 的瀏覽器)。
本次靠它做到:21 頁逐頁截圖、量測橫向溢出與文字截斷、比對表格欄名與 `data-label` 是否對位。

一個量測上的陷阱:**手機模式下 `scrollWidth` 會等於 `innerWidth`**(Chrome 會把 layout viewport 撐大),
拿兩者相比永遠看不出溢出。要跟**設定的 viewport 寬度**比才準。

---

## 未提交（2026-09-10）：網頁版大改版 —— 桌面功能全上瀏覽器 + 對外存取控制

**起因**：網頁能力原本只有兩份手刻 HTML（`/control` 手機遙控、`/board` 看板），功能是桌面版的零頭，
而且只能在內網用；主管人在外面看不到查詢記錄、統計與現場狀況。同時本地 server **完全沒有認證**，
`/images`、`/captures`（面單含收件人姓名電話地址）對整個區網裸奔。

**做法的核心是不做第二套系統**：桌面與網頁共用同一份 Vue 前端、同一批 Rust 業務邏輯，差別只在資料怎麼傳。

### 四條橋接（新增四個 server 子模組）

| 桌面走法 | 網頁走法 | 檔案 |
|---|---|---|
| `invoke('cmd', args)` | `POST /rpc/{cmd}` | `server/rpc.rs` |
| `listen('event', cb)` | `GET /events/stream`（SSE） | `server/events.rs` + `event_bridge.rs` |
| Tauri 載入 dist | 嵌入 binary 的 dist | `server/assets.rs` + `build.rs` |
| （無） | 內網免登入 / 外網要密碼 | `server/auth.rs` |

### 幾個關鍵決定與理由

- **78 支 command 一支都沒改。** `AppHandle::state()` 可以就地生出 `State<'_, SharedState>` 餵給既有的
  `#[tauri::command]` 函式，所以 `rpc.rs` 只是一張分派表。新增 command 時**必須同時補這張表**，
  否則桌面能用、網頁 404 —— 已用測試 `registry_sync` 比對 `generate_handler!` 與分派表，漏補會直接測試失敗。
- **`server::start` 被迫改成回傳 boxed future。** router 裡的 `/rpc` 能呼叫 `server_restart`，
  而它又回頭呼叫 `server::start`，`async fn` 的 `Send` 推導無法收斂這個自我遞迴
  （報 `cannot satisfy impl Future: Send`）。box 成 `dyn Future + Send` 由 trait bound 直接斷言，推導就此打住。
- **所有 `app.emit()` 都改走 `event_bridge::emit()`**（28 處）。只橋接「目前用得到的那幾個事件」的話，
  日後新增的事件會只有桌面收得到，而且要等使用者回報「網頁上這個沒更新」才會發現。
- **內外網判斷只認 TCP 對端位址，不看 `X-Forwarded-For`。** 這台機器直接對外、前面沒有代理，
  該標頭任何人都能自己填，信了整道門就形同虛設。日後若真的擺代理在前面，要先確認代理會覆寫該標頭再改。
- **兩處寫死的 `127.0.0.1` 已改為依環境決定**（`api/media.js`）：面單圖與相機串流。
  網頁版沿用舊寫法的話，主管在自己電腦開會去連自己那台的 18080，圖全部載不出來。
- **`CorsLayer::permissive()` 已移除。** 它等於允許任何網站的 JS 對這台機器發請求。

### 存取控制的行為

```
內網網段        → 免登入，完整權限（現場電腦、工控機、手機）
非內網          → 要輸入共用密碼；通過後與坐在現場同等權限（依使用者決定，不做角色分權）
非內網打工控機 API → 一律 403，即使已登入
未設密碼        → 外網一律拒絕（不是「沒設密碼就放行」）
```

設定入口在「伺服器設定 → 網頁存取」：對外開關（**預設關**）、共用密碼、可用時數、失敗鎖定、內網網段清單。

### 舊頁的去留

`control_page.html`、`board_page.html` 與對應的兩個測試檔已搬到 `backups/20260910104322/`。
`/board`、`/control` 改為 303 導向 `/#/sort-board`、`/#/sort-channels` —— 現場貼的 QR 與書籤不會失效。

---

## ⚠️ 網頁版改版：已驗到什麼、還沒驗到什麼

**已實測（以真實服務 + 真實資料）**

- RPC：無參數 / 帶 req / camelCase 與 snake_case 參數對應 / 未知 command 回 404 中文訊息。
- SSE：觸發真實事件收得到，背景事件（`network-status`）也自動上匯流排；`/board/stream` 的舊格式與過濾仍正確。
- 存取控制：以「把 127.0.0.1 移出內網網段」模擬外網，逐項驗過
  工控機 API 403、資料端點 401、面單圖與存證照 401、靜態殼 200（否則登不了）、
  偽造 `X-Forwarded-For` 無效、登入成功後放行、偽造 token 401、
  連錯 5 次鎖 15 分鐘且正確密碼也擋、既有 session 不受鎖定影響、安全事件寫進 `event_log`。
- 測試造的資料（密碼、session、鎖定記錄、security 事件）**已全部清除**，設定已還原。
- 回歸：`cargo check --all-targets` 0 問題、`cargo test` 67 綠、`yarn build` 通過、
  `node --test tests/web-runtime.test.mjs` 19 綠、兩語系 1003 鍵完全對齊。

**還沒驗到（接手的人請補）**

1. **真正從外網連進來**。上面是用改網段模擬的，等同於驗過判斷邏輯，但沒有走過
   「路由器 Port Forward + DDNS」這條實際路徑。要驗：手機關 Wi-Fi 用行動網路連 DDNS 網址。
2. **網頁版的實際畫面**。這台機器的 Chrome 擴充打不開 localhost（既有限制），
   所以 21 個頁面在瀏覽器裡長什麼樣、版面有沒有破、互動順不順，**一次都沒看過**。
   後端與傳輸層已用 curl 與單元測試驗到，但畫面層要人去點。
3. **桌面 App 的回歸**。這次動到 19 個檔案的環境判斷、11 個檔案的 `listen` 匯入來源、28 處事件送出點。
   編譯與 build 都過，但桌面版請實際點一輪（尤其：儀表板統計即時更新、袋件核對、分揀看板、
   多螢幕開窗、快取設定的選資料夾、伺服器設定的開機自動啟動）。
4. **手機上的實際操作**。整套 Vue 版比原本的手刻遙控頁重（JS 約 1.7 MB），
   舊手機或現場 Wi-Fi 不好時的載入速度沒有量過。若太慢，該做的是把路由改成動態載入分割 chunk，
   不是退回去維護第二套手刻頁面。

---

## 🚨 網頁版尚未加密（HTTPS）—— 對外開放前務必知道

依使用者決定，這一期**不做 TLS**。Port Forward 上線後，以下資料在網際網路上**明文傳輸**：
共用密碼、session cookie（攔到就能直接冒用，不需要密碼）、面單圖（收件人姓名電話地址）、查詢記錄與統計。

路徑上任何一段（客戶路由器、ISP、公共 Wi-Fi）都可讀取或竄改。這在公共 Wi-Fi 環境是常見手法，不是理論風險。

**已預留的接點**：TLS 只影響 `server/mod.rs` 建立 listener 那一段，之後換成 `axum-server` + rustls 即可，
不需要重構認證或路由。啟用後記得把 `auth.rs` 登入回應的 cookie 補上 `Secure`（原始碼該處有註記）。

**目前的補償措施**：對外開關預設關閉、密碼至少 8 字、失敗鎖定、session 預設 8 小時、安全事件留痕。

---

## 2026-09-09：欄位標題與輸入提示重疊修正（跨專案同步，已提交 f31fd0f）

**問題**：`src/styles/@core/base/libs/vuetify/_overrides.scss` 的提示文字區塊無條件寫了 `opacity: 1 !important`，且該區塊不在 `@layer` 內
（未分層樣式優先於所有分層樣式），把 Vuetify「標題還沒浮到框線上時先把提示文字藏起來」的機制整條蓋掉。
凡是「標題寫在輸入框內、又給了提示文字」的欄位，兩段字就會疊在同一個位置。
**這行是 Vuexy 樣板原生就有的**，同一份樣板衍生的 15 個專案全部都有，不是哪一次改動引進的。

**處理**：在同一區塊補回 Vuetify 的條件——`.v-field:not(.v-field--no-label, .v-field--active)` 時提示文字 `opacity: 0`。
提示文字顏色維持原樣，標題在框外的欄位（`AppTextField` 系列）完全不受影響。

**本專案目前沒有受影響的欄位**（`SortChannelsPage.vue` 那個欄位只有 `label`，沒有 `placeholder`）。這次是預防性同步，畫面不會有任何變化。

**驗證**：已實測補丁編譯進頁面（`localhost:11420` 首頁樣式中查得到該條規則，Vuetify 4.1.12 的隱藏規則與 4.2.0 逐字相同）。

**同批處理**：15 個專案（含樣板 `VuexyLaravelVue`）的同一個檔案都補上相同的 10 行，補丁逐字相同。
問題最早在 `FeiqingAdmin` 的廠商貨運綁定頁被發現，該專案已完整實機驗證（空值／有值／focus／框外標題四種情境）。

---

## ⚠️ 立刻要處理的事（未完成，會影響使用者）

### 1. 工控機廠商需配合修改（外部依賴，中介端無法自行解決）

直印模式下工控機**仍必須** `POST /api/report`。目前它多半綁在「有沒有拿到 `label_path`」上，該欄位在直印模式不存在 → 整段跳過。

- 契約已寫進 `docs/local-http-api.md` 第 2、3 節（含完整時序表），可直接給廠商。
- **`.docx` 已在 2026-08-10 下午用 pandoc 重產**（舊檔備份於 `backups/20260810135454/`）。先前它停在 v0.12.0、**不含直印回報契約** —— 若照舊檔寄出，廠商會照錯的規格實作。要寄檔案版就寄現在這份。
- 廠商未修好之前，桌面 App「佇列歷史」頁的**回報來源欄會整批顯示黃色「僅中介機自印」** —— 這是預期現象，不是故障。修好後會轉綠色「工控機已回報」。

---

## 2026-09-09：看板多螢幕管理 + 監視器可讀性（v0.23.0）

### 多螢幕管理（`useDisplayWindow` / `DisplayLauncher`）
- 選單標出「目前開在哪一台」、每台螢幕被哪個看板佔用（跨看板可見），並提供「關閉看板」。
- **換螢幕改用「關掉重開」而非搬移**：`setPosition` 對 macOS 的原生全螢幕視窗無效（實測過：退出全螢幕、等動畫、再設位置都沒用，視窗仍留在原本那台）。關掉重開對使用者是同一個動作，結果可靠。
- **判斷視窗在哪台螢幕用實際座標比對**，不記前端狀態——使用者手動關窗或拖動後，記的狀態就會失準。
- 兩個看板搶同一台螢幕時自動對調。要能把對方原樣重開，得知道它的路由與開啟參數，因此把這些記在 `localStorage`（主視窗重新整理後仍認得已開的看板）。

### 看板以監視器為準的調整
- 位置標示移進燈號圓圈（L1–L5 / R1–R5），省下的側欄寬度全給單號，單號放大約四成。
- 配色全面加深（監視器反光時淡色看不見）、單號前後段統一同色。
- 綠燈持續到下一件進來，不再自動熄滅。

### 這次踩到的兩個坑（都與「批次改文字」有關）
- **刪 JSON 鍵用了靠出現順序的 regex**（`count=1`），結果刪掉的是還在使用的 `page.sort.pos`，分揀通道頁整排顯示成 key 名稱。修法：刪鍵要用該區塊獨有的內容定位，不能靠順序。
- **越南語文案裡寫了未跳脫的雙引號**，直接把 locale JSON 弄壞、`vite build` 當場失敗。文案裡不要用雙引號。

## 2026-09-08（深夜）：看板顯示分揀時間（v0.22.2）

- `BoardEvent` 加 `at` 欄位，值由後端在推播當下取 `chrono::Local`（格式 `%Y-%m-%d %H:%M:%S`）。**刻意不讓前端自己抓時間**：畫面延遲多久時間就差多久，且與「請求記錄」對不起來。日期一定要給——夜班跨午夜只有時分秒會分不出是哪一天的件。
- 顯示在看板最上緣正中央，藍色 `#1976D2`。**這個藍是刻意寫死的**：專案主題色盤沒有藍（`primary` 是乖乖綠、`info` 偏青綠），這裡需要一個跟紅色單號、黑色訊息都分得開的第三色。
- **用絕對定位釘在頂端**：時間只要留在版面流裡，就會推擠單號與燈號的位置（調位置時來回試了幾輪才確定這點）。絕對定位後它不佔高度，其他元素完全不受影響。
- 物流名字級一併放大，與單號的比例重新調過。

## 2026-09-08（晚，續）：看板遠距可讀性調整 + 網頁版網址 QR（v0.22.1）

### 接上監視器後調的幾件事（實機掛上去才看得出來）
- **異常原因原本是淡灰小字，幾公尺外看不到**：放大到與單號同級（字級隨字數自適應）、改用主題正文色 `on-background` 加粗。
- **訊息在逗號處斷行**：「無法列印，訂單當前狀態異常」拆成兩行比擠成一長條好讀，行短了字還能更大。⚠️ 第一次寫的分隔字元是**兩個半形逗號**，而雲端訊息用的是全形「，」，所以完全沒生效；已改用 `\uFF0C` 明寫並實測兩種訊息都正確斷行。**在程式碼裡寫全形標點要用碼位，不要直接打字元。**
- **側欄改成固定寬 15vw**：原本是 `auto`（依內容撐寬），中欄寬度會隨字體與位置名長度浮動，字級公式只能用估的，估錯就會蓋到燈號。改固定寬後中欄寬度是確定值，字級以它回推，再加 1.5vw 安全邊距；順豐 15 碼實測兩側仍有間距。
- **版面整體上移**（上下 `0.55fr : 1.45fr`）：異常訊息字大又長短不一，單號放正中央的話訊息一長就把版面往下擠出去。

「開啟看板」選單底部多一段：區網網址（依網卡列出）＋ QR，掃了就能用電視或另一台機器開網頁版看板，不必手抄網址。

- 做在共用元件 `DisplayLauncher` 上，用新的 `web-path` prop 控制——只有傳了路徑的頁面（目前只有分揀通道頁傳 `/board`）才顯示這段；件數核對、印單統計那兩處沒有網頁版，不受影響。
- 網址與 QR 由同一個字串模板產生（`http://{ip}:{port}{webPath}`），QR 只給第一個位址，列表仍列出全部供手動輸入或複製。
- **QR 圖像沒有實際掃描驗證**（這台機器沒有 QR 解碼工具）；已驗證的是網址本身可用（`http://192.168.36.52:18080/board` 回 200）與兩者同源。要完全確認請用手機掃一次。

## 2026-09-08（晚）：分揀看板（v0.22.0）

### 做了什麼
- **後端**：`server/mod.rs` 新增 `BoardEvent` 與 `publish_board()`，在 `get_parcel` 的正常與錯誤兩個出口各推一則（位置、單號、物流名、狀態、異常訊息）。桌面走 Tauri 事件 `sort-board`，網頁走 SSE `GET /board/stream`（broadcast channel，容量 16、落後只補最新）；`GET /board` 回內嵌的 `board_page.html`。
- **兩種開法**：桌面「分揀通道」頁右上角「開啟看板」→ 沿用既有的 `DisplayLauncher` / `useDisplayWindow`（跟件數核對同一套），可挑螢幕全螢幕開，多螢幕各開一個；網頁版給電視或另一台機器用網址開。**側欄刻意不放入口**——看板是給另一個螢幕看的。
- **版面**：左右各五盞燈（綠=剛分到、6 秒後淡回；黃=暫停中；灰=還沒輪到），中央單號特大字、後四碼再放大，下方物流名與異常訊息。異常紅字、未指派通道黃字。
- **字級隨單號長度自適應**（CSS `--len`）：固定字級遇到 15 碼的順豐單號會撐破中欄、蓋到燈號。係數以最長單號回推，兩邊看板用同一組。
- **配色取自 App 主題**（`vuetify-materio/theme.js`）：底 `background #F8F7FA`、字 `on-background #2F2B3D`、綠 `success`、黃 `warning`、紅 `error`。網頁版沒有 Vuetify 只能寫死同一組值，**主題改色時兩邊要一起改**。
- **順手做的 UI 調整**：「分揀通道」頁的純分揀模式與異常件提示面單兩張卡片併成一列、說明精簡成一句話（細節走教學文件），少佔掉半個畫面。

### 驗證結果
- `tests/board-page.test.mjs`（jsdom）15 項全綠：三種狀態字色、後四碼單獨放大、暫停黃燈、綠燈淡出、沒有通道時不亮燈。`cargo test` 48 項、`control-page` 35 項、`vite build` 皆綠。
- **實機**：桌面獨立視窗與全螢幕（1920×1080）、網頁版都實際跑過並截圖，燈號與單號同步顯示、綠燈會淡出、暫停黃燈正確。長度驗證用最長的順豐 15 碼單號，全螢幕與 1560 寬視窗都不會頂到燈號。
- 測試環境（`local` 模式 + 錯誤面單開關 + 暫停 L2）已全數還原，測試記錄與提示圖清乾淨。

### 已知限制
- 看板事件只在工控機 `GET /api/parcel` 時推送；桌面「掃描列印」「自動印單」不會上看板（那兩條路徑本來就沒有分揀通道）。
- 網頁版的顏色是寫死的，與 App 主題同一組值但沒有連動。

## 2026-09-08（下午）：分揀輪替改為「格口公平輪替」（尚未發版）

### 為什麼要改
舊版的輪替指標是**每家物流各自一份**。設定「左1=7-11、左2=7-11+全家、左3=7-11」、包裹順序 7-11 → 全家 → 7-11 時，第三件會再進左2（全家那件不會推進 7-11 的指標），左3 反而閒著——共用格口的人比別人累。

### 改成什麼
候選中挑**最久沒有收件**的格口（平手照 L1→R5）。排序依據是「這個格口實際收了什麼」，**完全不看物流別**，所以共用格口被別家的件用掉之後，下一件就會讓給比較閒的格口。同一情境現在的結果是 左1 → 左2 → **左3**。

- 狀態改成 `SortRouting { last: HashMap<通道代碼, ChannelLast{seq, no}>, seq }`，型別更名 `SortRoutingState`（原 `RoundRobinState` 已不符語意）。
- 冷啟動回查 `print_event` 時，順序值取該格口最近一筆的 `id`；本進程分配則取 `RUNTIME_SEQ_BASE + 遞增序號`，保證「這次跑起來之後分過的」永遠排在啟動前的歷史之後。
- 尾碼迴避、「跳過本輪」額度、只跑一輪的退讓規則都疊在這個新順序上，語意不變。

### 驗證結果
- 單元測試 76 項全綠（新增 `shared_channel_yields_to_the_idlest_one`、`tail_rule_looks_at_the_channel_not_the_provider`；原 `each_provider_keeps_its_own_rotation` 是舊行為，已改寫）。
- **實機端到端**：面單路徑暫切 `local`、開錯誤面單開關。造情境讓 7-11 四個格口的最近收件序號成為 L1=23（剛收過）、R3=17、R1=16、L3=15（最久沒收），連查四件：

  | 件 | 實際分到 | 舊行為會給 |
  |---|---|---|
  | 1 | **L3** | L1 |
  | 2 | **R1** | L3 |
  | 3 | **R3** | R1 |
  | 4 | **L1** | R3 |

  四件全中新行為，且與舊行為完全錯開，同時證明冷啟動是用 `print_event.id` 排序。
- **跨物流讓位沒有實機驗到**：需要兩家不同物流的可查單號，這台機器只有 7-11 的歷史單查得到（黑貓／新竹的歷史單號是舊測試假單，雲端回 NOT_FOUND、帶不出物流商）。排序程式碼完全不看 provider，跨物流走的是同一段路徑，由上述兩個單元測試覆蓋。
- 環境已完全還原：`config.toml` 從備份還原、造的假 `print_event` 與測試期間記錄／每日統計清除、5 張錯誤提示圖搬進 `backups/`。

### 尚未處理
- 尚未 commit、未寫 CHANGELOG、未發版。

## 2026-09-08：分揀分配加入「後兩碼迴避」（尚未發版）

### 為什麼要做
現場作業員貼單是靠配送單號**後兩碼**認包裹。同一個格口若連著兩張後兩碼相同的單，就分不出手上這張對應哪一件，容易貼錯。

### 做了什麼（全部在 `server/mod.rs`）
- `resolve_channel_code` 多收一個「本件配送單號」參數。輪到的格口若**上一件後兩碼與本件相同**，就讓給下一個候選。
- **前提**：該物流商有 2 個以上可用通道（啟用且有代碼）才套用；只有一格時無處可換，照給。
- **只跑一輪**：繞完一圈每個候選都撞尾碼，就退讓回 round-robin 原本該給的那個 —— 寧可尾碼撞，不可沒格口可去。
- **不吃掉「跳過本輪」額度**：因撞尾碼而讓過的通道不扣 `skip_count`，那是操作員在手機上按的，只該花在真的輪到它的那次。
- 狀態改成 `SortRouting`（輪替指標 + 各通道最近單號共用同一把鎖）；進程剛啟動記憶體是空的，會回查 `print_event` 補值，否則每次重開 App 前幾件等於沒有這條規則。
- 錯誤面單路徑也帶入雲端回的單號（拿不到單號時自動退化成原本的純輪替）。

### 驗證結果
- 新增 6 個測試（`server::tests`，in-memory SQLite 建等價表直接跑分配）：撞尾碼改道、只有一格照給、只跑一輪不落空、冷啟動回查列印記錄、不消耗跳過額度、無單號退化為純輪替。`cargo test` 全綠（74 項、1 ignored）。
- **實機端到端（工控機 API + 真實雲端回應 + 真實通道設定）**：面單路徑暫切 `local`（只回路徑不送印）。7-ELEVEN 有 4 個通道（L1/L3/R1/R3）：
  | 件 | 單號尾碼 | 輪替該給 | 該格上一件尾碼 | 實際分到 | 驗到什麼 |
  |---|---|---|---|---|---|
  | 1 | 85 | L1 | L1 = 85（**來自 DB 歷史，記憶體剛啟動是空的**）| **L3** | 撞尾碼改道 ＋ 冷啟動回查列印記錄 |
  | 2 | 17 | R1 | R1 = 17 | **R3** | 撞尾碼改道 |
  | 3 | 32 | L1 | L1 = 85（不撞）| **L1** | 不撞就照常輪替，沒有亂跳 |
  | 4 | 74 | L1 | 四格全是 74（造的情境）| **L1** | 只跑一輪、退讓回輪替，不會落空 |
- 正常列印路徑在這台機器驗不到（雲端測試站上所有可用訂單都是 `STATUS_ABNORMAL`），改走「異常件提示面單」開關開啟後的錯誤路徑——**分配走的是同一個 `resolve_channel_code`、同一份參數來源**，差別只在呼叫點。
- 測試後環境已完全還原：`config.toml` 從備份還原（`direct_print`、錯誤面單開關關閉）、造的假 `print_event` 與測試期間的查詢／異常記錄與每日統計全部清除、產生的 4 張錯誤提示圖搬進 `backups/`。
- 「只有一格照給」與「不消耗跳過額度」兩條只有單元測試（前者現場沒有單通道物流的可查單號，後者要用手機按跳過再送件），程式路徑與已實測的分支相同。

### 尚未處理
- 尚未 commit、未寫 CHANGELOG、未發版。
- 這條規則目前**沒有開關**（一律生效）。若現場希望能關掉，要另外加設定。

## 2026-09-07：分揀通道由左右各 4 擴充為各 5（尚未發版）

### 做了什麼
- `migrations/0029_sort_channel_l5_r5.sql`：補建 `L5`、`R5` 兩列（`INSERT OR IGNORE`，`updated_at` 明寫 `localtime`，因建表預設值是 UTC）。舊檔一律沒動。
- `commands/sort_channel_commands.rs`：`POSITIONS` 由 8 筆改 10 筆。這個常數同時是桌面 command 與手機遙控四支 API（暫停 / 跳過 / 最近件 / 指派）的位置白名單，改一處全通。清單排序 SQL 本來就是 `substr` + `CAST`，不必動。
- 前端 `SortChannelsPage.vue`：`LEFT_POSITIONS` / `RIGHT_POSITIONS` / `POSITION_LABELS` 各補 L5、R5；i18n 雙語補 `page.sort.pos.L5` / `R5`，並改掉頁面副標「左 4 通道 / 右 4 通道」→「左 5 / 右 5」（vi 同步）。
- `api/tauri.js` 瀏覽器預覽用的 mock `POSITIONS` 同步補到 10 筆。
- 文件：`CLAUDE.md`、`README.md`、`docs/local-http-api.md`（排序範例 L1…L5 < R1…R5）已改，`local-http-api.docx` 用 pandoc 重產。

### 驗證結果
- migration 實跑：`_sqlx_migrations` 第 29 筆 success=1，`sort_channels` 10 列，L5 / R5 的 `updated_at` 是本地時間。
- 執行中的中介服務打 `GET /api/channels` 回 10 筆、順序 L1…L5、R1…R5。
- L5 / R5 走過手機遙控四支 API：暫停 / 恢復 200、跳過累加回 `skip_count:1`、最近件 200、指派 200；無效位置 `L6` 正確回 400。**驗證用的暫停、skip、貼標人員已全數還原，`sticker_history` 的測試名字也已刪除。**
- 桌面「分揀通道」頁實機截圖：左 5 / 右 5 共 10 張卡，新卡顯示「未設定」與代碼範例佔位字，副標已變「左 5 通道 / 右 5 通道」，未動到既有通道（儲存變更數維持 0）。
- 桌面存檔路徑實跑:在「左 5」填代碼 → 儲存 → DB 寫入、卡片狀態由「未設定」轉「已啟用」→ 再清空存檔 → DB 回 NULL。**測試代碼已還原,10 列現況與改動前一致。**
- `cargo test` 全綠（67 項、1 ignored，分散在 6 個 test target）；`yarn build` 綠；`node tests/control-page.test.mjs` 35 通過 0 失敗；兩份 i18n JSON 的 key 完全對齊（雙向皆無缺）。
- 未改動的等價推論（不另外實測）:round-robin 分配（`resolve_channel_code`）、印單統計的通道彙總都是純 SQL 驅動、沒有寫死通道數,新增位置走的是同一條路徑。
- 手機遙控頁 `/control`：**不需改任何程式碼**，它整頁由 `/api/channels` 驅動、位置文字是 `posLabel()` 依 L/R + 數字組出來的。以 jsdom 載入該頁、fetch 轉打執行中的服務實測，清單 10 格全出現，`左5` 的詳情頁與「設定」（貼標 / 指派物流）頁都進得去、選項正常。

### 尚未處理
- 發版已完成：commit `84602e8`、tag `v0.21.0`、CI 六個 job 全綠、九項產物齊全、release 已公開、`latest.json` 版本 0.21.0 且 notes 正確。
- 分揀機端（工控機／PLC）要能實際路由到新的兩個格口，得由現場設定新格口的通道代碼並確認機器側對應，桌面填代碼只是中介端的對照表。

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
