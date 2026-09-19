-- 讀碼失敗照片回顧:現場翻看每張 NoRead 存證照,標記讀不到的原因(標籤朝下、反光、破損、太小、
-- 位置偏出視野、沒貼標、其他),統計哪種最多,才知道讀碼站要調什麼。
-- 一張照片一個原因;改標就覆蓋、清標就刪列。response_id 對到 parcel_query_log(NoRead 為負數 id)。
CREATE TABLE IF NOT EXISTS noread_review (
    response_id INTEGER PRIMARY KEY REFERENCES parcel_query_log(response_id) ON DELETE CASCADE,
    tag         TEXT NOT NULL,
    note        TEXT,
    reviewed_at TEXT NOT NULL DEFAULT (datetime('now','localtime'))
);
