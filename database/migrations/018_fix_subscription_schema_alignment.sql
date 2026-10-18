-- =====================================================================
-- Migration 018: Align Subscription and Plan Schema for DeskStream
-- Database: u588459601_deskstream
-- =====================================================================
-- PURPOSE:
-- 1. Adds missing canonical columns to `plans`:
--    - `slug` VARCHAR(50) NULL (Used by SubscriptionService to identify plans: welcome, free, monthly, yearly)
--    - `duration_value` INT NOT NULL DEFAULT 1
--    - `duration_unit` VARCHAR(20) NOT NULL DEFAULT 'months'
--    - `concurrent_sessions` INT NOT NULL DEFAULT 1
--    - `max_devices` INT NOT NULL DEFAULT 10
--    - `features` LONGTEXT NULL
--
-- 2. Populates `slug` on existing rows in `plans` based on canonical plan names.
--
-- 3. Adds missing columns to `subscriptions`:
--    - `user_id` INT NULL (with index `idx_subscriptions_user_id`)
--    - `starts_at` DATETIME NULL
--    - `expires_at` DATETIME NULL
--
-- 4. Backfills `subscriptions.user_id` from `devices.user_id` using `device_id`.
-- 5. Backfills `subscriptions.starts_at` and `expires_at` from `start_date` and `expiry_date`.
--
-- BACKUP INSTRUCTION BEFORE APPLYING:
-- Run in MySQL CLI or phpMyAdmin export:
-- mysqldump -u <user> -p u588459601_deskstream plans subscriptions > backup_plans_subscriptions_$(date +%Y%m%d).sql
-- =====================================================================

SET FOREIGN_KEY_CHECKS = 0;

-- 1. Align `plans` table columns
SET @exist_plans_slug := (
    SELECT COUNT(*) FROM information_schema.COLUMNS 
    WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'plans' AND COLUMN_NAME = 'slug'
);
SET @sql_plans_slug := IF(@exist_plans_slug = 0, 'ALTER TABLE `plans` ADD COLUMN `slug` VARCHAR(50) NULL AFTER `name`', 'SELECT 1');
PREPARE stmt FROM @sql_plans_slug;
EXECUTE stmt;
DEALLOCATE PREPARE stmt;

SET @exist_plans_dur_val := (
    SELECT COUNT(*) FROM information_schema.COLUMNS 
    WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'plans' AND COLUMN_NAME = 'duration_value'
);
SET @sql_plans_dur_val := IF(@exist_plans_dur_val = 0, 'ALTER TABLE `plans` ADD COLUMN `duration_value` INT NOT NULL DEFAULT 1 AFTER `duration_days`', 'SELECT 1');
PREPARE stmt FROM @sql_plans_dur_val;
EXECUTE stmt;
DEALLOCATE PREPARE stmt;

SET @exist_plans_dur_unit := (
    SELECT COUNT(*) FROM information_schema.COLUMNS 
    WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'plans' AND COLUMN_NAME = 'duration_unit'
);
SET @sql_plans_dur_unit := IF(@exist_plans_dur_unit = 0, 'ALTER TABLE `plans` ADD COLUMN `duration_unit` VARCHAR(20) NOT NULL DEFAULT \'months\' AFTER `duration_value`', 'SELECT 1');
PREPARE stmt FROM @sql_plans_dur_unit;
EXECUTE stmt;
DEALLOCATE PREPARE stmt;

SET @exist_plans_concurrent := (
    SELECT COUNT(*) FROM information_schema.COLUMNS 
    WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'plans' AND COLUMN_NAME = 'concurrent_sessions'
);
SET @sql_plans_concurrent := IF(@exist_plans_concurrent = 0, 'ALTER TABLE `plans` ADD COLUMN `concurrent_sessions` INT NOT NULL DEFAULT 1 AFTER `price`', 'SELECT 1');
PREPARE stmt FROM @sql_plans_concurrent;
EXECUTE stmt;
DEALLOCATE PREPARE stmt;

SET @exist_plans_devices := (
    SELECT COUNT(*) FROM information_schema.COLUMNS 
    WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'plans' AND COLUMN_NAME = 'max_devices'
);
SET @sql_plans_devices := IF(@exist_plans_devices = 0, 'ALTER TABLE `plans` ADD COLUMN `max_devices` INT NOT NULL DEFAULT 100 AFTER `concurrent_sessions`', 'SELECT 1');
PREPARE stmt FROM @sql_plans_devices;
EXECUTE stmt;
DEALLOCATE PREPARE stmt;

SET @exist_plans_features := (
    SELECT COUNT(*) FROM information_schema.COLUMNS 
    WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'plans' AND COLUMN_NAME = 'features'
);
SET @sql_plans_features := IF(@exist_plans_features = 0, 'ALTER TABLE `plans` ADD COLUMN `features` LONGTEXT NULL AFTER `status`', 'SELECT 1');
PREPARE stmt FROM @sql_plans_features;
EXECUTE stmt;
DEALLOCATE PREPARE stmt;

