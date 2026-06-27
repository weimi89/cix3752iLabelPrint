# 件數核對跨機即時同步 — 設計方案（待審）

> 狀態:**P0 已驗證通過**(2026-06-26)。方案確認:WebSocket + 雲端廣播、**每袋頻道 `bag.{package_sn}`、public 頻道**,兩 repo(`cix3752iWeb` 雲端 + `cix3752iLabelPrint` 中介端)一起改,經 `ReverbHub` 轉發。

## P0 驗證結果(已通過)

最小 Rust Pusher subscriber(`tokio-tungstenite` over `ws://`)實測:連上 ReverbHub → 收 `connection_established` + socket_id → 訂閱 public `bag.P0TEST` → `subscription_succeeded` → 收到 node publisher(Pusher HTTP API + HMAC 簽章)發的 `parcel.printed` 事件,payload 完整。協定流程確認:`connect → connection_established → subscribe → subscription_succeeded → 事件`,另需處理 `pusher:ping → pusher:pong`。

ReverbHub 已加 `SORTING` app(`.env`:`REVERB_APP_SORTING_ID/KEY/SECRET`,自動掃描載入)。

**TLS(wss)亦已驗證**:`tokio-tungstenite` 加 `rustls-tls-webpki-roots` feature,實測經 `wss://<測試環境 Reverb host>:443` 連上、訂閱成功(只訂閱、未發佈,不污染)。**踩到並解掉一個坑**:rustls 0.23 需在啟動時 `CryptoProvider::install_default()`(ring 或 aws-lc-rs),否則 wss 握手 panic「Could not automatically determine the process-level CryptoProvider」。

### 環境與 app(已更正)
- **app 沿用既有 `雲端廣播 app`(ID `<app id>`)**,**不另開 SORTING app**。雲端 `cix3752iWeb` 本來就用此 app 廣播(`.env` `PUSHER_APP_ID=<app id>`),發佈管線現成。
- **Reverb 機器分環境,發佈端與訂閱端必須同環境配對**:
  - 正式:`<正式環境 Reverb host>`(Cloudflare 代理,`172.67.161.106`,wss/443)
  - 測試:`<測試環境 Reverb host>`(直連 origin `122.117.181.156`,wss/443)
- **正式 `<正式環境 Reverb host>` 走 Cloudflare** —— CF 對 WebSocket 的代理需在 P3 rollout 時以只訂閱方式實連驗證一次(本次只驗了測試機直連)。

### P2 必做的 rustls provider 處理
中介端已用 reqwest(rustls)+ sqlx(rustls)。加 WS client 後須在啟動時安裝一次 `CryptoProvider`。注意 **provider 對齊**:reqwest 0.12 預設可能用 `aws-lc-rs`,WS 端也用同一個(擇一 `ring` 或 `aws-lc-rs`,全 crate 一致),避免「installed provider 不符」執行期錯誤。

## 1. 問題與現況

`件數核對`(`src-tauri/src/bag_check/mod.rs`)是**純記憶體、單機**狀態(`Arc<Mutex<VecDeque<BagEntry>>>`),由本機工控機 `GET /api/parcel` 驅動。當同一袋的包裹被拿到**別台**處理,本機看不到對方的列印進度,畫面「未印」數字失真。

### 關鍵事實:雲端已是「列印狀態」真相來源
- 任何機台 `GET /api/parcel` 成功 → 雲端 update `order_info.shipping_print_time`(`cix3752iWeb/app/Http/Controllers/Api/V1/OrderPrintController.php:312-313`、`app/Services/OrderPrintService.php:408` `recordPrintLog()`)。
- 中介端 `examine_package` 回傳的每件都帶雲端當下 `last_print_time`(`bag_check/mod.rs` build_entry 註解:「其餘沿用雲端 last_print_time」)。
- ∴ **新袋**載入時就已是跨機正確;**缺口只在「已載入的舊袋」**——`update_existing` 對舊袋「不再請求雲端、只就地更新」,故別台後續的列印不會反映。

