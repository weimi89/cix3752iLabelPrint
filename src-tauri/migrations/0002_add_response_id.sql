-- 為 report_queue 加上 response_id 欄位，方便透過 queue_id 反查雲端對應 ID
ALTER TABLE report_queue ADD COLUMN response_id INTEGER;
CREATE INDEX IF NOT EXISTS idx_report_queue_response ON report_queue(response_id);
