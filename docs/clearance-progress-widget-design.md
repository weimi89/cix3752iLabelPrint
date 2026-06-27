# 清關進度浮動框(指定日期 → 袋數/件數 + 即時剩餘)— 設計方案(待審)

> 狀態:**C1–C4 已實作完成(2026-06-26,編譯/測試/前端 build 綠,未 commit)**;C5 端到端待雲端部署。決策:依 `clearance_date`、即時走「雲端日期頻道」、啟動入口頂部全域鈕、**日期區間(≤3 天)**、沿用既有廣播 app。
>
> **完成摘要**:
> - C1 雲端聚合 API `GET clearance/progress?from&to`(LocalMiddlewareController::clearanceProgress + OrderPrintService::clearanceProgress,區間 clamp 3 天,回 bag_total/parcel_total/printed/remaining/parcels)。
> - C2 `ParcelPrinted` 補 `clearance_date`(dispatchFor 查 ClearancePackage)+ broadcastOn 額外發 `clearance-date.{YYYYMMDD}`。
> - C3 中介端 `SyncManager` 加 `active_dates` + `progress_set_dates` command + 收 date 頻道 emit `clearance-progress-printed`;`cloud_clearance_progress` command + cloud client `fetch_clearance_progress` + config `clearance_progress_path`。
> - C4 前端:Pinia `stores/clearanceProgress.js`(去重遞減 _allSet/_printedSet)+ 全域 `ClearanceProgressWidget.vue`(Teleport、可拖曳、記位置、聽 clearance-progress-printed)掛 DefaultLayout + AppNavbar 啟動鈕 + i18n zh/vi + mock。瀏覽器 mock 實測四格數字正確帶入(袋2/件14/已印9/剩5)、真實 app 框正常顯示。
> - **C5 待雲端部署**:把 cix3752iWeb 的 5 檔(ParcelPrinted/OrderPrintService/LocalMiddlewareController/routes/api_v1 + 既有 P1)部署到測試雲端後,查詢才有真實數字、列印才會即時遞減。流程同 P1。

## 1. 目標
操作員在任一頁點頂部全域鈕 → 選日期(區間,≤3 天)→ 浮動框顯示該區間「需處理的**袋數 / 件數 / 已印 / 剩餘**」,並隨各機台列印**即時遞減剩餘**。浮動框**跨頁不消失**,只有手動關閉才收起。

## 2. 現況與缺口(探查結果)
- 雲端 `orders-by-date`(`OrderPrintService::lookupOrderSnsByDate(date, source)`)已回 `{order_sns, package_count}`,但**無「已印/剩餘」聚合** → 需新增聚合 API。
- 「當天要處理」依 `ClearancePackage.clearance_date`(報關日),`package_sn` 1:N → `OrderInfo.package_sn`。
- 已印判定:`OrderInfo.shipping_print_time` 有值 或 `ShippingPrintLog.is_fake=0` 存在。
- `ParcelPrinted` 廣播**無日期維度**(只有 print_time 時間戳)→ 日期維度即時遞減需補。
- 全域浮動框可掛 `DefaultLayout`(跨頁持續)+ Pinia store 存狀態(已有 print-stats/parcel-alert 全域 listen 範式)。

## 3. 雲端 `cix3752iWeb` 改動
### 3a. 新聚合 API:日期區間進度
- Route(local-middleware 群):`GET clearance/progress?from=YYYY-MM-DD&to=YYYY-MM-DD`(to 省略=同 from;區間上限 3 天,後端 clamp)。
- Service:依 `ClearancePackage.whereBetween('clearance_date', [from,to])` 取 package_sn → join `OrderInfo` → 算:
  - `bag_total`(distinct package_sn)、`parcel_total`(訂單數)、`printed`(shipping_print_time 有值 或 ShippingPrintLog 命中)、`remaining = parcel_total - printed`。
