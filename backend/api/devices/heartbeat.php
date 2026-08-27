<?php
// backend/api/devices/heartbeat.php
header('Content-Type: application/json');
header('Access-Control-Allow-Origin: *');
header('Access-Control-Allow-Methods: POST, OPTIONS');
header('Access-Control-Allow-Headers: Content-Type');

if ($_SERVER['REQUEST_METHOD'] === 'OPTIONS') {
    exit(0);
}

require_once __DIR__ . '/../../config/database.php';

$rawInput = file_get_contents('php://input');
$data = json_decode($rawInput, true);

$systemId = preg_replace('/[^0-9]/', '', (string)($data['system_id'] ?? $data['device_uid'] ?? $data['device_id'] ?? ''));
$machineId = trim($data['machine_identifier'] ?? $data['device_uuid'] ?? '');

if (empty($systemId) && empty($machineId)) {
    http_response_code(400);
    echo json_encode(["status" => "error", "message" => "Missing system_id or machine_identifier"]);
    exit();
}

try {
    $stmt = $pdo->prepare("
        UPDATE devices 
        SET is_online = 1, last_seen_at = NOW() 
        WHERE (system_id = ? OR device_uid = ?) OR (machine_identifier IS NOT NULL AND machine_identifier = ?)
    ");
    $stmt->execute([$systemId, $systemId, $machineId]);
    $affected = $stmt->rowCount();

    echo json_encode([
        "status" => "success",
        "system_id" => $systemId,
        "machine_identifier" => $machineId,
        "updated" => ($affected > 0),
        "synced_at" => date('Y-m-d H:i:s')
    ]);
} catch (\PDOException $e) {
    http_response_code(500);
    echo json_encode(["status" => "error", "message" => $e->getMessage()]);
}