## 2. 架構:雲端廣播 → Reverb → 各中介端訂閱

```
工控機 GET /api/parcel ─▶ 雲端 update shipping_print_time（既有）
                              │
                              └─▶ broadcast ParcelPrinted{package_sn, shipping_no, print_time, print_num}
                                   到 Reverb 頻道
                                        │ (ReverbHub 轉發)
              ┌─────────────────────────┼─────────────────────────┐
            A 機台                     B 機台                     C 機台   ← Reverb/Pusher client
       記憶體有此袋才套用 → 更新該件 last_print_time → emit bag-check-updated → 前端即時刷新
```

設計原則:
- 中介端**只在記憶體已持有該袋時**套用遠端事件,**不從遠端事件無中生有建袋**(否則每台都會冒出全部袋)。
- 雲端維持唯一真相;中介端是被動 view。對齊既有「不讓工控機等雲端」——廣播在雲端非同步派發,工控機回應不受影響。

## 3. 頻道設計(需你拍板)

| 方案 | 頻道命名 | 優點 | 缺點 |
|------|---------|------|------|
| **A. 每袋一頻道(推薦)** | `bag.{package_sn}` | 精準、自包含,中介端不需要「site/倉別」概念;A 只收自己持有袋的事件 | 動態訂閱/退訂(袋進清單訂、prune 退);同時數十頻道(Reverb 可承受) |
| B. 每倉別一頻道 | `warehouse.{storage_warehouse}` | 單一頻道、訂閱簡單 | 需中介端設定「本機屬哪個倉別」,且物理分揀場 ≠ 倉別時會錯分組 |
| C. 每場一頻道 | `site.{siteId}` | 同上 | 需「site 身分」概念,目前兩端都沒有,要新發明 |

**推薦 A**:中介端本來就知道自己記憶體裡有哪些袋(VecDeque),袋進清單時 subscribe `bag.{sn}`、被 prune 時 unsubscribe。雲端每次列印就 broadcast 到 `bag.{package_sn}`。任何「碰過這袋」的機台都訂著同一頻道,天然解決「包裹拿到別台」。雲端側不需要知道 site,只需知道 package_sn(本來就有)。

> 倉別欄位(`ClearancePackage.storage_code` / `WarehousePackage.storage_warehouse`)保留作為方案 B/C 的後備,或作為頻道 namespace 前綴(`bag.{storage}.{package_sn}`)增加隔離。

## 4. 雲端 `cix3752iWeb` 改動 — ✅ P1 已完成並閉環驗證(2026-06-26)

實作:
- 新增 `app/Events/ParcelPrinted.php`(`implements ShouldBroadcast`):public `Channel("bag.{package_sn}")`、`broadcastAs='parcel.printed'`、`broadcastWith={package_sn,shipping_no,print_time,print_num,shipping_provider}`;建構時把 pipe 串 print_time 取最新段並去 `#fake` 後綴;static `dispatchFor()` 守衛散單(空 package_sn/shipping_no 不廣播)且 try/catch 不影響列印主流程。
- **兩條列印路徑都 dispatch**(發現 controller 不走 service):
  - 設備端 `OrderPrintController::show()`(api.v2 `order-proxy/forward-print`,user_type='E')—— `OrderInfo::...->update($uPOData)` 後
  - GUI/雲端端 `OrderPrintService::recordPrintLog()`(user_type='U')—— 同上
- 閉環驗證:雲端 `event(new ParcelPrinted(...))`(sync queue + env 指向本地 Reverb)→ P0 subscriber 在 `bag.P1TEST` 收到正確 payload,print_time 正規化正確。

部署注意:`ShouldBroadcast` 進 queue,正式環境需 **Horizon / queue worker 在跑**才會送出;雲端 `PUSHER_HOST` 須指向**與該分揀場中介端同環境**的 Reverb(正式 <正式環境 Reverb host> / 測試 <測試環境 Reverb host>),app=雲端廣播 app(<app id>)。

