<?php
// Migration: Add unique_user_id to users table
require_once __DIR__ . '/../../backend/config/database.php';

try {
    // Check if column exists
    $stmt = $pdo->query("SHOW COLUMNS FROM users LIKE 'unique_user_id'");
    if ($stmt->rowCount() == 0) {
        $pdo->exec("ALTER TABLE users ADD COLUMN unique_user_id VARCHAR(32) NULL UNIQUE AFTER id");
        echo "Added unique_user_id column to users table\n";
    } else {
        echo "unique_user_id column already exists\n";
    }

    // Generate unique IDs for users that don't have one
    $stmt = $pdo->query("SELECT id FROM users WHERE unique_user_id IS NULL");
    $users = $stmt->fetchAll();
    foreach ($users as $user) {
        do {
            $candidate = (string)random_int(100000000, 999999999);
            $check = $pdo->prepare("SELECT id FROM users WHERE unique_user_id = ?");
            $check->execute([$candidate]);
        } while ($check->fetch());
        
        $update = $pdo->prepare("UPDATE users SET unique_user_id = ? WHERE id = ?");
        $update->execute([$candidate, $user['id']]);
        echo "Assigned unique_user_id {$candidate} to user id={$user['id']}\n";
    }

    echo "Migration complete.\n";
} catch (PDOException $e) {
    echo "ERROR: " . $e->getMessage() . "\n";
}
