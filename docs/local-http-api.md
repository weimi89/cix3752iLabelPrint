---
title: "cix3752iLabelPrint 本地 HTTP API 規範"
subtitle: "給分揀機工控機調用"
author: "cix3752iLabelPrint"
date: "2026-05-17（2026-05-18 修訂：浮水印字型內嵌化;2026-05-19 修訂：補 print_num 與 parcel_query_log.shipping_provider 欄位）"
lang: zh-Hant
documentclass: report
---

# 文件說明

本文件描述 `cix3752iLabelPrint` 桌面應用程式內建的本地 HTTP API，提供給分揀機工控機（PLC / 邊緣裝置）於同一區網下呼叫。

- **適用對象**：工控機端開發人員、整合測試人員
- **協定**：HTTP/1.1（明文）
- **預設綁定**：`0.0.0.0:18080`（可於桌面 App 內「服務設定」修改）
- **編碼**：所有 JSON 採 UTF-8
- **CORS**：`tower_http::cors::CorsLayer::permissive()`，允許任意來源（同網段內整合用）
- **來源依據**：
  - 規格書 `~/Desktop/local_sorting_middleware_plan.md`
  - Rust 實作 `src-tauri/src/server/mod.rs`、`src-tauri/src/models/mod.rs`
  - 規格書與實作不一致或實作補充的部分，在內文以 *[從 code 補]* 或 *[實作差異]* 標註

---

# 端點總覽

| 方法 | Path | 用途 |
|---|---|---|
| `GET` | `/healthz` | 服務存活檢查 |
| `GET` | `/api/parcel/{queryNo}` | 工控機掃碼查詢包裹資料 |
| `POST` | `/api/report` | 工控機回報執行結果 |
| `POST` | `/api/device-alert` | 工控機回報設備異常（卡包裹 / USB 斷線 …），觸發雙語語音廣播 |
| `GET` | `/images/*` *[從 code 補]* | 面單圖檔靜態服務（`tower_http::ServeDir`，根路徑為快取目錄） |

---

# 統一回應格式

## 成功回應（HTTP 2xx）

`/healthz` 採用 `SuccessEnvelope`：

```json
{ "message": "OK", "data": { ... } }
```

業務 API（`/api/parcel`、`/api/report`）採用較輕量的格式：

- `GET /api/parcel`：`{ "data": { ... } }`（HTTP 200 即代表成功，無 `message`）
- `POST /api/report`：`{ "message": "OK" }`（無業務資料需回，只確認收到）
- `POST /api/device-alert`：`{ "message": "OK" }`（立即回 200，語音廣播由桌面 App 背景處理）

## 錯誤回應（HTTP 4xx / 5xx）

統一使用 `ApiErrorBody`：

```json
{
  "message": "<錯誤敘述>",
  "status_code": 404
}
```

- 無 `success` 欄位 —— HTTP 狀態碼即代表成功與否
- `status_code` 與 HTTP 狀態碼一致

---

# 1. GET /healthz

服務存活檢查。工控機可定時呼叫以判斷 Middleware 是否在線。

## Request

```http
GET /healthz HTTP/1.1
Host: <middleware-ip>:18080
```

無 query string，無 request body。

## Response

**成功（HTTP 200）**

```json
{
  "message": "OK",
  "data": {
    "name": "cix3752i-label-print"
  }
}
```

| 欄位 | 型別 | 說明 |
|---|---|---|
| `message` | string | 固定回 `"OK"` |
| `data.name` | string | 服務識別字串，固定 `"cix3752i-label-print"` |

`/healthz` 不會回傳錯誤 —— 若服務未啟動，工控機端會直接拿到 connection refused。

---

# 2. GET /api/parcel/{queryNo}

工控機掃碼後查詢包裹資料，取得分揀通道、列印設定與面單檔案路徑。

## Request

```http
GET /api/parcel/{queryNo} HTTP/1.1
Host: <middleware-ip>:18080
```

| Path 參數 | 型別 | 說明 |
|---|---|---|
| `queryNo` | string | 工控機掃描到的條碼。可為訂單編號、配送單號或物流條碼，由雲端 API 自行判斷類型 |

