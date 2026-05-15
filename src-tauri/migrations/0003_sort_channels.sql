-- 物流商主檔（給「分揀通道」設定下拉用，獨立於 printer_profile）
CREATE TABLE IF NOT EXISTS dispatch_provider (
  code TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 分揀通道（8 個固定位置：L1-L4 左、R1-R4 右）
CREATE TABLE IF NOT EXISTS sort_channels (
  position TEXT PRIMARY KEY,                -- 'L1'..'L4','R1'..'R4'
  channel_code TEXT,                        -- 通道代碼（可為空，但若填則需唯一）
  dispatch_code TEXT,                       -- 指派物流，對應 dispatch_provider.code
  job_sticker TEXT,                         -- 貼標人員
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
-- channel_code 不可重複（NULL 不算重複）
CREATE UNIQUE INDEX IF NOT EXISTS idx_sort_channels_code_unique
  ON sort_channels(channel_code) WHERE channel_code IS NOT NULL AND channel_code <> '';

-- 預先建立 8 個位置
INSERT OR IGNORE INTO sort_channels (position) VALUES
  ('L1'), ('L2'), ('L3'), ('L4'),
  ('R1'), ('R2'), ('R3'), ('R4');

-- 貼標人員歷史（供 autocomplete 建議用）
CREATE TABLE IF NOT EXISTS sticker_history (
  name TEXT PRIMARY KEY,
  used_at TEXT NOT NULL DEFAULT (datetime('now'))
);
