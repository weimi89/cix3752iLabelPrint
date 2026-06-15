-- 為 print_event 補上 channel_code(分揀通道代碼)欄位,用於統計各分揀通道印單數
-- 只有 source='ipc'(工控機 GET /api/parcel)會帶值,scan/auto 為 GUI 操作無物理分揀通道,保持 NULL
-- 舊資料 channel_code 為 NULL,統計時以 channel_code IS NOT NULL 過濾,不影響歷史準確性
ALTER TABLE print_event ADD COLUMN channel_code TEXT;
CREATE INDEX IF NOT EXISTS idx_print_event_channel ON print_event(channel_code);
