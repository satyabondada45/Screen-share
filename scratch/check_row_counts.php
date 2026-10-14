<?php
require_once __DIR__ . '/../backend/config/database.php';
$tables = ['audit_logs', 'connection_requests', 'connections', 'devices', 'file_transfers', 'notifications', 'payments', 'plans', 'sessions', 'subscriptions', 'support_tickets', 'users'];
foreach ($tables as $t) {
    $count = $pdo->query("SELECT COUNT(*) FROM `$t`")->fetchColumn();
    echo str_pad($t, 22) . ": " . $count . " rows\n";
}