### (原設計)4. 雲端改動規劃

現況:`BROADCAST_CONNECTION=pusher`(`.env` 指向 `<測試環境 Reverb host>`,app <app id>),`composer.json` 已有 `pusher/pusher-php-server`,已有 `ShouldBroadcast` 事件範式(`app/Events/TaskQueueUpdated.php`、`FileHandlerMessage.php`)。**基礎齊備,只缺這個事件。**

1. 新增 Event `app/Events/ParcelPrinted.php`(`implements ShouldBroadcast`):
   - payload:`package_sn`、`shipping_no`、`print_time`、`print_num`
   - `broadcastOn()` → `Channel("bag.{$packageSn}")`(方案 A;若 private 改 `PrivateChannel`)
   - `broadcastAs()` → `"parcel.printed"`
2. 在列印時間更新點 dispatch:`OrderPrintService::recordPrintLog()`(`:408` 一帶)更新完 `shipping_print_time` 後 `ParcelPrinted::dispatch(...)`。**集中在 service 層一處**,涵蓋 `OrderPrintController:312-313` 與其他路徑。
3. 確認該訂單能取得 `package_sn`(`order_info.package_sn`,`OrderInfo.php:34`)。散單(無 package_sn)不廣播。
4. 廣播走 queue(`ShouldBroadcast` 預設進 queue);雲端若 `QUEUE_CONNECTION=sync` 則同步送,需確認不拖慢 `GET /api/parcel`——建議 `ShouldBroadcastNow` 評估或確保 queue worker 在跑。

## 5. 中介端 `cix3752iLabelPrint` 改動 — ✅ P2 已完成(2026-06-26,cargo check/test 綠,未 commit)

實作:
- `Cargo.toml`:加 `tokio-tungstenite`(default-features=false + `connect`,`rustls-tls-webpki-roots`)、`rustls`(default-features=false + `ring`)。**單一 provider**:Cargo.lock 確認只有 ring、無 aws-lc-rs。
- `config/mod.rs`:`SyncConfig`(`[sync]`:`enabled`(預設 false)/`reverb_host`/`reverb_port`(443)/`reverb_scheme`(wss)/`reverb_app_key`)。
- `bag_check/mod.rs`:`apply_remote_print`(僅持有袋才更新、字典序比較取較新、變動才 emit)+ `refresh_bag`(examine_package 補洞,await 在鎖外)+ `held_package_sns`;核心抽 `apply_remote_to` free fn + 3 個回歸測試。
- `sync/mod.rs`(新):Reverb 訂閱端 — 等 connection_established → 每 2s reconcile(訂新持有袋 / 退已無袋,新訂閱觸發 refresh_bag 補洞)→ 收 `parcel.printed` → apply_remote_print;指數退避重連(≤30s);ping/pong;`parse_parcel_printed` 純函式 + 2 測試(鎖巢狀 JSON 字串 wire 格式)。
- `lib.rs`:`mod sync`;啟動裝 `rustls::crypto::ring::default_provider().install_default()`;`config.sync.enabled` 時 spawn。
- 順手修:`camera/mod.rs` 既有測試漏 CameraConfig 新欄位(v0.8.0 起 `cargo test` 編不過,`cargo check` 不編 test 故沒被發現)。
- **前端不用改**:`apply_remote_print`/`refresh_bag` 走既有 `bag-check-updated` 事件,`BagCheckPage.vue` 已在聽,自動刷新。

啟用方式(config.toml):
```toml
[sync]
enabled = true
reverb_host = "<測試環境 Reverb host>"   # 測試;正式 <正式環境 Reverb host>
reverb_port = 443
reverb_scheme = "wss"
reverb_app_key = "<app key>" # 雲端廣播 app
```

