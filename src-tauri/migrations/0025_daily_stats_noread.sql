-- 工控機相機讀不到單號(NoRead)的每日件數統計欄位。
-- NoRead 不打雲端(只拍照存證),但仍是一次請求:request_count 照計、success_count 不計,
-- 另立 noread_count 供儀表板獨立呈現「今日讀碼失敗件數」。
ALTER TABLE daily_stats ADD COLUMN noread_count INTEGER NOT NULL DEFAULT 0;
