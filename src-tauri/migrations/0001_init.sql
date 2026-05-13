-- 系統設定的 key-value 補充欄位（複雜或敏感的不寫 config.toml 的東西）
CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL,
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 印表機設定（每個物流商一筆）
CREATE TABLE IF NOT EXISTS printer_profile (
  provider_code TEXT PRIMARY KEY,
  provider_name TEXT NOT NULL,
  printer_name TEXT,
  paper_width_mm REAL,
  paper_height_mm REAL,
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 預設物流商（與既有 web 端 SP_* 對齊）
INSERT OR IGNORE INTO printer_profile (provider_code, provider_name) VALUES
  ('7', '7-ELEVEN'),
  ('F', '全家'),
  ('O', '萊爾富'),
  ('C', '黑貓'),
  ('H', '新竹'),
  ('P', '宅配通'),
  ('E', '順豐速運'),
  ('S', '蝦皮（離線）'),
  ('A', '蝦皮（授權）');

-- 工控機回報佇列
CREATE TABLE IF NOT EXISTS report_queue (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  tracking_no TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending',     -- pending / sending / success / failed
  retry_count INTEGER NOT NULL DEFAULT 0,
  last_error TEXT,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now')),
  sent_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_report_queue_status ON report_queue(status);
CREATE INDEX IF NOT EXISTS idx_report_queue_tracking ON report_queue(tracking_no);

-- 圖片快取索引（記錄快取何時建立、來源、命中次數）
CREATE TABLE IF NOT EXISTS cache_meta (
  label_key TEXT PRIMARY KEY,
  local_path TEXT NOT NULL,
  source_url TEXT,
  size_bytes INTEGER NOT NULL DEFAULT 0,
  hit_count INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  last_hit_at TEXT
);

-- 事件 / 請求 log
CREATE TABLE IF NOT EXISTS event_log (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  level TEXT NOT NULL,            -- info / warn / error
  category TEXT NOT NULL,         -- server / cloud / cache / queue / printer
  action TEXT NOT NULL,
  message TEXT NOT NULL,
  context_json TEXT,
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_event_log_created ON event_log(created_at);
CREATE INDEX IF NOT EXISTS idx_event_log_category ON event_log(category, action);

-- 每日請求統計
CREATE TABLE IF NOT EXISTS daily_stats (
  date TEXT PRIMARY KEY,
  request_count INTEGER NOT NULL DEFAULT 0,
  success_count INTEGER NOT NULL DEFAULT 0,
  cache_hit INTEGER NOT NULL DEFAULT 0,
  cache_miss INTEGER NOT NULL DEFAULT 0
);

-- 掃描列印歷史（給操作員 UI 用）
CREATE TABLE IF NOT EXISTS scan_history (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  order_sn TEXT NOT NULL,
  shipping_no TEXT,
  provider_code TEXT,
  status TEXT NOT NULL,
  message TEXT,
  image_path TEXT,
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_scan_history_created ON scan_history(created_at);
