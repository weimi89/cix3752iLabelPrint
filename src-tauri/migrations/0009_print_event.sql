-- 印單事件統一紀錄
-- 三個來源: 'scan'(掃描列印 / cloud_fetch_label)、'auto'(自動印單 / cloud_fetch_cloud_print)、'ipc'(工控機 GET /api/parcel)
-- 統計時用 COUNT(DISTINCT shipping_no) 去重(同一張單重印只算一次)
-- 完整保留事件:可後續擴展「重印次數」統計
CREATE TABLE IF NOT EXISTS print_event (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  source TEXT NOT NULL,                -- 'scan' / 'auto' / 'ipc'
  shipping_no TEXT NOT NULL,           -- 物流追蹤號(去重依據)
  provider_code TEXT,                  -- 物流商代碼(7/F/O/C/H/P/E/S/A)
  sticker_user TEXT,                   -- 貼標人員(scan/auto 由前端傳;ipc 由 sort_channels.job_sticker)
  scanner_user TEXT,                   -- 操作人員(保留欄位,目前統計不使用)
  created_at TEXT NOT NULL DEFAULT (datetime('now','localtime'))
);
CREATE INDEX IF NOT EXISTS idx_print_event_created ON print_event(created_at);
CREATE INDEX IF NOT EXISTS idx_print_event_shipping ON print_event(shipping_no);
CREATE INDEX IF NOT EXISTS idx_print_event_source ON print_event(source);
CREATE INDEX IF NOT EXISTS idx_print_event_provider ON print_event(provider_code);