設定頁 UI(已完成):`ServerSettingsPage.vue` 加「件數核對跨機同步」卡片(啟用開關 + host/port/scheme/app_key + 提示),i18n zh/vi 補 `sync.settings.*`,mock config 補 sync。**熱套用**:改 `SyncManager`(對齊 CameraManager,存 AppState),`update_config` 呼叫 `state.sync.apply_config(&new_config.sync)` → 切換開關 / 改 host 即時重連或斷線,無須重啟 App。

待驗(部署/實機):啟用後實機跑 + 雲端真實出單 → 看另一台 BagCheckPage 是否即時更新;正式機 Cloudflare WS 代理只訂閱實連一次。

### (原設計)5. 中介端改動規劃

現況:`tokio`(full)、`reqwest 0.12`(rustls)、`futures` 有;**無 WS client**;config **無 site/倉別概念**。

1. **新增依賴**:`tokio-tungstenite`(rustls feature)實作 Pusher WS 協定(reqwest 不支援 WS)。
2. **新 config 區塊** `[sync]`(`config/mod.rs`):
   - `enabled`(預設 false)、`reverb_host`、`reverb_port`(443)、`reverb_scheme`(wss)、`reverb_app_key`(=雲端廣播 app key)
   - host 依環境填:正式機填 `<正式環境 Reverb host>`、測試機填 `<測試環境 Reverb host>`,**須與該場雲端的 PUSHER_HOST 同環境配對**
   - public 頻道免 secret / 免 auth_endpoint
3. **新模組** `src-tauri/src/sync/mod.rs` — Reverb/Pusher client:
   - 連線 `ws(s)://host:port/app/{key}?protocol=7&client=cix3752i&version=...`,收 `pusher:connection_established` 取 `socket_id`
   - 動態 `pusher:subscribe` / `pusher:unsubscribe` `bag.{sn}`(由 bag_check 持有袋集合驅動)
   - 收 `parcel.printed` 事件 → 呼叫 bag_check 新方法 `apply_remote_print(package_sn, shipping_no, print_time)`
   - **自動重連**(指數退避);**重連後對 missing>0 的袋重抓 `examine_package` 補洞**(WS 斷線期間漏掉的靠雲端真相補回)——健壯性關鍵
4. **bag_check 擴充**:
   - `apply_remote_print(...)`:**僅在記憶體已有該袋時**更新對應 shipping_no 的 `last_print_time`、recount、emit。無此袋則忽略。
   - 袋進 VecDeque / prune 時通知 sync 模組訂閱/退訂對應 `bag.{sn}`。
5. **啟動接線**(`lib.rs` bootstrap):`enabled` 時 spawn sync client(對齊相機/排程 worker 模式);未啟用完全不影響現有單機流程。

## 6. ReverbHub 改動
- **不需新增 app** —— 沿用既有 `雲端廣播 app`(ID `<app id>`,KEY `<app key>`)。測試機與正式機 ReverbHub 皆已有此 app。
- 雲端(發)與中介端(訂)共用 `雲端廣播 app` 的 key/secret;中介端只需 KEY(public 頻道訂閱免 secret)。

## 7. 安全(需你拍板)

| | public 頻道 | private 頻道(`private-bag.{sn}`) |
|--|------------|----------------------------------|
| 中介端 | 直接 subscribe | 需先向雲端 auth endpoint 用 socket_id+channel 取簽章 |
| 安全性 | 任何有 app key 者可訂(key 為共用基礎建設) | 需通過雲端授權 |
| 工作量 | 低 | 中(Rust 端多一段 auth HTTP + 雲端開 auth route) |

v1 建議:**public 頻道**(資料僅列印狀態 + 袋號,內網用;app key 不外流)先上;若日後要嚴格隔離再升 private。

## 8. 工作量估算與分期