無 request body。

## 處理流程

1. 接收 `queryNo`
2. 呼叫雲端 `GET /api/v2/order-forward-print/{queryNo}`
3. 從雲端回應取得 6 個欄位：`order_sn`、`shipping_no`、`shipping_provider`、`shipping_image`、`print_num`（累計列印次數,觸發浮水印用）、`response_id`（debug 模式時為 `null`）
4. 用 `shipping_image` 推 `label_key`（例：`labels/SF123.png`），判斷本地是否有快取：
   - 命中 → `record_hit`
   - 未命中 → `record_miss` + 同步下載至完成（`fetch_now`）
5. 用 `shipping_provider` 查 `sort_channels.dispatch_code`，取得所有對應的 `channel_code`：
   - 排序：先 L 後 R，數字小到大（L1 < L2 < L3 < L4 < R1 < R2 < R3 < R4）
   - **同物流商配置多通道時，採 round-robin 輪流分配** *[從 code 補]*
6. 用 `shipping_provider` 查 `dispatch_provider.print_profile`，作為 `print_profile`
   - *[實作差異]*：規格書原寫從 `printer_profile.provider_code` 取 `printer_name`，實作中已改為從「指派物流」頁的 `dispatch_provider.print_profile` 欄位讀取
7. 寫入 `parcel_query_log`（response_id 為 PK，存在則 UPSERT）
   - `should_print` 固定寫 `1`（雲端 v2 路徑均代表「要列印」）
8. 更新 `daily_stats`（當日請求數 +1、成功數 +1）*[從 code 補]*
9. 回傳給工控機 4 個欄位

## Response

**成功（HTTP 200）**

```json
{
  "data": {
    "channel_code": "L1",
    "print_profile": "EPSON L6190 Series-3D0641-14",
    "label_path": "/Users/.../labels/SF0220862051573.png",
    "response_id": 1234567
  }
}
```

| 欄位 | 型別 | 可空 | 說明 |
|---|---|---|---|
| `channel_code` | string \| null | 是 | 分揀通道代碼（本地 `sort_channels`，用雲端 `shipping_provider` 查 `dispatch_code` 後依 round-robin 取一個）。無對應通道時為 `null` |
| `print_profile` | string \| null | 是 | 對應本機印表機設定（本地 `dispatch_provider.print_profile`，用 `shipping_provider` 查）。無對應設定時為 `null` |
| `label_path` | string（可省略） | 省略 | 面單存取路徑;格式由「面單路徑回傳模式」設定決定(見下)。**`direct_print` 模式、或同步下載失敗時,整個欄位不回傳**(不是 `null`,是 JSON 裡根本沒有這個 key);工控機應判斷「欄位是否存在」而非「是否為 null」 |
| `response_id` | integer \| null | 是 | 列印記錄 ID，工控機需於 `POST /api/report` 帶回以利配對。正常面單為雲端產生的正數；錯誤面單為 Middleware 本地產生的**負數**；雲端 debug 模式或記錄寫入失敗時為 `null`（此時不要回報） |
| `is_error_label` | boolean | 否 | **錯誤面單旗標**。正常面單時省略(視為 `false`);為 `true` 時代表這是一張「錯誤提示面單」(見下節) |
| `error_code` | string | 否 | 僅錯誤面單出現:機器可讀代碼(`STORE_CLOSED` / `NOT_FOUND` / …) |
| `message` | string | 否 | 僅錯誤面單出現:人類可讀的雲端原始錯誤敘述 |

### 錯誤面單（雲端業務錯誤 → HTTP 200 + `is_error_label`）

當雲端回業務錯誤（門市關轉 / 未確認 / 找不到 / 非代寄 / 非轉寄 / 狀態異常 / 出單失敗等所有**非 401** 的雲端錯誤），Middleware **不再回 502**，改為：自動產生一張「錯誤提示面單」(條碼 + 查詢號 + 對應圖示 + 中越雙語說明)，並以 **HTTP 200** 回傳，body 形態與正常面單相同，但帶 `is_error_label: true`：

