-- 推送佇列加退避排程欄位:worker 只撿 next_attempt_at <= now(或 NULL)的項目,
-- 失敗時依 capped 指數退避設定下次可送時間,取代原本「固定每 5 秒硬重試」。
-- NULL 視為「立即可送」(既有 pending 與新進件不受影響)。
ALTER TABLE report_queue ADD COLUMN next_attempt_at TEXT;
