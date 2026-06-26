-- 請求記錄加入「讀碼站當下畫面」快照路徑(相對 cache 根目錄的 key,例 @captures/74Z01114611_20260625T204729.jpg)
-- 收到工控機 GET /api/parcel 當下,後端背景抓 USB 相機最新一幀存檔,寫回此欄。
-- 用途:當分揀線出現「沒貨卻出紙」爭議時,可在請求記錄頁直接調出當下讀碼站畫面佐證有無實體包裹。
-- 抓不到幀(相機未啟用/未接/權限未給)時保持 NULL,不影響正常出單流程。
ALTER TABLE parcel_query_log ADD COLUMN photo_path TEXT;
