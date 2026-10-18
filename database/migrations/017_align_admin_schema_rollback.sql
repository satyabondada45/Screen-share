-- =============================================================================
-- Migration Rollback: 017_align_admin_schema_rollback.sql
-- Description: Rollback script for 017_align_admin_schema.sql.
-- Compatible with: MySQL 5.7+, MySQL 8.0+, MariaDB 10.3+
--
-- CAUTION:
-- Running this script will remove the aligned columns from the database.
-- Before running, ensure you have taken a full database backup via phpMyAdmin or mysqldump.
-- =============================================================================

DELIMITER $$

DROP PROCEDURE IF EXISTS `deskstream_drop_column_if_exists`$$
CREATE PROCEDURE `deskstream_drop_column_if_exists`(
    IN in_table VARCHAR(64),
    IN in_column VARCHAR(64)
)
BEGIN
    DECLARE col_exists INT DEFAULT 0;
    
    SELECT COUNT(*) INTO col_exists
    FROM information_schema.COLUMNS
    WHERE TABLE_SCHEMA = DATABASE()
      AND TABLE_NAME = in_table
      AND COLUMN_NAME = in_column;

    IF col_exists > 0 THEN
        SET @sql = CONCAT('ALTER TABLE `', in_table, '` DROP COLUMN `', in_column, '`');
        PREPARE stmt FROM @sql;
        EXECUTE stmt;
        DEALLOCATE PREPARE stmt;
    END IF;
END$$

DROP PROCEDURE IF EXISTS `deskstream_drop_index_if_exists`$$
CREATE PROCEDURE `deskstream_drop_index_if_exists`(
    IN in_table VARCHAR(64),
    IN in_index VARCHAR(64)
)
BEGIN
    DECLARE idx_exists INT DEFAULT 0;

    SELECT COUNT(*) INTO idx_exists
    FROM information_schema.STATISTICS
    WHERE TABLE_SCHEMA = DATABASE()
      AND TABLE_NAME = in_table
      AND INDEX_NAME = in_index;

    IF idx_exists > 0 THEN
        SET @sql = CONCAT('ALTER TABLE `', in_table, '` DROP INDEX `', in_index, '`');
        PREPARE stmt FROM @sql;
        EXECUTE stmt;
        DEALLOCATE PREPARE stmt;
    END IF;
END$$

DELIMITER ;

-- =============================================================================
-- 1. ROLLBACK PAYMENTS TABLE
-- =============================================================================
CALL deskstream_drop_index_if_exists('payments', 'idx_payments_transaction_id');
CALL deskstream_drop_index_if_exists('payments', 'idx_payments_user_id');
CALL deskstream_drop_column_if_exists('payments', 'transaction_id');
CALL deskstream_drop_column_if_exists('payments', 'user_id');

-- =============================================================================
-- 2. ROLLBACK SUBSCRIPTIONS TABLE
-- =============================================================================
CALL deskstream_drop_index_if_exists('subscriptions', 'idx_subscriptions_expires_at');
CALL deskstream_drop_index_if_exists('subscriptions', 'idx_subscriptions_user_id');
CALL deskstream_drop_column_if_exists('subscriptions', 'max_devices');
CALL deskstream_drop_column_if_exists('subscriptions', 'expires_at');
CALL deskstream_drop_column_if_exists('subscriptions', 'starts_at');
CALL deskstream_drop_column_if_exists('subscriptions', 'user_id');

-- =============================================================================
-- 3. ROLLBACK PLANS TABLE
-- =============================================================================
CALL deskstream_drop_index_if_exists('plans', 'idx_plans_slug');
CALL deskstream_drop_column_if_exists('plans', 'features');
CALL deskstream_drop_column_if_exists('plans', 'concurrent_sessions');
CALL deskstream_drop_column_if_exists('plans', 'max_devices');
CALL deskstream_drop_column_if_exists('plans', 'duration_unit');
CALL deskstream_drop_column_if_exists('plans', 'duration_value');
CALL deskstream_drop_column_if_exists('plans', 'slug');

-- =============================================================================
-- 4. ROLLBACK USERS TABLE
-- =============================================================================
CALL deskstream_drop_index_if_exists('users', 'idx_users_role');
CALL deskstream_drop_index_if_exists('users', 'idx_users_status');
CALL deskstream_drop_column_if_exists('users', 'status');
CALL deskstream_drop_column_if_exists('users', 'role');

-- =============================================================================
-- CLEANUP
-- =============================================================================
DROP PROCEDURE IF EXISTS `deskstream_drop_column_if_exists`;
DROP PROCEDURE IF EXISTS `deskstream_drop_index_if_exists`;

SELECT 'Rollback of migration 017 completed successfully.' AS status;
