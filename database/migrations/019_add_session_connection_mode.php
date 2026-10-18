<?php
// database/migrations/019_add_session_connection_mode.php
// Adds `connection_mode` column to `sessions` table if not already present.

require_once dirname(__DIR__, 2) . '/backend/config/database.php';

function sessionColumnExists(PDO $pdo, string $column): bool
{
    $stmt = $pdo->prepare("
        SELECT 1
        FROM information_schema.COLUMNS
        WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'sessions' AND COLUMN_NAME = ?
        LIMIT 1
    ");
    $stmt->execute([$column]);
    return (bool)$stmt->fetchColumn();
}

try {
    if (!sessionColumnExists($pdo, 'connection_mode')) {
        $pdo->exec("ALTER TABLE `sessions` ADD COLUMN `connection_mode` VARCHAR(20) DEFAULT 'desktop' AFTER `host_plan_slug`");
        echo "Migration 019: Added `connection_mode` column to `sessions` table.\n";
    } else {
        echo "Migration 019: `connection_mode` column already exists in `sessions` table.\n";
    }
} catch (Throwable $e) {
    fwrite(STDERR, "Migration 019 failed: " . $e->getMessage() . "\n");
    exit(1);
}