```json
{
  "data": {
    "channel_code": "C03",
    "print_profile": "100x100",
    "label_path": "http://10.0.0.5:18080/images/@error/SF0220862051573_STORE_CLOSED.png",
    "response_id": -1,
    "is_error_label": true,
    "error_code": "STORE_CLOSED",
    "message": "無法列印，訂單門市關轉"
  }
}
```

**工控機處理規則**：

1. 看到 `is_error_label: true` 時，把 `label_path` **當成一般面單印出**（讓現場人員憑這張圖把異常包裹撿出處理）。`label_path` 的格式同樣依面單路徑模式（`local` / `share` / `http`）決定。
2. 錯誤面單的 `response_id` 為 Middleware 本地產生的**負數** ID（與雲端正數 ID 區隔），工控機**照正常流程 `POST /api/report` 回報即可**——Middleware 對負數 ID 只記錄本機、不推雲端，回應同樣是 200。**工控機端不需要為錯誤面單做任何特殊處理，整條流程（查詢 → 分揀 → 列印 → 回報）與正常面單完全相同**。僅在 `response_id` 為 `null`（中介端記錄寫入失敗的罕見退化）時不要回報。
3. **雲端查得到訂單的業務錯誤**（`STORE_CLOSED` / `UNCONFIRMED` / `STATUS_ABNORMAL` / `NOT_PROXY` / `NOT_FORWARD` / `LABEL_FAILED`）會帶出 `shipping_provider`，Middleware 照**正常面單的同一套流程**解析 `channel_code`（指派通道 round-robin，未指派時退回「未指派通道代碼」）與 `print_profile`——工控機把包裹分揀進該通道並列印錯誤面單即可，處理方式與正常面單一致。
4. **查無訂單**（`NOT_FOUND` 等雲端無法判斷物流商的錯誤）時，`channel_code` 統一退回設定頁的「未指派通道代碼」（`print_profile` 為 `null`）。亦即只要 Middleware 設定頁有設未指派通道，**所有錯誤面單都保證有 `channel_code`**；僅在該設定留空時才會是 `null`，此時工控機依自身邏輯處理（建議走異常/未指派格口）。
5. `direct_print` 模式下，錯誤面單由中介 PC 本機直接列印（優先使用該包裹解析到的 `channel_code` 在「分揀通道」頁設定的印表機；該通道未設印表機、或退回「未指派通道代碼」而對不到任何通道時，改用系統預設印表機），**不回傳 `label_path` 欄位**（與正常面單一致）。

> *[設計演進]* 舊版實作對雲端業務錯誤一律回 `502 Bad Gateway`，導致 `http` / `local` / `share` 模式下錯誤面單無法送達工控機（錯誤面單僅在 `direct_print` 模式有效）。v0.5.4 改為取向 A：錯誤面單與正常面單走同一條 `label_path` 出口，所有模式皆可印出。v0.5.6 起錯誤回應進一步帶出 `channel_code` / `print_profile`（雲端錯誤 body 含 `shipping_provider` 時，查不到物流商則統一退回未指派通道代碼）與本地負數 `response_id`（錯誤查詢同步寫入查詢記錄，回報可正常配對、不推雲端），讓分揀機把錯誤面單當一般面單跑完整流程，工控機端零特殊處理。

**仍回錯誤碼的情況**

| HTTP | message | 觸發條件 |
|---|---|---|
| `401 Unauthorized` | `雲端未登入,請先在桌面 App 完成登入` | Middleware 尚未登入雲端 API（系統層問題，現場無法處理，故不產錯誤面單） |
| `502 Bad Gateway` | 雲端錯誤敘述（透傳） | 僅在錯誤面單**暫存到 cache 失敗**時退化回此行為（同時嘗試本機列印） |

無論成功、錯誤面單或失敗，`daily_stats.request_count` 皆會 +1。

### NoRead（相機讀不到單號 → 不打雲端 + HTTP 200 + `error_code=NOREAD`）

工控機讀碼站**相機無法辨識條碼**時，請以 `queryNo = NoRead` 呼叫本 API（大小寫與底線/空白皆可，Middleware 正規化後比對 `noread`，故 `NoRead` / `NO_READ` / `no read` 皆可）。此時 Middleware：

