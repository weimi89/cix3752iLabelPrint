-- 物流商加 print_profile 欄位:每個物流商對應一個列印設定字串
-- server.get_parcel 用 shipping_provider 查此欄位回給工控機
ALTER TABLE dispatch_provider ADD COLUMN print_profile TEXT;
