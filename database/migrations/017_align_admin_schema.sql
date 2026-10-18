-- =============================================================================
-- Migration: 017_align_admin_schema.sql
-- Description: Safe, non-destructive schema alignment for DeskStream Admin Portal.
-- Compatible with: MySQL 5.7+, MySQL 8.0+, MariaDB 10.3+ (Hostinger environment)
--
-- Safety Guarantees:
-- 1. Non-destructive: Does NOT drop or rename any existing columns or tables.
-- 2. Idempotent: Uses a helper procedure to inspect information_schema before adding.
-- 3. Provides sensible defaults for all newly added non-null columns.
-- =============================================================================

DELIMITER $$

DROP PROCEDURE IF EXISTS `deskstream_add_column_if_missing`$$
CREATE PROCEDURE `deskstream_add_column_if_missing`(
    IN in_table VARCHAR(64),
    IN in_column VARCHAR(64),
    IN in_definition TEXT
)
BEGIN
    DECLARE col_exists INT DEFAULT 0;
    
    SELECT COUNT(*) INTO col_exists
    FROM information_schema.COLUMNS
    WHERE TABLE_SCHEMA = DATABASE()
      AND TABLE_NAME = in_table
      AND COLUMN_NAME = in_column;

    IF col_exists = 0 THEN
        SET @sql = CONCAT('ALTER TABLE `', in_table, '` ADD COLUMN `', in_column, '` ', in_definition);
        PREPARE stmt FROM @sql;
        EXECUTE stmt;
        DEALLOCATE PREPARE stmt;
    END IF;
END$$

DROP PROCEDURE IF EXISTS `deskstream_add_index_if_missing`$$
CREATE PROCEDURE `deskstream_add_index_if_missing`(
    IN in_table VARCHAR(64),
    IN in_index VARCHAR(64),
    IN in_columns VARCHAR(255)
)
BEGIN
    DECLARE idx_exists INT DEFAULT 0;

    SELECT COUNT(*) INTO idx_exists
    FROM information_schema.STATISTICS
    WHERE TABLE_SCHEMA = DATABASE()
      AND TABLE_NAME = in_table
      AND INDEX_NAME = in_index;

    IF idx_exists = 0 THEN
        SET @sql = CONCAT('ALTER TABLE `', in_table, '` ADD INDEX `', in_index, '` (', in_columns, ')');
        PREPARE stmt FROM @sql;
        EXECUTE stmt;
        DEALLOCATE PREPARE stmt;
    END IF;
END$$

DELIMITER ;

-- =============================================================================
-- 1. USERS TABLE ALIGNMENT
-- =============================================================================
CALL deskstream_add_column_if_missing('users', 'role', "VARCHAR(32) NOT NULL DEFAULT 'user'");
CALL deskstream_add_column_if_missing('users', 'status', "VARCHAR(32) NOT NULL DEFAULT 'active'");
CALL deskstream_add_column_if_missing('users', 'unique_user_id', "VARCHAR(32) NULL");

-- Add index on role, status, unique_user_id if missing
CALL deskstream_add_index_if_missing('users', 'idx_users_role', '`role`');
CALL deskstream_add_index_if_missing('users', 'idx_users_status', '`status`');
CALL deskstream_add_index_if_missing('users', 'idx_users_unique_user_id', '`unique_user_id`');

-- Ensure the primary root user (ID = 1) has admin role
UPDATE `users` SET `role` = 'admin' WHERE `id` = 1 AND `role` != 'admin';

-- =============================================================================
-- 2. PLANS TABLE ALIGNMENT
-- =============================================================================
CALL deskstream_add_column_if_missing('plans', 'slug', "VARCHAR(64) NULL");
CALL deskstream_add_column_if_missing('plans', 'duration_value', "INT NOT NULL DEFAULT 30");
CALL deskstream_add_column_if_missing('plans', 'duration_unit', "VARCHAR(16) NOT NULL DEFAULT 'days'");
CALL deskstream_add_column_if_missing('plans', 'max_devices', "INT NOT NULL DEFAULT 5");
CALL deskstream_add_column_if_missing('plans', 'concurrent_sessions', "INT NOT NULL DEFAULT 1");
CALL deskstream_add_column_if_missing('plans', 'features', "TEXT NULL");

