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
| `label_path` | string \| null | 是 | 面單存取路徑;格式由「面單路徑回傳模式」設定決定(三選一,見下)。同步下載失敗時為 `null`,工控機可重試 |
| `response_id` | integer \| null | 是 | 雲端列印記錄 ID，工控機需於 `POST /api/report` 帶回以利配對；雲端 debug 模式時為 `null` |

**失敗**

| HTTP | message | 觸發條件 |
|---|---|---|
| `401 Unauthorized` | `雲端未登入,請先在桌面 App 完成登入` | Middleware 尚未登入雲端 API |
| `502 Bad Gateway` | 雲端錯誤敘述（透傳） | 雲端 API 拒絕、逾時、解析失敗等所有非 401 的雲端錯誤 |

**注意** *[實作差異]*：規格書範例提到 `404 找不到此包裹條碼`，但實作中所有非 401 的雲端錯誤一律歸 `502 Bad Gateway`（在 `message` 中保留雲端原始錯誤敘述）。

**錯誤範例（HTTP 502）**

```json
{
  "message": "雲端 API 錯誤: 404 NOT FOUND",
  "status_code": 502
}
```

無論成功或失敗，`daily_stats.request_count` 皆會 +1。

## 面單路徑回傳模式

`label_path` 的形態由 `config.toml` 中 `[label_path]` 區塊或設定頁「面單路徑回傳模式」決定,可在執行期熱套用(不需重啟 server)。三種模式:

| `mode` | `label_path` 範例 | 設定欄位 | 工控機行為 |
|---|---|---|---|
| `local`(預設) | `/Users/.../labels/2026/05/SF.png` | — | 直接讀本機檔(僅同機器有效) |
| `share` | `\\10.0.0.1\labels\2026\05\SF.png` | `share_root` | 經 SMB / NFS 掛載讀檔 |
| `http` | `http://192.168.1.50:18080/images/SF.png` | — | 走 HTTP GET 下載 |

**`share` 模式**:Middleware 把本地 `cache_root` 前綴替換為 `share_root`,再依 `share_root` 含 `\` 與否決定分隔符風格。`share_root` 必須與 cache 根目錄指向同一份檔案的不同視角。

**`http` 模式**:設計為內網部署,直接以工控機請求的 Host header 組合 `http://{host}/images/{label_key}`,**無須額外設定**。內部走現有靜態檔案端點(第 4 節)。

**設定錯誤的退化策略**:`share_root` 為空時退回 `local`。

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
- 若 `label_path` 為 `null`,工控機可選擇:
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

# 4. GET /images/* *[從 code 補]*

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

# 5. 錯誤碼總表

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
| `request_count` | `/api/parcel` 當日呼叫次數（含成功與失敗） |
| `success_count` | `/api/parcel` 當日成功次數 |

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