| 階段 | 範圍 | 估時 |
|------|------|------|
| P0 最小驗證 | ReverbHub 加 app；中介端最小 WS client 連上 + 訂一個固定頻道 + 收到測試事件就 log/emit;雲端用 tinker 手動 broadcast 一發 | 0.5–1 天 |
| P1 雲端廣播 | `ParcelPrinted` event + service 層 dispatch + package_sn 守衛 | 0.5 天 |
| P2 中介端整合 | sync 模組(動態訂閱 + apply_remote_print + 重連 + 補洞)+ config + bag_check 接線 | 2–3 天 |
| P3 驗證 | 跨機實測(A 載入袋、B 印、A 即時更新)、斷線重連補洞、未啟用回歸 | 0.5–1 天 |

## 9. 待你決定
1. 頻道方案:**A 每袋(推薦)** / B 每倉別 / C 每場
2. 安全:**public(推薦 v1)** / private
3. 是否先做 P0 最小驗證(打通 ReverbHub ↔ 中介端 WS)再決定全量
4. 雲端廣播 dispatch 用 `ShouldBroadcast`(進 queue,需 worker)還是 `ShouldBroadcastNow`(同步,確保不拖慢工控機回應)

## 10. 端到端驗收結果(E2E,測試環境,2026-06-26)

針對「無法跑工控機」的限制,以 curl 扮演工控機(`GET /api/parcel/{單號}` = 設備端列印端點 OrderPrintController::show)、必要時以 tinker 扮演「別台列印」,對**真實 App + 測試環境 Reverb** 完整驗收。**全數通過。**

驗到的鏈路:
1. **工控機查件 → 載入袋 + 自動訂閱**:curl 查 bag XYE2619201 的一件 → App `件數核對:新袋載入 package_sn=XYE2619201 total=34` → sync `件數核對同步:訂閱 channel=bag.XYE2619201`。
2. **真實列印 → 雲端廣播 → App 收到**:對該袋 3 個未印件 `GET /api/parcel`(回正整數 response_id + 分配通道 = 雲端真記列印),App sync 收到雲端真實廣播的 `parcel.printed`(測試雲端已部署 P1,查件即觸發廣播)。
3. **別台列印 → 本機套用 → UI 更新**:以 tinker 對該袋未印件發 `ParcelPrinted`(模擬別台),App `件數核對:套用遠端列印` → recount → emit `bag-check-updated` → 「件數核對」頁該件即時由未印→已印。

關鍵正確行為(防重複計數):**本機自己查件印的件,雲端廣播繞回(loopback)時 `apply_remote_print` 判定「已印且時間未更新」→ no-op、不重複套用**。故步驟 2 只見「收到」無「套用」(已被 on_parcel 本機標記);步驟 3(非本機印)才見「套用」。兩者合計即完整跨機同步。

> 測試副作用:tinker 假造的通知會讓中介端記憶體顯示已印、但雲端未記 → **僅測試造成的暫時不一致**;正式運作中 `ParcelPrinted` 只在真記列印時 dispatch,廣播與真相恆對齊,無此問題。

### 驗收中發現並修正的 bug:`refresh_bag` 用錯查詢鍵
斷線補洞 `BagCheckState::refresh_bag` 原以**袋號(package_sn)**呼叫 `cloud.examine_package` → 雲端回 `NO-PACKAGE-DATA`(該端點要的是「訂單號 / 單號」,非袋號)→ 補洞失效。**已修**:改從該袋取一個成員單號(`orders[].shipping_no`)當查詢鍵;修正後重測不再報錯、靜默補洞成功。(live 事件路徑原本就正常,僅「重連後補洞」受影響。)

### 順手加入的觀測 log(保留,供正式環境除錯)
`件數核對:新袋載入`(列出 package_sn + 未印件)、`件數核對同步:訂閱`、`件數核對同步:收到 parcel.printed`、`件數核對:套用遠端列印(跨機同步)`。

### 仍待實機/部署的
- 「第二台中介端視角」(A 印、B 即時更新)同畫面驗證:機制與步驟 3 完全相同,需第二台持有同袋。
- 正式機 `<正式環境 Reverb host>`(Cloudflare)WS 代理:rollout 時只訂閱實連驗一次。
