<?php
require_once __DIR__ . '/../backend/config/database.php';
$tables = $pdo->query('SHOW TABLES')->fetchAll(PDO::FETCH_COLUMN);
foreach ($tables as $t) {
    echo "TABLE: " . $t . "\n";
    $cols = $pdo->query("DESCRIBE `$t`")->fetchAll(PDO::FETCH_ASSOC);
    foreach ($cols as $c) {
        echo "  " . $c['Field'] . " (" . $c['Type'] . ") " . ($c['Null'] === 'YES' ? 'NULL' : 'NOT NULL') . "\n";
    }
}
