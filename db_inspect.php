<?php
require_once __DIR__ . '/backend/config/database.php';
echo "=== ALL DEVICES (id, user_id, device_uid, system_id, machine_identifier) ===\n";
$stmt = $pdo->query("SELECT id, user_id, device_uid, system_id, machine_identifier, name, is_online, last_seen_at FROM devices ORDER BY id DESC LIMIT 20");
$rows = $stmt->fetchAll(PDO::FETCH_ASSOC);
foreach ($rows as $r) {
    echo sprintf("id=%-5d user=%-5s uid=%-12s sys_id=%-12s machine=%-40s name=%-20s online=%d\n",
        $r['id'], $r['user_id']??'null', $r['device_uid'], $r['system_id'], $r['machine_identifier']??'', $r['name'], $r['is_online']);
}
echo "\n=== CHECKING DUPLICATE machine_identifiers ===\n";
$stmt2 = $pdo->query("SELECT machine_identifier, COUNT(*) as cnt FROM devices GROUP BY machine_identifier HAVING cnt > 1");
$dups = $stmt2->fetchAll(PDO::FETCH_ASSOC);
echo json_encode($dups, JSON_PRETTY_PRINT);
echo "\n=== CHECKING device_uid != system_id ===\n";
$stmt3 = $pdo->query("SELECT id, device_uid, system_id, name FROM devices WHERE device_uid != system_id OR system_id IS NULL");
$mismatches = $stmt3->fetchAll(PDO::FETCH_ASSOC);
echo json_encode($mismatches, JSON_PRETTY_PRINT);
