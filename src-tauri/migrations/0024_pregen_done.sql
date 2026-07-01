-- 面單預產「今日已預產的訂單編號」共用去重記憶。
-- 原本自動排程(記憶體區域變數)與手動頁(前端 localStorage)各記一份、互不同步,
-- 導致自動跑完後手動又全部重打雲端、全報「成功」。改存 DB 讓兩邊共讀共寫、且撐過重啟。
-- cache_day 以 04:00 為界(對齊快取清理慣例);跨快取日的舊列會在載入/寫入時清除。
CREATE TABLE IF NOT EXISTS pregen_done (
    order_sn  TEXT NOT NULL,
    cache_day TEXT NOT NULL,
    PRIMARY KEY (order_sn, cache_day)
);

CREATE INDEX IF NOT EXISTS idx_pregen_done_day ON pregen_done (cache_day);
