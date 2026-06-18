-- 分揀通道快速暫停開關。
-- 分揀進行中可臨時暫停某通道:暫停(enabled=0)的通道不參與路由分配
-- (resolve_channel_code 跳過),其餘指派同物流的通道照常 round-robin。
-- 預設 1(啟用),既有通道升級後維持啟用。
ALTER TABLE sort_channels ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1;
