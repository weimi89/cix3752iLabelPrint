---
title: "智配通 面單列印 — 設備異常通知 API"
subtitle: "POST /api/device-alert 廠商整合文件"
date: "2026-06-11"
---

# 文件說明

本文件說明**設備異常通知 API**（`POST /api/device-alert`）的呼叫方式，供工控機（分揀機台控制端）整合使用。

此 API 讓工控機在偵測到**設備異常**（例如分揀機台卡包裹、USB 接口異常斷線、掃描器／印表機故障）時，主動「喊一聲」通知中介機（智配通 面單列印 桌面 App）。中介機收到後會以**中文 + 越南語雙語語音廣播**喊話提示現場人員到場處理，並在畫面顯示 toast 提示。

> 本 API 為獨立的「通知」端點，與面單查詢（`GET /api/parcel`）、回報（`POST /api/report`）流程無關，可在任何時機獨立呼叫。

---

# 端點資訊

| 項目 | 內容 |
|---|---|
| 方法 | `POST` |
| 路徑 | `/api/device-alert` |
| 完整 URL | `http://<中介機IP>:18080/api/device-alert` |
| Content-Type | `application/json` |
| 回應 | 立即回 `200 OK`（不等廣播放完） |

預設埠為 `18080`（可於中介機設定頁調整）。

---

# 設計原則

**不讓工控機等。** 工控機只負責送出通知，中介機**立即回 200**，語音廣播全部在桌面 App 背景進行。工控機送完即可繼續作業，不需等待廣播播放完畢。

**雙語語音為預錄音檔（非即時合成）。** 內建分類碼的中／越雙語語音已**預先錄製內嵌進 App**（中文採 `HsiaoChen`、越南語採 `HoaiMy` neural 音色）。因此：

- 每台工控機（中介機）唸出來**音色一致、發音標準**
- **離線可用**，不依賴雲端
- **越南語免在 Windows 端安裝語音包**

僅當工控機傳入「未預先錄音的自訂 `type`」時，才退回系統 TTS（此情況下越南語才需要機器自備語音包）。

---

# Request

```http
POST /api/device-alert HTTP/1.1
Host: <中介機IP>:18080
Content-Type: application/json
```

## Body

```json
{
  "type": "PARCEL_JAM",
  "message": "L2 通道卡件",
  "repeat": 2
}
```

## 欄位說明

| 欄位 | 型別 | 必填 | 說明 |
|---|---|---|---|
| `type` | string | 否 | 異常分類碼（大寫）。中介機會自動轉大寫，工控機傳大小寫皆可。省略或空字串時當作 `ERROR`（通用設備異常） |
| `message` | string | 否 | 補充細節（如卡在哪個通道）。顯示在畫面 toast；**語音廣播只唸固定的雙語分類文案，不唸自訂 `message`** |
| `repeat` | integer | 否 | 雙語廣播重複次數。**預設 1**，中介機會自動限制在 **1～3** 之間（大於 3 取 3、小於 1 取 1） |

三個欄位皆可省略。若 body 為空物件 `{}`，等同 `type=ERROR`、`repeat=1`。

---

# 內建異常分類碼

下列 `type` 已預錄中／越雙語語音，雙語文案由 App 提供，工控機端無須處理翻譯：

| `type` | 中文廣播內容 | 越南語廣播內容 |
|---|---|---|
| `PARCEL_JAM` | 注意，分揀機台卡包裹，請現場人員立即處理 | Chú ý, máy phân loại bị kẹt kiện hàng, nhân viên hiện trường xử lý ngay |
| `USB_DISCONNECT` | 注意，USB 接口異常斷線，請檢查設備連線 | Chú ý, cổng USB bị ngắt kết nối bất thường, vui lòng kiểm tra kết nối thiết bị |
| `SCANNER_ERROR` | 注意，掃描器異常，請檢查設備 | Chú ý, máy quét gặp sự cố, vui lòng kiểm tra thiết bị |
| `PRINTER_ERROR` | 注意，印表機異常，請檢查設備 | Chú ý, máy in gặp sự cố, vui lòng kiểm tra thiết bị |
| `ERROR` | 注意，分揀設備發生異常，請現場人員確認 | Chú ý, thiết bị phân loại gặp sự cố, nhân viên hiện trường vui lòng kiểm tra |

> 傳入未列於上表的 `type` 不會報錯，App 會 fallback 成 `ERROR` 通用文案（並退回系統 TTS 發聲）。如需新增固定分類，僅需在 App 端補對應語音，**工控機端不需改動**。

---

# Response

## 成功（HTTP 200）

```json
{
  "message": "OK"
}
```

收到 200 即代表中介機已接收並排入廣播。**不代表廣播已播完** —— 廣播在背景進行。

## 失敗

| HTTP | 觸發條件 |
|---|---|
| `400 Bad Request` | body 非合法 JSON |

此端點不回業務錯誤；只要 body 為合法 JSON，一律回 200。

---

# 呼叫範例

## curl（測試用）

```bash
curl -X POST http://192.168.1.100:18080/api/device-alert \
  -H "Content-Type: application/json" \
  -d '{"type":"PARCEL_JAM","message":"L2 通道卡件","repeat":2}'
```

## C#（.NET）

```csharp
using var client = new HttpClient();
var json = "{\"type\":\"USB_DISCONNECT\",\"repeat\":1}";
var body = new StringContent(json, System.Text.Encoding.UTF8, "application/json");
await client.PostAsync("http://192.168.1.100:18080/api/device-alert", body);
```

## Python

```python
import requests

requests.post(
    "http://192.168.1.100:18080/api/device-alert",
    json={"type": "PARCEL_JAM", "message": "L2 通道卡件", "repeat": 2},
    timeout=3,
)
```

---

# 工控機使用注意事項

- 此為「通知」型端點，**與 `response_id`／查件流程無關**，可獨立在任何時機呼叫。
- 同一異常**持續存在**時，工控機可自行決定重發頻率（例如每 30 秒重發一次直到排除）。App 端每次收到都會**重新廣播**，並會先停掉前一筆未播完的語音，避免聲音重疊。
- `repeat` 用於「單次通知就重複喊幾遍」；持續性異常請改用「定時重發」而非把 `repeat` 設很大（上限就是 3）。
- 中介機立即回 200，僅代表已接收。實際是否播出取決於桌面 App 是否運行、音量是否開啟。
- **未錄音的自訂 `type` 才會退回系統 TTS**：此時越南語需中介機（Windows）自備越南語語音包，否則可能只唸得出中文。建議優先使用內建分類碼。

---

# 版本歷史

| 日期 | 變更 |
|---|---|
| 2026-06-11 | 新增 `POST /api/device-alert` 端點；中／越雙語預錄音檔廣播；`type` 大寫正規化；`repeat` 次數控制（預設 1、上限 3） |
