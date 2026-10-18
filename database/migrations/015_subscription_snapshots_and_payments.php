<?php
require_once dirname(__DIR__, 2) . '/backend/config/database.php';

function subscriptionColumnExists(PDO $pdo, string $table, string $column): bool
{
    $stmt = $pdo->prepare("
        SELECT 1
        FROM information_schema.COLUMNS
        WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ? AND COLUMN_NAME = ?
        LIMIT 1
    ");
    $stmt->execute([$table, $column]);
    return (bool)$stmt->fetchColumn();
}

function subscriptionIndexExists(PDO $pdo, string $table, string $index): bool
{
    $stmt = $pdo->prepare("
        SELECT 1
        FROM information_schema.STATISTICS
        WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ? AND INDEX_NAME = ?
        LIMIT 1
    ");
    $stmt->execute([$table, $index]);
    return (bool)$stmt->fetchColumn();
}

function subscriptionTableExists(PDO $pdo, string $table): bool
{
    $stmt = $pdo->prepare("
        SELECT 1
        FROM information_schema.TABLES
        WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ?
        LIMIT 1
    ");
    $stmt->execute([$table]);
    return (bool)$stmt->fetchColumn();
}

try {
    foreach ([
        'plan_snapshot' => 'LONGTEXT NULL',
        'activation_source' => "VARCHAR(32) NOT NULL DEFAULT 'legacy'",
        'activated_by' => 'INT NULL',
        'activation_reason' => 'VARCHAR(500) NULL'
    ] as $column => $definition) {
        if (!subscriptionColumnExists($pdo, 'subscriptions', $column)) {
            $pdo->exec("ALTER TABLE `subscriptions` ADD COLUMN `$column` $definition");
        }
    }

    if (subscriptionTableExists($pdo, 'payments')) {
        foreach ([
            'gateway_order_id' => 'VARCHAR(100) NULL',
            'gateway_payment_id' => 'VARCHAR(100) NULL',
            'currency' => "CHAR(3) NOT NULL DEFAULT 'INR'",
            'plan_id' => 'INT NULL',
            'plan_snapshot' => 'LONGTEXT NULL',
            'signature_verified_at' => 'DATETIME NULL'
        ] as $column => $definition) {
            if (!subscriptionColumnExists($pdo, 'payments', $column)) {
                $pdo->exec("ALTER TABLE `payments` ADD COLUMN `$column` $definition");
            }
        }

        foreach ([
            'uq_payments_gateway_order_id' => 'gateway_order_id',
            'uq_payments_gateway_payment_id' => 'gateway_payment_id'
        ] as $index => $column) {
            if (!subscriptionIndexExists($pdo, 'payments', $index)) {
                $pdo->exec("ALTER TABLE `payments` ADD UNIQUE INDEX `$index` (`$column`)");
            }
        }
    }

    $legacySubscriptions = $pdo->query("
        SELECT s.id, p.name, p.slug, p.price, p.duration_days, p.duration_value,
               p.duration_unit, p.concurrent_sessions, p.max_devices, p.features
        FROM `subscriptions` s
        JOIN `plans` p ON p.id = s.plan_id
        WHERE s.plan_snapshot IS NULL
    ")->fetchAll(PDO::FETCH_ASSOC);
    $snapshotStmt = $pdo->prepare("
        UPDATE `subscriptions`
        SET `plan_snapshot` = ?
        WHERE `id` = ? AND `plan_snapshot` IS NULL
    ");
    foreach ($legacySubscriptions as $subscription) {
        $snapshot = [
            'name' => (string)$subscription['name'],
            'slug' => (string)$subscription['slug'],
            'price' => (string)$subscription['price'],
            'duration_days' => (int)$subscription['duration_days'],
            'duration_value' => (int)$subscription['duration_value'],
            'duration_unit' => (string)$subscription['duration_unit'],
            'concurrent_sessions' => (int)$subscription['concurrent_sessions'],
            'max_devices' => (int)$subscription['max_devices'],
            'features' => json_decode((string)$subscription['features'], true) ?: []
        ];
        $snapshotStmt->execute([json_encode($snapshot, JSON_THROW_ON_ERROR), $subscription['id']]);
    }

    if (subscriptionTableExists($pdo, 'payments')) {
        $pdo->exec("
            UPDATE `payments` pay
            JOIN `subscriptions` sub ON sub.id = pay.subscription_id
            SET pay.plan_id = sub.plan_id,
                pay.plan_snapshot = sub.plan_snapshot
            WHERE pay.plan_id IS NULL
        ");
    }

    $welcome = $pdo->prepare("
        UPDATE `plans`
        SET `name` = 'Welcome Plan offered by Friends Software Solutions'
        WHERE `slug` = 'welcome'
          AND `name` <> 'Welcome Plan offered by Friends Software Solutions'
    ");
    $welcome->execute();

    $monthly = $pdo->prepare("
        UPDATE `plans`
        SET `duration_value` = 1, `duration_unit` = 'months', `duration_days` = 30
        WHERE `slug` = 'monthly' AND `price` = 100
          AND `duration_value` = 30 AND `duration_unit` = 'days'
    ");
    $monthly->execute();

    $yearly = $pdo->prepare("
        UPDATE `plans`
        SET `duration_value` = 1, `duration_unit` = 'years', `duration_days` = 365
        WHERE `slug` = 'yearly' AND `price` = 1100
          AND `duration_value` = 365 AND `duration_unit` = 'days'
    ");
    $yearly->execute();

    $pdo->exec("
        UPDATE `plans`
        SET `max_devices` = 100
        WHERE `slug` = 'free' AND `max_devices` = 1
    ");

    foreach (['welcome', 'free', 'monthly', 'yearly'] as $slug) {
        $planStmt = $pdo->prepare("SELECT `id`, `features` FROM `plans` WHERE `slug` = ? LIMIT 1");
        $planStmt->execute([$slug]);
        $plan = $planStmt->fetch(PDO::FETCH_ASSOC);
        if (!$plan) {
            continue;
        }
        $features = json_decode((string)$plan['features'], true);
        if (!is_array($features)) {
            $features = [];
        }
        foreach (['audio' => true, 'chat' => true, 'file_transfer' => true] as $feature => $enabled) {
            $features[$feature] ??= $enabled;
        }
        if ($slug === 'free') {
            $features['recording'] = false;
            $features['reverse_control'] = false;
            $features['max_quality'] = '720p';
            $features['concurrent_sessions'] = 1;
        }
        $pdo->prepare("UPDATE `plans` SET `features` = ? WHERE `id` = ?")
            ->execute([json_encode($features, JSON_THROW_ON_ERROR), $plan['id']]);
    }

    echo "Migration 015 completed. Existing subscription expiries were not changed.\n";
} catch (Throwable $error) {
    error_log('Migration 015 failed: ' . $error->getMessage());
    fwrite(STDERR, "Migration 015 failed; no subscription expiries were modified.\n");
    exit(1);
}
