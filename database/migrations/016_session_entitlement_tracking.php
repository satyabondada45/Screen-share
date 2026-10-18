<?php
require_once dirname(__DIR__, 2) . '/backend/config/database.php';

function sessionEntitlementColumnExists(PDO $pdo, string $column): bool
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

function sessionEntitlementIndexExists(PDO $pdo, string $index): bool
{
    $stmt = $pdo->prepare("
        SELECT 1
        FROM information_schema.STATISTICS
        WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'sessions' AND INDEX_NAME = ?
        LIMIT 1
    ");
    $stmt->execute([$index]);
    return (bool)$stmt->fetchColumn();
}

try {
    foreach ([
        'requester_user_id' => 'INT NULL',
        'connection_request_id' => 'INT NULL',
        'last_activity_at' => 'DATETIME NULL',
        'requester_plan_slug' => 'VARCHAR(50) NULL',
        'host_plan_slug' => 'VARCHAR(50) NULL'
    ] as $column => $definition) {
        if (!sessionEntitlementColumnExists($pdo, $column)) {
            $pdo->exec("ALTER TABLE `sessions` ADD COLUMN `$column` $definition");
        }
    }

    if (!sessionEntitlementIndexExists($pdo, 'idx_sessions_requester_status')) {
        $pdo->exec("ALTER TABLE `sessions` ADD INDEX `idx_sessions_requester_status` (`requester_user_id`, `status`)");
    }
    if (!sessionEntitlementIndexExists($pdo, 'uq_sessions_connection_request')) {
        $pdo->exec("ALTER TABLE `sessions` ADD UNIQUE INDEX `uq_sessions_connection_request` (`connection_request_id`)");
    }

    echo "Migration 016 completed. New session rows will track both account participants.\n";
} catch (Throwable $error) {
    error_log('Migration 016 failed: ' . $error->getMessage());
    fwrite(STDERR, "Migration 016 failed; no session records were changed.\n");
    exit(1);
}
