-- 回填 sticker_history:從既有印單事件 / 分揀通道的人員姓名匯入共用歷史名單。
-- 目的:升級到「人員下拉記憶」功能前已累積的操作者姓名,升級後立刻可在下拉選取,
-- 不必等使用者重新各做一次成功操作才出現。name 為 PRIMARY KEY,以 INSERT OR IGNORE 去重。

-- 掃描人員(操作人員)
INSERT OR IGNORE INTO sticker_history (name, used_at)
SELECT TRIM(scanner_user), MAX(created_at)
FROM print_event
WHERE scanner_user IS NOT NULL AND TRIM(scanner_user) <> ''
GROUP BY TRIM(scanner_user);

-- 貼單人員
INSERT OR IGNORE INTO sticker_history (name, used_at)
SELECT TRIM(sticker_user), MAX(created_at)
FROM print_event
WHERE sticker_user IS NOT NULL AND TRIM(sticker_user) <> ''
GROUP BY TRIM(sticker_user);

-- 分揀通道設定的貼標人員
INSERT OR IGNORE INTO sticker_history (name, used_at)
SELECT TRIM(job_sticker), datetime('now')
FROM sort_channels
WHERE job_sticker IS NOT NULL AND TRIM(job_sticker) <> ''
GROUP BY TRIM(job_sticker);
