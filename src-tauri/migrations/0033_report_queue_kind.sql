-- 回報佇列補上「這筆要送什麼」
--
-- report      = 工控機回報 / 直印自補:把貼標人員推給雲端(logistic-cat webhook)
-- not_printed = 分揀機過機沒印出面單(物流沒指派格口、撞號改送、重複印單攔截、雲端沒給面單):
--               請雲端撤回查件當下記的那次列印,否則撿出重跑時會被當成重複印單
--
-- 與 source(這筆是誰建立的)分開存:兩者回答的是不同問題,佇列頁也分開顯示。
ALTER TABLE report_queue ADD COLUMN kind TEXT NOT NULL DEFAULT 'report';
