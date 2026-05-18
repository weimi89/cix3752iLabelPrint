-- 在 parcel_query_log 補上物流商代碼,讓「請求記錄」頁可顯示 shipping_provider
-- 舊資料留 NULL(只有此 migration 之後的查詢才會寫入)
ALTER TABLE parcel_query_log ADD COLUMN shipping_provider TEXT;