- 回傳:
  ```json
  { "from": "...", "to": "...", "bag_total": N, "parcel_total": N,
    "printed": N, "remaining": N,
    "parcels": [ { "shipping_no": "...", "package_sn": "...", "printed": true } ] }
  ```
  `parcels` 清單供前端**去重遞減**(避免重印重複扣;對齊 bag_check apply 邏輯)。區間大時清單可能上千筆,一次性載入可接受;若過大再改分頁/省略 parcels 改純計數 + 信任事件去重。

### 3b. 日期頻道廣播
- `ParcelPrinted` 補一個 **`clearance_date`** 來源:dispatch 時(已知 package_sn)查 `ClearancePackage.clearance_date`;有值才額外 broadcast 到 **`clearance-date.{YYYYMMDD}`** 頻道(同 event name `parcel.printed`,payload 加 `clearance_date`)。
- 既有 `bag.{package_sn}` 廣播照舊(件數核對用);此為**額外**一條 date 頻道(件數核對與進度框各取所需)。
- 無 clearance_date(非清關單)→ 不發 date 頻道。

## 4. 中介端 `cix3752iLabelPrint` 改動
### 4a. sync 模組加「日期頻道」訂閱維度
- 現 `SyncManager` 只依 bag_check 持有袋訂 `bag.{sn}`。新增:一組「進度框啟用中的日期」`active_dates`,訂 `clearance-date.{d}`(區間內每天 1~3 條)。
- 新 command `progress_set_dates(dates: Vec<String>)`(浮動框開啟/換日期時呼叫;空=關閉訂閱)。
- 收到 date 頻道 `parcel.printed` → emit Tauri 事件 `clearance-progress-printed { clearance_date, package_sn, shipping_no, print_time }` 給前端。
- reconcile 與 bag 頻道共用同一條 WS 連線;斷線重連後依 `active_dates` 重訂。

### 4b. 前端浮動框(全域)
- **Pinia store** `useClearanceProgress`:`open`、`from/to`、`bag_total/parcel_total/printed/remaining`、`printedSet`(已印 shipping_no 集合,供去重)、`pos`(拖曳位置,記 localStorage)。
- **元件** `ClearanceProgressWidget.vue` 掛在 `DefaultLayout` template(RouterView 外)→ 跨頁不消失;可拖曳、可關閉(關閉時呼叫 `progress_set_dates([])` 退訂)。
- **啟動鈕**:`AppNavbar` 頂部加圖示鈕 → 開 store + 開日期選擇。
- 流程:選日期區間 → 呼叫聚合 API(經中介端 command→雲端)→ 填數字 + 建 printedSet + `progress_set_dates(區間日期)` → 監聽 `clearance-progress-printed`:該 shipping_no 不在 printedSet 才 `printed++ / remaining-- / printedSet.add`(去重,避免重印重複扣)。
- 非 Tauri 預覽:mock 數據。

## 5. 分期
| 階段 | 範圍 | 估時 |
|------|------|------|
| C1 雲端聚合 API | `clearance/progress` route + service(區間統計 + parcels)| 0.5–1 天 |
| C2 雲端日期頻道 | ParcelPrinted 補 clearance_date + broadcast 到 clearance-date.{d} | 0.5 天 |
| C3 中介 sync 日期訂閱 | SyncManager 加 active_dates + progress_set_dates command + emit 事件 | 0.5–1 天 |
| C4 前端浮動框 | store + 全域元件 + 拖曳/關閉 + navbar 鈕 + i18n | 1–2 天 |
| C5 驗證 | 真實查件 + 跨機列印 → 浮動框即時遞減(複用測試機,扮工控機/別台) | 0.5 天 |

## 6. 待確認(次要)
- 區間上限固定 3 天?還是任意區間(只是常見 1~3)。
- 浮動框要不要也顯示「分袋進度」或只總計數字?(先做總計數字,要細項再加)
- 「件數」定義 = 訂單數(parcel_total)。袋數 = distinct package_sn。確認與你認知一致。