1. **不提交雲端**（沒有單號可查，避免雲端一律回「查無訂單」的噪音）。
2. **仍拍照存證**：把收到請求當下釘住的讀碼站相機畫面存檔，檔名 `NoRead_{YYYYMMDDHHMMSS}_{序號}.jpg`（序號為進程內遞增值，確保同一秒多筆讀碼失敗不互相覆蓋），於桌面 App「請求記錄」頁可檢視。
3. **計入統計**：`daily_stats.request_count +1`、`noread_count +1`（不計 `success_count`）。桌面儀表板「本日請求數」旁會顯示讀碼失敗件數。
4. **袋件核對連續性不中斷**：NoRead 不帶袋號，不會打斷當前處理中袋的連續判定（等同「沒有袋號算含在連續次數內」）。

回應為 **HTTP 200**，無面單、無通道：

```json
{
  "data": {
    "channel_code": null,
    "print_profile": null,
    "response_id": null,
    "error_code": "NOREAD",
    "message": "讀碼失敗,未提交雲端"
  }
}
```

**工控機處理規則**：收到 `error_code = "NOREAD"` 時，此件無面單可印、無通道可分揀（`label_path` 欄位不存在、`response_id` 為 `null`），**不需 `POST /api/report`**。該包裹請依現場異常流程處理（人工補讀 / 撿出重掃）。

## 面單路徑回傳模式

`label_path` 的形態由 `config.toml` 中 `[label_path]` 區塊或設定頁「面單路徑回傳模式」決定,可在執行期熱套用(不需重啟 server)。四種模式:

| `mode` | `label_path` 範例 | 設定欄位 | 工控機行為 |
|---|---|---|---|
| `local`(預設) | `/Users/.../labels/2026/05/SF.png` | — | 直接讀本機檔(僅同機器有效) |
| `share` | `\\10.0.0.1\labels\2026\05\SF.png` | `share_root` | 經 SMB / NFS 掛載讀檔 |
| `http` | `http://192.168.1.50:18080/images/SF.png` | — | 走 HTTP GET 下載 |
| `direct_print` | (不回傳此欄位) | 分揀通道頁各通道 `printer_name` | 不讀檔;由中介機本機印表機直接列印 |

**`share` 模式**:Middleware 把本地 `cache_root` 前綴替換為 `share_root`,再依 `share_root` 含 `\` 與否決定分隔符風格。`share_root` 必須與 cache 根目錄指向同一份檔案的不同視角。

**`http` 模式**:設計為內網部署,直接以工控機請求的 Host header 組合 `http://{host}/images/{label_key}`,**無須額外設定**。內部走現有靜態檔案端點(第 4 節)。

**`direct_print` 模式**:**回應不含 `label_path` 欄位**(因 `Option::is_none` skip 而整個省略,不是 `null`),面單由中介機直接送本機印表機(依「分揀通道」頁各通道設定的 `printer_name`,以該包裹分配到的通道決定送哪台)。

**設定錯誤的退化策略**:`share_root` 為空時退回 `local`。

### 兩種列印模型(回應時序差異)

依「誰負責列印」分成兩條不同處理路徑:

| | 回傳工控機處理(`local`/`share`/`http`) | 中介機本機列印(`direct_print`) |
|---|---|---|
| 列印者 | **工控機**(讀 `label_path` 自行列印) | **中介機**(送本機印表機) |
| 回應內容 | 回 `label_path`(路徑/URL) | 不回 `label_path` 欄位 |
| 圖檔下載時機 | **回應前同步下載到完成**(工控機要讀檔,設計原則 #3) | **回應後背景處理**(立即回應,不讓工控機等雲端,設計原則 #2) |
| `parcel_query_log.label_ms` | 實際圖檔處理耗時 | `0`(回應路徑不含圖檔處理) |
| 列印順序保證 | 由工控機掌控(等每筆同步回應才送下一筆,天生照順序) | 由中介機**單一 FIFO 列印佇列**保證(見下) |

**`direct_print` 的列印順序保證**:中介機收到請求 → 取得雲端 metadata → 把「下載+浮水印+列印」工作**排入有序佇列**後立即回應。佇列由**單一 worker 逐筆 `await`**(下載完一筆、送印完一筆,才取下一筆),因此:

- 列印順序 **= 入列順序 = 工控機請求順序**(工控機在分揀線一件件刷、等回應才送下一件,故入列即照順序);**不會**因為「第 2 件圖檔下載比第 1 件快」而搶先印出。
- 同一時間只送印一筆,**不並發打印表機 spooler**(Windows GDI 對並發 stale-state 敏感)。
- 圖檔下載逾時 30 秒:某筆下載卡住最多塞住佇列 30 秒,逾時後該筆略過、後續照印(維持其餘相對順序)。

## 列印次數浮水印(對齊雲端 OrderPrintController)

雲端 `/api/parcel` 回應夾帶 `print_num` 欄位(累計列印次數)。當 `print_num > 1` 時,Middleware 會生成一份帶浮水印的副本,`label_path` 指向該副本(原圖保留不改)。

- **觸發條件**:`print_num > 1`
- **浮水印**:`({print_num})` 16pt 純黑(字型已隨應用內嵌,使用者無需部署字型檔)
- **位置**:
  - 順豐速運(`shipping_provider = "E"`):右下角,距右 30px、距下 50px
  - 其他物流商:右上角,距右 15px、距上 50px
- **副本 key**:`@repeat/W{provider}-{原檔名}` (例:`@repeat/WH-abc.png`)
- **覆寫策略**:每次 GET 同包裹都重新生成(成本低,確保 `print_num` 變化即時反映)
- **fallback**:浮水印生成失敗(讀檔、寫檔 I/O 異常)→ 回原圖,log 警告,不阻斷出單

## 工控機使用注意事項

- `local` / `share` 模式下 `label_path` 為**絕對路徑**,工控機需有檔案系統讀取權限
- `http` 模式下 `label_path` 為 URL,工控機需用 HTTP GET 取得 binary
- 若回應**不含 `label_path` 欄位**:
  - `direct_print` 模式 → 正常情況,面單由中介機自印,工控機不需取面單
  - 非 `direct_print` 但欄位仍缺 → 同步下載失敗,工控機可選擇:
    1. 重新呼叫 `GET /api/parcel/{queryNo}`(首次未命中觸發了下載,第二次通常會命中本地快取)
    2. 改用 `/images/{label_key}` 走 HTTP 拉取(見第 4 節)
- `response_id` 在後續 `POST /api/report` 為**必填**;若為 `null`,本次查詢不會寫 `parcel_query_log`,後續無法回報

---

# 3. POST /api/report

工控機回報執行結果。**極簡設計**：工控機只需帶 `response_id`，Middleware 反查 `parcel_query_log` 取得其餘欄位，組裝 payload 後寫入本地 queue，立即回 200，背景 worker 推送雲端 webhook。

## Request

```http
POST /api/report HTTP/1.1
Host: <middleware-ip>:18080
Content-Type: application/json
```

**Body**

```json
{
  "response_id": 1234567
}
```

| 欄位 | 型別 | 必填 | 說明 |
|---|---|---|---|
| `response_id` | integer | 是 | 對應同一次 `GET /api/parcel` 回傳的 `response_id` |

工控機**完全不需**傳其他欄位（`tracking_no`、執行結果、時間戳等）。

## 處理流程

1. 用 `response_id` 在 `parcel_query_log` 反查 `tracking_no`、`sort_channel`
2. 若 `response_id` 在 `parcel_query_log` 找不到 → 回 `422 Unprocessable Entity`
3. 用 `sort_channel` 反查 `sort_channels.job_sticker`（貼標人員）
4. 將 `ReportPayload { response_id }` 序列化進 `payload_json`，連同 `tracking_no`、`sort_channel`、`job_sticker` 寫入 `report_queue`（`status=pending`）
5. 立即回 `200 OK`，背景 worker 推送雲端 `logistic-cat` webhook

## Response

**成功（HTTP 200）**

```json
{
  "message": "OK"
}
```

說明：

- `queue_id` 為 server 內部佇列序號，不外露給工控機。如需 debug 可從桌面 App 的「佇列歷史」頁查看
- `response_id` 不在回應中重複出現；Server 會與 `payload_json` 一同存入 `report_queue`

**失敗**

| HTTP | message | 觸發條件 |
|---|---|---|
| `422 Unprocessable Entity` | `找不到 response_id={n} 對應的查詢紀錄，請先 GET /api/parcel` | `response_id` 在 `parcel_query_log` 不存在（工控機跨越 server 重啟、log 表被清空，或亂帶值） |
| `500 Internal Server Error` | SQLx / queue 錯誤敘述（透傳） | 資料庫查詢或 queue 寫入失敗 |

**錯誤範例（HTTP 422）**

```json
{
  "message": "找不到 response_id=1234567 對應的查詢紀錄，請先 GET /api/parcel",
  "status_code": 422
}
```

## 工控機使用注意事項

- 只在收到 200 後才能認定回報成功；4xx / 5xx 均應重試
- `422` 表示這個 `response_id` 不可能用了，重試也沒用 —— 工控機應記錄錯誤並改走人工流程
- `500` 為暫時性錯誤，工控機可以稍後重試（建議指數退避）
- Middleware 立即回 200 不代表雲端已收到 —— 背景 worker 推送雲端失敗會在 `report_queue.last_error` 留紀錄，需到桌面 App 「佇列歷史」頁查看

---

# 4. POST /api/device-alert

工控機回報**設備異常**（分揀機台卡包裹、USB 接口異常斷線、掃描器/印表機故障等）。桌面 App 收到後，會用**中文 + 越南語雙語**語音廣播，喊話提示現場人員到場處理，並在畫面跳 toast。

**設計原則同 `POST /api/report`：不讓工控機等。** 工控機只負責「喊一聲」，Middleware 立即回 200，語音廣播全在桌面 App 背景進行，工控機不需等廣播放完。

**發聲方式：預錄音檔（非系統 TTS）。** 內建分類碼的中/越雙語語音已**預先錄製內嵌進 App**（中文 `HsiaoChen`、越南語 `HoaiMy` neural 音色），每台工控機唸出來音色一致、發音標準、**離線可用、越南語免在 Windows 裝語音包**。只有傳入未錄音的自訂 `type` 時，才退回系統 TTS（此時越南語需機器自備語音包）。

## Request

```http
POST /api/device-alert HTTP/1.1
Host: <middleware-ip>:18080
Content-Type: application/json
```

**Body**

```json
{
  "type": "PARCEL_JAM",
  "message": "L2 通道卡件",
  "repeat": 2
}
```

| 欄位 | 型別 | 必填 | 說明 |
|---|---|---|---|
| `type` | string | 否 | 異常分類碼（大寫，對齊雲端機器碼風格）。Middleware 會自動轉大寫，工控機傳大小寫皆可。省略或空字串時當作 `ERROR`（通用設備異常） |
| `message` | string | 否 | 補充細節（如卡在哪個通道），顯示在 toast；語音廣播只唸固定的雙語分類文案，不唸自訂 `message` |
| `repeat` | integer | 否 | 雙語廣播重複次數。**預設 1**，Middleware 會 clamp 到 **1～3**（上限 3，超過取 3，小於 1 取 1），避免誤帶大數洗版 |

**內建分類碼**（雙語文案由 App i18n 提供，現場無須關心翻譯）：

| `type` | 中文廣播 |
|---|---|
| `PARCEL_JAM` | 注意，分揀機台卡包裹，請現場人員立即處理 |
| `USB_DISCONNECT` | 注意，USB 接口異常斷線，請檢查設備連線 |
| `SCANNER_ERROR` | 注意，掃描器異常，請檢查設備 |
| `PRINTER_ERROR` | 注意，印表機異常，請檢查設備 |
| `ERROR` | 注意，分揀設備發生異常，請現場人員確認 |

> 傳入未列於上表的 `type` 不會報錯，App 會 fallback 成 `ERROR` 通用文案。新增固定分類只需在 App 端補 i18n key（大寫），不需改工控機。

## 處理流程

1. 解析 `type`（轉大寫；空 → `ERROR`）、`message`、`repeat`（空 → 1，clamp 1～3）
2. emit `device-alert` 事件 `{ alert_type, message, repeat }` 給桌面前端
3. 寫一筆 `warn` 等級事件 log（分類 `server`，標題「設備異常」）
4. 立即回 `200 OK`
5. 前端 `useDeviceAlert` 收到事件 → 依 `type` 播預錄中/越雙語音檔 `repeat` 次（未錄音類型退回系統 TTS）→ 顯示 toast

## Response

**成功（HTTP 200）**

```json
{
  "message": "OK"
}
```

此端點不回業務錯誤；body 解析失敗（非合法 JSON）由 axum 回 `400 Bad Request`。

## 工控機使用注意事項

- 此為「通知」型端點，與 `response_id` / 查件流程無關，可在任何時機獨立呼叫
- 同一異常**持續存在**時，工控機可自行決定重發頻率（例如每 30 秒重發一次直到排除）；App 端每次收到都會重新廣播，並會先停掉前一筆未播完的語音避免疊聲
- **越南語廣播需要 Windows 安裝 vi 語音包**：工控機機台若未安裝越南語語音，TTS 會 fallback 成系統預設嗓音，越南語可能腔調怪或唸不出。安裝路徑：Windows「設定 → 時間與語言 → 語言 → 新增越南語 → 語音」。中文（zh-TW）語音 Windows 預設多半已內建

---

# 5. GET /images/* *[從 code 補]*

面單圖檔的靜態檔案服務。實作於 `src-tauri/src/server/mod.rs` 的 `.nest_service("/images", ServeDir::new(cache.base_dir()))`。

## Request

```http
GET /images/{label_key} HTTP/1.1
Host: <middleware-ip>:18080
```

| Path 參數 | 型別 | 說明 |
|---|---|---|
| `label_key` | string | 例：`labels/2026/05/SF123456789.png` |

## Response

- **HTTP 200**：直接回傳圖檔內容（`image/png` 等），由 `tower_http::services::ServeDir` 處理
- **HTTP 404**：檔案不存在

## 用途

工控機若不便讀取絕對路徑（跨機器、權限限制等），可改走 `/images/{label_key}` 取得面單圖。`label_key` 可由 `GET /api/parcel` 回傳的 `label_path` 反推（去掉快取根目錄前綴）。

> 規格書未提及此端點，但實作中已存在。若需穩定使用，建議與 Middleware 維護方確認是否視為正式 API。

---

# 6. 錯誤碼總表

| HTTP | 端點 | 觸發條件 |
|---|---|---|
| `401` | `GET /api/parcel` | 雲端未登入 |
| `422` | `POST /api/report` | `response_id` 找不到對應的 `parcel_query_log` |
| `500` | `POST /api/report` | 資料庫 / queue 寫入失敗 |
| `502` | `GET /api/parcel` | 雲端 API 錯誤（非 401） |

---

# 6. 完整呼叫流程範例

工控機完整一次掃碼到回報的流程：

```text
工控機掃碼
    │
    ▼
GET /api/parcel/SF0220862051573
    │
    ▼
Middleware 呼叫雲端 → 拿到 shipping_no / shipping_image / response_id
Middleware 判斷本地 cache（未命中則同步下載）
Middleware 查 sort_channels → channel_code = "L1"（round-robin）
Middleware 查 dispatch_provider → print_profile
Middleware 寫 parcel_query_log（response_id 為 key）
    │
    ▼
HTTP 200 {
  "data": {
    "channel_code": "L1",
    "print_profile": "EPSON L6190 Series-3D0641-14",
    "label_path": "/Users/.../labels/2026/05/SF0220862051573.png",
    "response_id": 1234567
  }
}
    │
    ▼
工控機讀 label_path → 走本機印表機列印
工控機根據 channel_code 控制分流機構
    │
    ▼
POST /api/report  { "response_id": 1234567 }
    │
    ▼
Middleware 用 response_id 反查 parcel_query_log
Middleware 取 tracking_no / sort_channel / job_sticker
Middleware 寫 report_queue（status=pending）
    │
    ▼
HTTP 200 { "message": "OK" }
    │
    ▼
（背景）Worker 推送雲端 logistic-cat webhook
```

---

# 7. 相關資料表（Middleware 內部）

工控機不直接接觸 SQLite，以下僅作為理解上述端點背後狀態的參考。

## parcel_query_log

每次 `GET /api/parcel` 成功（雲端有回 `response_id`）就 INSERT/UPDATE 一筆。

| 欄位 | 說明 |
|---|---|
| `response_id` | PK，雲端產生的對應 ID |
| `query_no` | 工控機傳入的查詢條碼 |
| `tracking_no` | 雲端回的真實追蹤號 |
| `shipping_provider` | 雲端回的物流商代碼（7/F/O/C/H/P/S/A/J/E),供後續查 dispatch_provider / sort_channels 用 |
| `sort_channel` | 本次分配的分揀通道（round-robin 結果） |
| `print_profile` | 列印 profile |
| `should_print` | 固定 `1` |
| `label_key` | 圖檔 key |
| `created_at` | 建立時間 |

