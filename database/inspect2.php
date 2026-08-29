<?php
require_once __DIR__ . '/../backend/config/database.php';

echo "=== Device 238548998 ===\n";
$stmt = $pdo->prepare("SELECT * FROM devices WHERE system_id = ? OR device_uid = ?");
$stmt->execute(['238548998', '238548998']);
$device = $stmt->fetch(PDO::FETCH_ASSOC);
if ($device) {
    foreach ($device as $k => $v) {
        echo "$k: $v\n";
    }
} else {
    echo "NOT FOUND\n";
}

echo "\n=== All devices ===\n";
$stmt = $pdo->query("SELECT id, user_id, device_uid, system_id, machine_identifier, name, is_online, last_seen_at FROM devices");
while ($r = $stmt->fetch()) {
    echo implode("\t", $r) . "\n";
}

echo "\n=== Users ===\n";
$stmt = $pdo->query("SELECT id, username, email, unique_user_id FROM users");
while ($r = $stmt->fetch()) {
    echo implode("\t", $r) . "\n";
}
