<?php
header('Content-Type: text/plain');
require_once __DIR__ . '/includes/db.php';
$pdo = admin_db();
$tables = ['plans', 'subscriptions', 'devices', 'users'];
foreach ($tables as $t) {
    echo "=== DATA IN $t ===\n";
    $rows = $pdo->query("SELECT * FROM `$t` LIMIT 10")->fetchAll(PDO::FETCH_ASSOC);
    print_r($rows);
}

