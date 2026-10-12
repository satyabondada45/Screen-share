<?php
$_SERVER['REQUEST_METHOD'] = 'GET';
session_start();
$_SESSION['admin_logged_in'] = true;
$_SESSION['admin_user_id'] = 1;

require_once 'includes/db.php';
$pdo = admin_db();

function dump_table($pdo, $table) {
    if (!admin_table_exists($pdo, $table)) {
        echo "$table missing\n";
        return;
    }
    echo "--- $table ---\n";
    $stmt = $pdo->query("SHOW CREATE TABLE `$table`");
    $row = $stmt->fetch(PDO::FETCH_ASSOC);
    echo $row['Create Table'] . "\n\n";
}

dump_table($pdo, 'users');
dump_table($pdo, 'devices');
dump_table($pdo, 'plans');
dump_table($pdo, 'subscriptions');
