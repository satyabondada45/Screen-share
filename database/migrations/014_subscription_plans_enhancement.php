<?php
/**
 * Safe, idempotent migration to enhance the plans and subscriptions tables
 * for DeskStream subscription management and entitlement enforcement.
 */
require_once dirname(__DIR__, 2) . '/backend/config/database.php';

echo "Running Migration 014: Subscription Plans Enhancement...\n";

function columnExists(PDO $pdo, string $table, string $column): bool {
    $stmt = $pdo->prepare("
        SELECT 1 FROM information_schema.COLUMNS 
        WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ? AND COLUMN_NAME = ?
        LIMIT 1
    ");
    $stmt->execute([$table, $column]);
    return (bool) $stmt->fetchColumn();
}

try {
    // 1. Add missing columns to `plans`
    if (!columnExists($pdo, 'plans', 'description')) {
        echo "Adding `description` column to `plans`...\n";
        $pdo->exec("ALTER TABLE `plans` ADD COLUMN `description` TEXT NULL AFTER `name`");
    }

    if (!columnExists($pdo, 'plans', 'slug')) {
        echo "Adding `slug` column to `plans`...\n";
        $pdo->exec("ALTER TABLE `plans` ADD COLUMN `slug` VARCHAR(50) NULL UNIQUE AFTER `description`");
    }

    if (!columnExists($pdo, 'plans', 'status')) {
        echo "Adding `status` column to `plans`...\n";
        $pdo->exec("ALTER TABLE `plans` ADD COLUMN `status` ENUM('active', 'inactive') NOT NULL DEFAULT 'active' AFTER `price`");
    }

    if (!columnExists($pdo, 'plans', 'duration_value')) {
        echo "Adding `duration_value` column to `plans`...\n";
        $pdo->exec("ALTER TABLE `plans` ADD COLUMN `duration_value` INT NOT NULL DEFAULT 30 AFTER `duration_days`");
    }

    if (!columnExists($pdo, 'plans', 'duration_unit')) {
        echo "Adding `duration_unit` column to `plans`...\n";
        $pdo->exec("ALTER TABLE `plans` ADD COLUMN `duration_unit` ENUM('days', 'months', 'years', 'unlimited') NOT NULL DEFAULT 'days' AFTER `duration_value`");
    }

    // 2. Allow NULL for expires_at in `subscriptions` for unlimited plans
    echo "Modifying `expires_at` in `subscriptions` to allow NULL for unlimited plans...\n";
    $pdo->exec("ALTER TABLE `subscriptions` MODIFY COLUMN `expires_at` DATETIME NULL DEFAULT NULL");

    // 3. Define and seed the 4 canonical plans idempotently
    $defaultPlans = [
        [
            'name' => 'Welcome Plan offered by Friends Software Solutions',
            'slug' => 'welcome',
            'description' => 'Welcome Plan offered by Friends Software Solutions. New eligible registrations receive the configured duration.',
            'price' => 0.00,
            'duration_days' => 183,
            'duration_value' => 6,
            'duration_unit' => 'months',
            'max_devices' => 100,
            'concurrent_sessions' => 10,
            'status' => 'active',
            'features' => json_encode([
                'recording' => true,
                'reverse_control' => true,
                'max_quality' => 'full',
                'concurrent_sessions' => 10,
                'audio' => true,
                'file_transfer' => true
            ])
        ],
        [
            'name' => 'Free Plan',
            'slug' => 'free',
            'description' => 'Free Plan with one concurrent remote session, 720p streaming, chat, file transfer, and bidirectional audio.',
            'price' => 0.00,
            'duration_days' => 0,
            'duration_value' => 0,
            'duration_unit' => 'unlimited',
            'max_devices' => 100,
            'concurrent_sessions' => 1,
            'status' => 'active',
            'features' => json_encode([
                'recording' => false,
                'reverse_control' => false,
                'max_quality' => '720p',
                'concurrent_sessions' => 1,
                'audio' => true,
                'file_transfer' => true
            ])
        ],
        [
            'name' => 'Monthly Plan',
            'slug' => 'monthly',
            'description' => 'Full access for one calendar month including session recording and reverse control.',
            'price' => 100.00,
            'duration_days' => 30,
            'duration_value' => 1,
            'duration_unit' => 'months',
            'max_devices' => 5,
            'concurrent_sessions' => 5,
            'status' => 'active',
            'features' => json_encode([
                'recording' => true,
                'reverse_control' => true,
                'max_quality' => 'full',
                'concurrent_sessions' => 5,
                'audio' => true,
                'file_transfer' => true
            ])
        ],
        [
            'name' => 'Yearly Plan',
            'slug' => 'yearly',
            'description' => 'Full access for 12 calendar months including session recording and reverse control.',
            'price' => 1100.00,
            'duration_days' => 365,
            'duration_value' => 1,
            'duration_unit' => 'years',
            'max_devices' => 10,
            'concurrent_sessions' => 10,
            'status' => 'active',
            'features' => json_encode([
                'recording' => true,
                'reverse_control' => true,
                'max_quality' => 'full',
                'concurrent_sessions' => 10,
                'audio' => true,
                'file_transfer' => true
            ])
        ]
    ];

    foreach ($defaultPlans as $plan) {
        $stmt = $pdo->prepare("SELECT id FROM `plans` WHERE `slug` = ? OR `name` = ? LIMIT 1");
        $stmt->execute([$plan['slug'], $plan['name']]);
        $existingId = $stmt->fetchColumn();

        if ($existingId) {
            echo "Updating existing plan '{$plan['name']}' (ID: $existingId)...\n";
            $update = $pdo->prepare("
                UPDATE `plans`
                SET `slug` = COALESCE(NULLIF(`slug`, ''), ?),
                    `description` = COALESCE(NULLIF(`description`, ''), ?)
                WHERE `id` = ?
            ");
            $update->execute([
                $plan['slug'],
                $plan['description'],
                $existingId
            ]);
        } else {
            echo "Inserting new plan '{$plan['name']}'...\n";
            $insert = $pdo->prepare("
                INSERT INTO `plans` (
                    `name`, `slug`, `description`, `price`, `duration_days`, 
                    `duration_value`, `duration_unit`, `max_devices`, `concurrent_sessions`, 
                    `status`, `features`, `created_at`
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NOW())
            ");
            $insert->execute([
                $plan['name'],
                $plan['slug'],
                $plan['description'],
                $plan['price'],
                $plan['duration_days'],
                $plan['duration_value'],
                $plan['duration_unit'],
                $plan['max_devices'],
                $plan['concurrent_sessions'],
                $plan['status'],
                $plan['features']
            ]);
        }
    }

    echo "Migration 014 completed successfully!\n";

} catch (Throwable $e) {
    if ($pdo->inTransaction()) {
        $pdo->rollBack();
    }
    echo "Migration failed: " . $e->getMessage() . "\n";
    exit(1);
}
