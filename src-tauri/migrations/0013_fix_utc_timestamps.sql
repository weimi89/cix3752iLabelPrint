-- 修正時差：舊版時間欄位以 datetime('now') 寫入 UTC，台灣為 UTC+8，
-- 統一補 +8 小時對齊本地時間。
-- 不影響 print_event / print_failure_event / app_setting（已用 localtime 寫入）。
UPDATE settings         SET updated_at  = datetime(updated_at,  '+8 hours') WHERE updated_at  IS NOT NULL;
UPDATE printer_profile  SET updated_at  = datetime(updated_at,  '+8 hours') WHERE updated_at  IS NOT NULL;
UPDATE report_queue     SET created_at  = datetime(created_at,  '+8 hours'),
                            updated_at  = datetime(updated_at,  '+8 hours'),
                            sent_at     = datetime(sent_at,     '+8 hours')
                        WHERE created_at IS NOT NULL;
UPDATE cache_meta       SET created_at  = datetime(created_at,  '+8 hours'),
                            last_hit_at = datetime(last_hit_at, '+8 hours')
                        WHERE created_at IS NOT NULL;
UPDATE event_log        SET created_at  = datetime(created_at,  '+8 hours') WHERE created_at  IS NOT NULL;
UPDATE scan_history     SET created_at  = datetime(created_at,  '+8 hours') WHERE created_at  IS NOT NULL;
UPDATE dispatch_provider SET updated_at = datetime(updated_at,  '+8 hours') WHERE updated_at  IS NOT NULL;
UPDATE sort_channels    SET updated_at  = datetime(updated_at,  '+8 hours') WHERE updated_at  IS NOT NULL;
UPDATE sticker_history  SET used_at     = datetime(used_at,     '+8 hours') WHERE used_at     IS NOT NULL;
UPDATE parcel_query_log SET created_at  = datetime(created_at,  '+8 hours') WHERE created_at  IS NOT NULL;