## report_queue

| 欄位 | 說明 |
|---|---|
| `id` | 自增 PK，桌面 App「佇列歷史」頁的 `queue_id` |
| `tracking_no` | 從 `parcel_query_log` 反查 |
| `payload_json` | 序列化的 `ReportPayload`（目前僅含 `response_id`） |
| `response_id` | 與 `payload_json` 內一致，獨立索引 |
| `sort_channel` / `job_sticker` | 反查結果，供桌面 UI 顯示 |
| `status` | `pending` / `sending` / `success` / `failed` |
| `retry_count` | 推送雲端失敗重試次數 |
| `last_error` | 最近一次失敗原因 |
| `created_at` / `updated_at` / `sent_at` | 時間戳 |

## daily_stats *[從 code 補]*

| 欄位 | 說明 |
|---|---|
| `date` | PK，當日（local time） |
| `request_count` | `/api/parcel` 當日呼叫次數（含成功、失敗與 NoRead） |
| `success_count` | `/api/parcel` 當日成功次數 |
| `noread_count` | 當日 NoRead（相機讀不到單號）件數（`request_count` 的失敗細分，不計入 `success_count`） |

---

# 8. 設計原則回顧

以下原則沿用規格書，供工控機端理解設計理念：

1. **不回 base64**：API 回 `label_path`（或 `/images/{key}`），不直接傳檔案內容
2. **不讓工控機等待雲端回報**：`POST /api/report` 立即回 200，雲端推送由背景 worker 接手
3. **面單一律走本地路徑**：本地未命中時 Middleware **同步下載**至完成才回 `label_path`，工控機不需自行抓雲端

