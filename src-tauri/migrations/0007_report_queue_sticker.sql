-- 工控機回報佇列補上「貼標人員」與「分流通道」欄位
-- POST /api/report 反查 parcel_query_log + sort_channels 後一併寫進來,
-- QueueLogPage 直接顯示,免再 join
ALTER TABLE report_queue ADD COLUMN job_sticker TEXT;
ALTER TABLE report_queue ADD COLUMN sort_channel TEXT;
