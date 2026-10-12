<?php
header("Content-Type: application/json");
require_once "../../config/database.php";

$data = json_decode(file_get_contents("php://input"), true);
$deviceId = $data['device_id'] ?? null;
$sessionKey = $data['session_key'] ?? null;

$stmt = $pdo->prepare("
    INSERT INTO sessions (session_key, host_device_id, started_at, status)
    SELECT :session_key, id, NOW(), 'active'
    FROM devices WHERE device_uid = :device_uid
");
$stmt->execute([
    ':session_key' => $sessionKey,
    ':device_uid' => $deviceId
]);

echo json_encode(["status" => "success", "session_id" => $pdo->lastInsertId()]);    