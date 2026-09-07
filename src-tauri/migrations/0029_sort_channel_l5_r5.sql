-- 分揀機左右各加一個格口:固定位置由 8 個(L1-L4 / R1-R4)擴充為 10 個(L1-L5 / R1-R5)
--
-- 位置列是「先建好空殼、由設定頁填代碼」的預置資料,不建則設定頁的新卡片讀不到列、
-- 手機遙控頁的 /api/channels 也不會出現這兩格。INSERT OR IGNORE 讓已升級過的機器重跑無副作用。
-- updated_at 明寫 localtime:表定義的預設值是 UTC(0003 建表時所寫),沿用會讓新列比其他列少 8 小時。
INSERT OR IGNORE INTO sort_channels (position, updated_at)
VALUES ('L5', datetime('now','localtime')), ('R5', datetime('now','localtime'));
