-- =====================================================================
-- Rollback Migration 018: Revert Subscription and Plan Schema Alignment
-- Database: u588459601_deskstream
-- =====================================================================

SET FOREIGN_KEY_CHECKS = 0;

-- Drop added index from subscriptions
SET @exist_sub_idx_user := (
    SELECT COUNT(*) FROM information_schema.STATISTICS 
    WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'subscriptions' AND INDEX_NAME = 'idx_subscriptions_user_id'
);
SET @sql_drop_idx := IF(@exist_sub_idx_user > 0, 'ALTER TABLE `subscriptions` DROP INDEX `idx_subscriptions_user_id`', 'SELECT 1');
PREPARE stmt FROM @sql_drop_idx;
EXECUTE stmt;
DEALLOCATE PREPARE stmt;

-- Drop added columns from subscriptions
SET @exist_sub_expires_at := (
    SELECT COUNT(*) FROM information_schema.COLUMNS 
    WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'subscriptions' AND COLUMN_NAME = 'expires_at'
);
SET @sql_drop_exp := IF(@exist_sub_expires_at > 0, 'ALTER TABLE `subscriptions` DROP COLUMN `expires_at`', 'SELECT 1');
PREPARE stmt FROM @sql_drop_exp;
EXECUTE stmt;
DEALLOCATE PREPARE stmt;

SET @exist_sub_starts_at := (
    SELECT COUNT(*) FROM information_schema.COLUMNS 
    WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'subscriptions' AND COLUMN_NAME = 'starts_at'
);
SET @sql_drop_start := IF(@exist_sub_starts_at > 0, 'ALTER TABLE `subscriptions` DROP COLUMN `starts_at`', 'SELECT 1');
PREPARE stmt FROM @sql_drop_start;
EXECUTE stmt;
DEALLOCATE PREPARE stmt;

SET @exist_sub_user_id := (
    SELECT COUNT(*) FROM information_schema.COLUMNS 
    WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'subscriptions' AND COLUMN_NAME = 'user_id'
);
SET @sql_drop_uid := IF(@exist_sub_user_id > 0, 'ALTER TABLE `subscriptions` DROP COLUMN `user_id`', 'SELECT 1');
PREPARE stmt FROM @sql_drop_uid;
EXECUTE stmt;
DEALLOCATE PREPARE stmt;

-- Drop added columns from plans
SET @exist_plans_features := (
    SELECT COUNT(*) FROM information_schema.COLUMNS 
    WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'plans' AND COLUMN_NAME = 'features'
);
SET @sql_drop_feat := IF(@exist_plans_features > 0, 'ALTER TABLE `plans` DROP COLUMN `features`', 'SELECT 1');
PREPARE stmt FROM @sql_drop_feat;
EXECUTE stmt;
DEALLOCATE PREPARE stmt;

SET @exist_plans_devices := (
    SELECT COUNT(*) FROM information_schema.COLUMNS 
    WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'plans' AND COLUMN_NAME = 'max_devices'
);
SET @sql_drop_dev := IF(@exist_plans_devices > 0, 'ALTER TABLE `plans` DROP COLUMN `max_devices`', 'SELECT 1');
PREPARE stmt FROM @sql_drop_dev;
EXECUTE stmt;
DEALLOCATE PREPARE stmt;

SET @exist_plans_concurrent := (
    SELECT COUNT(*) FROM information_schema.COLUMNS 
    WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'plans' AND COLUMN_NAME = 'concurrent_sessions'
);
SET @sql_drop_conc := IF(@exist_plans_concurrent > 0, 'ALTER TABLE `plans` DROP COLUMN `concurrent_sessions`', 'SELECT 1');
PREPARE stmt FROM @sql_drop_conc;
EXECUTE stmt;
DEALLOCATE PREPARE stmt;

SET @exist_plans_dur_unit := (
    SELECT COUNT(*) FROM information_schema.COLUMNS 
    WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'plans' AND COLUMN_NAME = 'duration_unit'
);
SET @sql_drop_unit := IF(@exist_plans_dur_unit > 0, 'ALTER TABLE `plans` DROP COLUMN `duration_unit`', 'SELECT 1');
PREPARE stmt FROM @sql_drop_unit;
EXECUTE stmt;
DEALLOCATE PREPARE stmt;

SET @exist_plans_dur_val := (
    SELECT COUNT(*) FROM information_schema.COLUMNS 
    WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'plans' AND COLUMN_NAME = 'duration_value'
);
SET @sql_drop_val := IF(@exist_plans_dur_val > 0, 'ALTER TABLE `plans` DROP COLUMN `duration_value`', 'SELECT 1');
PREPARE stmt FROM @sql_drop_val;
EXECUTE stmt;
DEALLOCATE PREPARE stmt;

SET @exist_plans_slug := (
    SELECT COUNT(*) FROM information_schema.COLUMNS 
    WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'plans' AND COLUMN_NAME = 'slug'
);
SET @sql_drop_slug := IF(@exist_plans_slug > 0, 'ALTER TABLE `plans` DROP COLUMN `slug`', 'SELECT 1');
PREPARE stmt FROM @sql_drop_slug;
EXECUTE stmt;
DEALLOCATE PREPARE stmt;

SET FOREIGN_KEY_CHECKS = 1;
