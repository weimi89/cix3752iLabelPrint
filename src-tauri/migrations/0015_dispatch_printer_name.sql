-- 物流商新增「本機直接列印」印表機欄位
-- 當 label_path.mode = direct_print 時，GET /api/parcel 由本機直接列印對應印表機，不回路徑給工控機
ALTER TABLE dispatch_provider ADD COLUMN printer_name TEXT;
