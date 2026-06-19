-- 查件異常記錄補上雲端回傳的精確物流單號。
-- 工控機掃的 query_no 是條碼原值(可能經正規化、為訂單編號或超商條碼),
-- 雲端 failResp 在 400 帶出的 shipping_no 才是該包裹確切物流單號,供異常清單對單。
ALTER TABLE parcel_alert ADD COLUMN shipping_no TEXT;
