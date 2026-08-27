-- Migration: 012_system_identity_and_requests.sql
-- Establishes unique 9-digit system IDs, persistent machine UUIDs, and connection requests approval lifecycle.

USE `screen_share_db`;

-- 1. Ensure `devices` table contains `machine_identifier` and `system_id`
CREATE TABLE IF NOT EXISTS `devices` (
    `id` INT AUTO_INCREMENT PRIMARY KEY,
    `user_id` INT NULL,
    `device_uid` VARCHAR(32) NOT NULL UNIQUE,
    `system_id` VARCHAR(32) NULL UNIQUE,
    `machine_identifier` VARCHAR(64) NULL UNIQUE,
    `name` VARCHAR(255) NOT NULL,
    `os_type` VARCHAR(50) DEFAULT 'windows',
    `ip_address` VARCHAR(45) NULL,
    `is_online` TINYINT(1) DEFAULT 0,
    `last_seen_at` DATETIME NULL,
    `created_at` DATETIME DEFAULT CURRENT_TIMESTAMP,
    `updated_at` DATETIME DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX `idx_device_uid` (`device_uid`),
    INDEX `idx_system_id` (`system_id`),
    INDEX `idx_machine_id` (`machine_identifier`),
    INDEX `idx_is_online` (`is_online`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- 2. Add columns to `devices` if table already existed without them
DELIMITER //
CREATE PROCEDURE AddColumnsIfNotExist()
BEGIN
    IF NOT EXISTS (
        SELECT * FROM information_schema.COLUMNS 
        WHERE TABLE_SCHEMA = 'screen_share_db' AND TABLE_NAME = 'devices' AND COLUMN_NAME = 'system_id'
    ) THEN
        ALTER TABLE `devices` ADD COLUMN `system_id` VARCHAR(32) NULL UNIQUE AFTER `device_uid`;
    END IF;

    IF NOT EXISTS (
        SELECT * FROM information_schema.COLUMNS 
        WHERE TABLE_SCHEMA = 'screen_share_db' AND TABLE_NAME = 'devices' AND COLUMN_NAME = 'machine_identifier'
    ) THEN
        ALTER TABLE `devices` ADD COLUMN `machine_identifier` VARCHAR(64) NULL UNIQUE AFTER `system_id`;
    END IF;
END //
DELIMITER ;

CALL AddColumnsIfNotExist();
DROP PROCEDURE IF EXISTS AddColumnsIfNotExist;

-- Sync existing device_uid to system_id where null
UPDATE `devices` SET `system_id` = `device_uid` WHERE `system_id` IS NULL;

-- 3. Create `connection_requests` table
CREATE TABLE IF NOT EXISTS `connection_requests` (
    `id` INT AUTO_INCREMENT PRIMARY KEY,
    `request_token` VARCHAR(64) NOT NULL UNIQUE,
    `requester_user_id` INT NULL,
    `requester_system_id` VARCHAR(32) NOT NULL,
    `requester_name` VARCHAR(255) NULL,
    `target_user_id` INT NULL,
    `target_system_id` VARCHAR(32) NOT NULL,
    `status` ENUM('pending', 'accepted', 'rejected', 'expired', 'cancelled', 'connected', 'disconnected') DEFAULT 'pending',
    `created_at` DATETIME DEFAULT CURRENT_TIMESTAMP,
    `updated_at` DATETIME DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    `expires_at` DATETIME NOT NULL,
    INDEX `idx_target_status` (`target_system_id`, `status`),
    INDEX `idx_requester` (`requester_system_id`),
    INDEX `idx_token` (`request_token`),
    INDEX `idx_expires` (`expires_at`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
