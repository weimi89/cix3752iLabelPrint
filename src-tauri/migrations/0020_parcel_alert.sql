-- 雲端查件異常記錄(門市關轉 / 未確認 / 找不到 …),供手機 App 與桌面回看清單顯示。
-- 每次 GET /api/parcel 雲端回業務錯誤時寫一筆。
CREATE TABLE IF NOT EXISTS parcel_alert (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  kind         TEXT NOT NULL,            -- store_closed / unconfirmed / not_found / not_proxy / not_forward / error …
  code         TEXT,                     -- 原始雲端 code(STORE_CLOSED…)
  query_no     TEXT,                     -- 工控機查詢條碼
  message      TEXT,                     -- 雲端訊息
  channel_code TEXT,                     -- 該筆解析到的分揀通道(若有)
  created_at   TEXT NOT NULL DEFAULT (datetime('now','localtime'))
);
CREATE INDEX IF NOT EXISTS idx_parcel_alert_created ON parcel_alert(created_at);
