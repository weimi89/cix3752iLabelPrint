-- 每次 GET /api/parcel 的查詢紀錄,讓 POST /api/report 能用 response_id 反查原始查詢資料
-- 工控機 POST 時只需傳 response_id + 執行結果,其他欄位由 server 從這張表 JOIN 出來
CREATE TABLE IF NOT EXISTS parcel_query_log (
  response_id   INTEGER PRIMARY KEY,    -- 雲端產生的對應 ID
  query_no      TEXT NOT NULL,           -- 工控機傳入的查詢條碼
  tracking_no   TEXT NOT NULL,           -- 雲端回的真實追蹤號
  sort_channel  TEXT,
  print_profile TEXT,
  should_print  INTEGER NOT NULL DEFAULT 0,
  label_key     TEXT,
  created_at    TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_parcel_query_log_tracking ON parcel_query_log(tracking_no);
CREATE INDEX IF NOT EXISTS idx_parcel_query_log_created ON parcel_query_log(created_at);
