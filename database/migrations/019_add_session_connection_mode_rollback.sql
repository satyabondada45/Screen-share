-- database/migrations/019_add_session_connection_mode_rollback.sql
-- Rollback for migration 019: removes `connection_mode` from `sessions` table.

ALTER TABLE `sessions` DROP COLUMN IF EXISTS `connection_mode`;
