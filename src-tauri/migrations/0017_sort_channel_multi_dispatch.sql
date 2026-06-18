-- 分揀通道 ↔ 物流商 由「1 對 1」改為「1 對多」
-- 原本 sort_channels.dispatch_code 單一欄位(一個通道只能指派一個物流),
-- 改為獨立關聯表 sort_channel_dispatch,讓一個分揀通道可指派多個物流商。
CREATE TABLE IF NOT EXISTS sort_channel_dispatch (
  position TEXT NOT NULL,         -- 對應 sort_channels.position(L1..L4 / R1..R4)
  dispatch_code TEXT NOT NULL,    -- 對應 dispatch_provider.code
  PRIMARY KEY (position, dispatch_code)
);

-- 加速依物流商反查通道(resolve_channel_code 路由用)
CREATE INDEX IF NOT EXISTS idx_scd_dispatch ON sort_channel_dispatch(dispatch_code);

-- 把舊的單一指派資料搬進關聯表(空字串不搬)
INSERT OR IGNORE INTO sort_channel_dispatch (position, dispatch_code)
SELECT position, dispatch_code
FROM sort_channels
WHERE dispatch_code IS NOT NULL AND dispatch_code <> '';

-- 移除舊的單一欄位,改由 sort_channel_dispatch 作唯一來源,避免兩處資料不一致
ALTER TABLE sort_channels DROP COLUMN dispatch_code;
