<?php
require_once __DIR__ . '/../backend/config/database.php';

echo "=== USERS TABLE ===\n";
$stmt = $pdo->query("DESCRIBE users");
while ($r = $stmt->fetch()) {
    echo implode("\t", $r) . "\n";
}

echo "\n=== USERS DATA ===\n";
$stmt = $pdo->query("SELECT * FROM users");
while ($r = $stmt->fetch()) {
    echo implode("\t", $r) . "\n";
}

echo "\n=== DEVICES TABLE ===\n";
$stmt = $pdo->query("DESCRIBE devices");
while ($r = $stmt->fetch()) {
    echo implode("\t", $r) . "\n";
}

echo "\n=== DEVICES DATA ===\n";
$stmt = $pdo->query("SELECT * FROM devices");
while ($r = $stmt->fetch()) {
    echo implode("\t", $r) . "\n";
}