-- Populate duration_value from duration_days if duration_days exists
UPDATE `plans` 
SET `duration_value` = `duration_days`, 
    `duration_unit` = 'days' 
WHERE `duration_days` IS NOT NULL 
  AND (`duration_value` IS NULL OR `duration_value` = 0 OR `duration_value` = 30);

-- Populate slug from name if missing
UPDATE `plans` 
SET `slug` = LOWER(REPLACE(TRIM(`name`), ' ', '-')) 
WHERE `slug` IS NULL OR `slug` = '';

CALL deskstream_add_index_if_missing('plans', 'idx_plans_slug', '`slug`');

-- =============================================================================
-- 3. SUBSCRIPTIONS TABLE ALIGNMENT
-- =============================================================================
CALL deskstream_add_column_if_missing('subscriptions', 'user_id', "INT NULL");
CALL deskstream_add_column_if_missing('subscriptions', 'starts_at', "DATETIME NULL");
CALL deskstream_add_column_if_missing('subscriptions', 'expires_at', "DATETIME NULL");
CALL deskstream_add_column_if_missing('subscriptions', 'max_devices', "INT NOT NULL DEFAULT 5");

-- Populate starts_at and expires_at from start_date and expiry_date if present
UPDATE `subscriptions` 
SET `starts_at` = `start_date` 
WHERE `starts_at` IS NULL AND `start_date` IS NOT NULL;

UPDATE `subscriptions` 
SET `expires_at` = `expiry_date` 
WHERE `expires_at` IS NULL AND `expiry_date` IS NOT NULL;

-- Backfill user_id from devices where possible
UPDATE `subscriptions` s
JOIN `devices` d ON (s.device_id = d.system_id OR s.device_id = d.device_uid OR s.device_id = d.id)
SET s.user_id = d.user_id
WHERE s.user_id IS NULL AND d.user_id IS NOT NULL;

CALL deskstream_add_index_if_missing('subscriptions', 'idx_subscriptions_user_id', '`user_id`');
CALL deskstream_add_index_if_missing('subscriptions', 'idx_subscriptions_expires_at', '`expires_at`');

-- =============================================================================
-- 4. PAYMENTS TABLE ALIGNMENT
-- =============================================================================
CALL deskstream_add_column_if_missing('payments', 'user_id', "INT NULL");
CALL deskstream_add_column_if_missing('payments', 'transaction_id', "VARCHAR(128) NULL");

-- Populate transaction_id from payment_reference or gateway_payment_id if missing
UPDATE `payments` 
SET `transaction_id` = COALESCE(`payment_reference`, `gateway_payment_id`)
WHERE `transaction_id` IS NULL AND (`payment_reference` IS NOT NULL OR `gateway_payment_id` IS NOT NULL);

-- Backfill user_id from devices where possible
UPDATE `payments` p
JOIN `devices` d ON (p.device_id = d.system_id OR p.device_id = d.device_uid OR p.device_id = d.id)
SET p.user_id = d.user_id
WHERE p.user_id IS NULL AND d.user_id IS NOT NULL;

CALL deskstream_add_index_if_missing('payments', 'idx_payments_user_id', '`user_id`');
CALL deskstream_add_index_if_missing('payments', 'idx_payments_transaction_id', '`transaction_id`');

-- =============================================================================
-- CLEANUP
-- =============================================================================
DROP PROCEDURE IF EXISTS `deskstream_add_column_if_missing`;
DROP PROCEDURE IF EXISTS `deskstream_add_index_if_missing`;

-- Verification:
SELECT 'Migration 017_align_admin_schema completed successfully.' AS status;
