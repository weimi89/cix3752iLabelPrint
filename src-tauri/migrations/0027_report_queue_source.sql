-- 回報佇列補上「這筆是誰建立的」與「工控機何時回報」
--
-- 背景:direct_print 模式下面單由中介機自己列印,工控機沒有列印動作,實務上也就不再
-- POST /api/report → 整條「回報 → 推雲端」斷掉,雲端收不到貼標人員。
-- 修法是中介機印完後自己補一筆(等一小段寬限時間讓工控機先回報),因此需要區分:
--   source           = 這筆佇列是誰建立的('ipc' 工控機回報 / 'direct_print' 中介機自印補記)
--   ipc_reported_at  = 工控機實際回報的時間(NULL = 從頭到尾沒回報過)
-- 兩者分開存,才能在「佇列歷史」頁分辨「工控機有確認收到分揀通道」與「只有面單印出來」,
-- 也才看得出工控機回報是否遲到(ipc_reported_at 晚於 sent_at)。

ALTER TABLE report_queue ADD COLUMN source TEXT NOT NULL DEFAULT 'ipc';
ALTER TABLE report_queue ADD COLUMN ipc_reported_at TEXT;

-- 既有資料一律是工控機回報建立的(此欄位存在前只有這條路徑),回填回報時間 = 建立時間
UPDATE report_queue SET ipc_reported_at = created_at WHERE ipc_reported_at IS NULL;

-- 同一筆列印記錄只能有一列佇列 —— 中介自補與工控機回報必須合併成同一列,
-- 否則同一件會被推兩次(雲端記兩筆印單)。
-- 建唯一索引前先去重:同一 response_id 保留「已成功推送」那筆,其餘取最新一筆。
DELETE FROM report_queue
WHERE response_id IS NOT NULL
  AND id NOT IN (
    SELECT keep_id FROM (
      SELECT id AS keep_id,
             ROW_NUMBER() OVER (
               PARTITION BY response_id
               ORDER BY (CASE WHEN status = 'success' THEN 0 ELSE 1 END), id DESC
             ) AS rn
      FROM report_queue
      WHERE response_id IS NOT NULL
    )
    WHERE rn = 1
  );

-- 刻意用「整表唯一索引」而非 partial index(WHERE response_id IS NOT NULL):
-- SQLite 的唯一索引本來就允許多筆 NULL(NULL 彼此不相等),行為與 partial 版一致;
-- 但 partial index 無法作為 upsert 的衝突目標(ON CONFLICT 會直接 prepare 失敗),
-- 而自補入列正是靠 ON CONFLICT(response_id) DO NOTHING 擋掉重複。
CREATE UNIQUE INDEX IF NOT EXISTS idx_report_queue_response_unique
  ON report_queue(response_id);