-- Populate canonical plan slugs
UPDATE `plans` SET `slug` = 'welcome' WHERE (`slug` IS NULL OR `slug` = '') AND LOWER(`name`) LIKE '%welcome%';
UPDATE `plans` SET `slug` = 'free' WHERE (`slug` IS NULL OR `slug` = '') AND LOWER(`name`) LIKE '%free%';
UPDATE `plans` SET `slug` = 'monthly' WHERE (`slug` IS NULL OR `slug` = '') AND LOWER(`name`) LIKE '%monthly%';
UPDATE `plans` SET `slug` = 'yearly' WHERE (`slug` IS NULL OR `slug` = '') AND LOWER(`name`) LIKE '%yearly%';

-- Populate canonical metadata for standard plans if missing
UPDATE `plans` SET `duration_value` = 6, `duration_unit` = 'months', `duration_days` = 183, `max_devices` = 100, `concurrent_sessions` = 10 
WHERE `slug` = 'welcome';

UPDATE `plans` SET `duration_value` = 0, `duration_unit` = 'unlimited', `duration_days` = 0, `max_devices` = 100, `concurrent_sessions` = 1 
WHERE `slug` = 'free';

UPDATE `plans` SET `duration_value` = 1, `duration_unit` = 'months', `duration_days` = 30, `max_devices` = 100, `concurrent_sessions` = 10 
WHERE `slug` = 'monthly';

UPDATE `plans` SET `duration_value` = 1, `duration_unit` = 'years', `duration_days` = 365, `max_devices` = 100, `concurrent_sessions` = 10 
WHERE `slug` = 'yearly';

-- 2. Align `subscriptions` table columns
SET @exist_sub_user_id := (
    SELECT COUNT(*) FROM information_schema.COLUMNS 
    WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'subscriptions' AND COLUMN_NAME = 'user_id'
);
SET @sql_sub_user_id := IF(@exist_sub_user_id = 0, 'ALTER TABLE `subscriptions` ADD COLUMN `user_id` INT NULL AFTER `id`', 'SELECT 1');
PREPARE stmt FROM @sql_sub_user_id;
EXECUTE stmt;
DEALLOCATE PREPARE stmt;

SET @exist_sub_idx_user := (
    SELECT COUNT(*) FROM information_schema.STATISTICS 
    WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'subscriptions' AND INDEX_NAME = 'idx_subscriptions_user_id'
);
SET @sql_sub_idx_user := IF(@exist_sub_idx_user = 0, 'ALTER TABLE `subscriptions` ADD INDEX `idx_subscriptions_user_id` (`user_id`)', 'SELECT 1');
PREPARE stmt FROM @sql_sub_idx_user;
EXECUTE stmt;
DEALLOCATE PREPARE stmt;

SET @exist_sub_starts_at := (
    SELECT COUNT(*) FROM information_schema.COLUMNS 
    WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'subscriptions' AND COLUMN_NAME = 'starts_at'
);
SET @sql_sub_starts_at := IF(@exist_sub_starts_at = 0, 'ALTER TABLE `subscriptions` ADD COLUMN `starts_at` DATETIME NULL AFTER `plan_name`', 'SELECT 1');
PREPARE stmt FROM @sql_sub_starts_at;
EXECUTE stmt;
DEALLOCATE PREPARE stmt;

SET @exist_sub_expires_at := (
    SELECT COUNT(*) FROM information_schema.COLUMNS 
    WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'subscriptions' AND COLUMN_NAME = 'expires_at'
);
SET @sql_sub_expires_at := IF(@exist_sub_expires_at = 0, 'ALTER TABLE `subscriptions` ADD COLUMN `expires_at` DATETIME NULL AFTER `expiry_date`', 'SELECT 1');
PREPARE stmt FROM @sql_sub_expires_at;
EXECUTE stmt;
DEALLOCATE PREPARE stmt;

-- 3. Data Backfills
-- Backfill user_id from devices where possible
UPDATE `subscriptions` s
JOIN `devices` d ON (s.device_id = d.system_id OR s.device_id = d.device_uid OR s.device_id = CAST(d.id AS CHAR))
SET s.user_id = d.user_id
WHERE s.user_id IS NULL AND d.user_id IS NOT NULL;

-- Backfill starts_at and expires_at from legacy date columns
UPDATE `subscriptions` SET `starts_at` = `start_date` WHERE `starts_at` IS NULL AND `start_date` IS NOT NULL;
UPDATE `subscriptions` SET `expires_at` = `expiry_date` WHERE `expires_at` IS NULL AND `expiry_date` IS NOT NULL;

SET FOREIGN_KEY_CHECKS = 1;
