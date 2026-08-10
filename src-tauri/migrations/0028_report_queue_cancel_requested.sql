-- 回報佇列補上「已請求攔截」旗標
--
-- 背景:直印失敗時要攔下該件的雲端回報(面單沒印出來,雲端不該記成完成)。
-- 光靠 status='cancelled' 擋不住一種局面:攔截發生時該筆正在 `sending`(webhook 呼叫中)。
-- 此時狀態不能直接改(推送結果未定),但這次推送若失敗,重試邏輯會把它轉回 failed 再送一次 ——
-- 重試完全不知道有人要求過攔截,於是「列印失敗的件」還是上了雲端。
--
-- 因此把「有人要求攔截」與「目前推到哪了」拆成兩個獨立事實:
--   status           = 這筆推送進行到哪個階段
--   cancel_requested = 是否已被要求攔截(一旦為 1 就不再參與推送,直到列印成功才會被清回 0)
-- worker 撿件與 claim 都會檢查此旗標,推送結果寫回前也會再看一次。

ALTER TABLE report_queue ADD COLUMN cancel_requested INTEGER NOT NULL DEFAULT 0;

-- 既有的 cancelled 列(0027 之後、本次之前產生的)補上旗標,語意一致
UPDATE report_queue SET cancel_requested = 1 WHERE status = 'cancelled';

-- worker 每輪都會用到「未被攔截且到期」這組條件,建索引避免佇列變長後掃全表
CREATE INDEX IF NOT EXISTS idx_report_queue_sendable
  ON report_queue(cancel_requested, status, next_attempt_at);
