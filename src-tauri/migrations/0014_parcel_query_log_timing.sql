-- 請求記錄加入三段計時欄位（毫秒）
-- cloud_ms: 雲端 API fetch_parcel 所花時間
-- label_ms: 面單快取確認 + 下載 + 浮水印所花時間
-- total_ms: 整個 GET /api/parcel handler 總執行時間
ALTER TABLE parcel_query_log ADD COLUMN cloud_ms INTEGER;
ALTER TABLE parcel_query_log ADD COLUMN label_ms INTEGER;
ALTER TABLE parcel_query_log ADD COLUMN total_ms INTEGER;
