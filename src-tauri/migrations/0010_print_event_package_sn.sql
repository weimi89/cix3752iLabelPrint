-- 為 print_event 補上 package_sn(袋號)欄位,用於統計分袋數
-- 舊資料 package_sn 為 NULL,統計時 COUNT(DISTINCT package_sn) 自動跳過 NULL,不影響歷史準確性
ALTER TABLE print_event ADD COLUMN package_sn TEXT;
CREATE INDEX IF NOT EXISTS idx_print_event_package ON print_event(package_sn);
