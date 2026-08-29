<?php
require_once __DIR__ . '/../backend/config/database.php';

// Simulate what the agent sends
$payload = [
    'machine_identifier' => '990f6284-edef-4762-a627-0eb1face7c12',
    'system_id' => '238548998'
];

$ch = curl_init();
curl_setopt($ch, CURLOPT_URL, 'http://127.0.0.1/Screen%20Share/backend/api/devices/heartbeat.php');
curl_setopt($ch, CURLOPT_POST, true);
curl_setopt($ch, CURLOPT_POSTFIELDS, json_encode($payload));
curl_setopt($ch, CURLOPT_RETURNTRANSFER, true);
curl_setopt($ch, CURLOPT_HTTPHEADER, ['Content-Type: application/json']);
$response = curl_exec($ch);
curl_close($ch);

echo "Heartbeat response: " . $response . "\n";

// Check the database
$stmt = $pdo->prepare('SELECT system_id, is_online, last_seen_at FROM devices WHERE system_id = ?');
$stmt->execute(['238548998']);
$row = $stmt->fetch();
echo "DB state: system_id=" . $row['system_id'] . " is_online=" . $row['is_online'] . " last_seen=" . $row['last_seen_at'] . "\n";
