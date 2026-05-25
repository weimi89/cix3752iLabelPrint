-- 統計擴充:重置機制 + 失敗事件記錄
--
-- 1) app_setting:輕量 KV 表,當前只存「本場作業起算時間 work_session_reset_at」
--    日後其他 app-wide 狀態(theme 自訂、最近用 printer)也可放這
--
-- 2) print_failure_event:獨立記錄失敗 / 異常事件,不污染 print_event 的去重邏輯
--    失敗時 shipping_no 經常為空,放同一表會破壞「COUNT(DISTINCT shipping_no)」語意

CREATE TABLE IF NOT EXISTS app_setting (
  key TEXT PRIMARY KEY,
  value TEXT,
  updated_at TEXT NOT NULL DEFAULT (datetime('now','localtime'))
);

-- 初次安裝:重置時間設為 migration 套用當下;舊使用者升級時也用當下,
-- 「本場累計」會從升級瞬間起累加,符合直覺
INSERT OR IGNORE INTO app_setting (key, value)
  VALUES ('work_session_reset_at', datetime('now','localtime'));

CREATE TABLE IF NOT EXISTS print_failure_event (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  source TEXT NOT NULL,                -- 'scan' / 'auto' / 'ipc'
  respond_code TEXT NOT NULL,          -- 'NO-DATA' / 'PRINT-ERROR' / 'ABNORMAL-SHIPMENT' / ...
  order_sn TEXT,                       -- 嘗試列印的訂單編號(可能沒有)
  shipping_no TEXT,                    -- 物流追蹤號(部分 respond_code 會帶)
  respond_message TEXT,                -- 雲端回應訊息(debug 用)
  scanner_user TEXT,
  sticker_user TEXT,
  created_at TEXT NOT NULL DEFAULT (datetime('now','localtime'))
);
CREATE INDEX IF NOT EXISTS idx_pfe_created ON print_failure_event(created_at);
CREATE INDEX IF NOT EXISTS idx_pfe_source ON print_failure_event(source);
CREATE INDEX IF NOT EXISTS idx_pfe_respond ON print_failure_event(respond_code);
