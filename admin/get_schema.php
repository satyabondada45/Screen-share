<?php
require 'includes/db.php';
$pdo = admin_db();
foreach(['users', 'devices', 'plans', 'subscriptions', 'payments'] as $t) {
    echo "TABLE $t\n";
    foreach($pdo->query("DESCRIBE `$t`")->fetchAll(PDO::FETCH_ASSOC) as $r) {
        echo $r['Field'].' '.$r['Type']."\n";
    }
}
