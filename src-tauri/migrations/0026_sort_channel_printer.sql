-- 本機印表機設定從「指派物流」(dispatch_provider) 移到「分揀通道」(sort_channels)
-- direct_print 模式改依包裹分配到的分揀通道決定送哪台印表機
ALTER TABLE sort_channels ADD COLUMN printer_name TEXT;

-- 回填:**僅在該通道所有指派物流都指向同一台印表機時**才自動帶入,讓單純的舊部署
-- (一個通道一台印表機)升級後不需重設即可繼續列印。
--
-- 刻意不用「任選第一筆」:一個通道可指派多個物流商(sort_channel_dispatch 是多對多),
-- 若 B→Zebra-A、S→Zebra-B 兩台實體印表機/不同紙材,任選一台會讓另一個物流商的面單
-- 從錯的機器印出 —— 而且不會有任何 warn 或 event_log,現場只看到「面單跑錯機器」卻查不到原因。
-- 分歧時留 NULL,由操作員在「分揀通道」頁明確指定(該頁對未設印表機的通道會顯示警示)。
--
-- dispatch_provider.printer_name 已無任何讀寫路徑,但**刻意保留不 DROP**:
-- 本 App 有自動更新(tauri-plugin-updater),使用者可能回退到舊版,舊版的
-- dispatch_provider_list 仍會 SELECT 該欄,DROP 掉會讓回退後的舊版直接查詢失敗。
-- (與 0017 DROP sort_channels.dispatch_code 的差別:那次是 schema 重構且無回退相容需求。)
UPDATE sort_channels
SET printer_name = (
  SELECT MIN(dp.printer_name)
  FROM sort_channel_dispatch scd
  JOIN dispatch_provider dp ON dp.code = scd.dispatch_code
  WHERE scd.position = sort_channels.position
    AND dp.printer_name IS NOT NULL AND dp.printer_name != ''
  HAVING COUNT(DISTINCT dp.printer_name) = 1
)
WHERE printer_name IS NULL;
