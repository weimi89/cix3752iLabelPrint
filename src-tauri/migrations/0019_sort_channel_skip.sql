-- 分揀通道「跳過本輪」次數。
-- 某物流指派多個通道 round-robin 輪流分配時,若某通道臨時卡關,
-- 可累加 skip_count;輪到該通道時消耗一次並改分配給下一個通道(不必整個暫停)。
-- 預設 0(不跳過)。
ALTER TABLE sort_channels ADD COLUMN skip_count INTEGER NOT NULL DEFAULT 0;