---

# 附錄 A：版本與來源

- **本文件版本**:2026-05-17 初版;2026-05-18 修訂(浮水印字型改為內嵌,移除字型放置教學;API 對外契約未變動);2026-05-19 修訂(處理流程補 `print_num` 為雲端回應的第 6 個欄位;`parcel_query_log` 表格補 `shipping_provider` 欄位;API 對外契約未變動)
- **規格書來源**：`~/Desktop/local_sorting_middleware_plan.md`（2026-05-15 修訂版）
- **實作來源**：
  - `src-tauri/src/server/mod.rs`
  - `src-tauri/src/models/mod.rs`
- **驗證方式**：`yarn tauri:dev` 啟動後使用 `curl` 或 Postman 對 `http://127.0.0.1:18080/` 發送請求

# 附錄 B：curl 速查

```bash
# 健康檢查
curl -s http://127.0.0.1:18080/healthz | jq

# 查詢包裹
curl -s http://127.0.0.1:18080/api/parcel/SF0220862051573 | jq

# 回報執行結果
curl -s -X POST http://127.0.0.1:18080/api/report \
  -H 'Content-Type: application/json' \
  -d '{"response_id": 1234567}' | jq

# 直接抓面單圖檔
curl -o /tmp/label.png http://127.0.0.1:18080/images/labels/2026/05/SF0220862051573.png
```
