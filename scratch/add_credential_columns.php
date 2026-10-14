<?php
require_once __DIR__ . '/../backend/config/database.php';

try {
    $cols = $pdo->query("SHOW COLUMNS FROM connection_requests LIKE 'session_credential'")->fetchAll();
    if (empty($cols)) {
        $pdo->exec("ALTER TABLE connection_requests ADD COLUMN session_credential VARCHAR(64) NULL AFTER status");
        echo "Added column session_credential\n";
    } else {
        echo "session_credential already exists\n";
    }

    $cols2 = $pdo->query("SHOW COLUMNS FROM connection_requests LIKE 'session_credential_expires_at'")->fetchAll();
    if (empty($cols2)) {
        $pdo->exec("ALTER TABLE connection_requests ADD COLUMN session_credential_expires_at DATETIME NULL AFTER session_credential");
        echo "Added column session_credential_expires_at\n";
    } else {
        echo "session_credential_expires_at already exists\n";
    }

    $cols3 = $pdo->query("SHOW COLUMNS FROM connection_requests LIKE 'session_credential_used_at'")->fetchAll();
    if (empty($cols3)) {
        $pdo->exec("ALTER TABLE connection_requests ADD COLUMN session_credential_used_at DATETIME NULL AFTER session_credential_expires_at");
        echo "Added column session_credential_used_at\n";
    } else {
        echo "session_credential_used_at already exists\n";
    }

    echo "Table schema migration successful.\n";
} catch (Exception $e) {
    echo "Error: " . $e->getMessage() . "\n";
}
