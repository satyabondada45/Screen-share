<?php
header("Content-Type: application/json");
header("Access-Control-Allow-Origin: *");
header("Access-Control-Allow-Headers: Content-Type");

if ($_SERVER['REQUEST_METHOD'] === 'OPTIONS') {
    exit(0);
}

require_once __DIR__ . '/../../config/database.php';

$data = json_decode(file_get_contents("php://input"), true);

if (!isset($data['device_id']) && !isset($data['device_uid'])) {
    http_response_code(400);
    echo json_encode(["status" => "error", "message" => "Missing device_id"]);
    exit;
}

$deviceId = $data['device_id'] ?? $data['device_uid'];
$osInfo = $data['os_info'] ?? 'windows';
$hostname = $data['hostname'] ?? 'RustDevice';
$ipAddress = $_SERVER['REMOTE_ADDR'] ?? '127.0.0.1';

try {
    $stmt = $pdo->prepare("
        INSERT INTO devices (device_uid, name, os_type, ip_address, is_online, last_seen_at, created_at)
        VALUES (?, ?, ?, ?, 1, NOW(), NOW())
        ON DUPLICATE KEY UPDATE 
            name = VALUES(name),
            os_type = VALUES(os_type),
            is_online = 1,
            ip_address = VALUES(ip_address),
            last_seen_at = NOW()
    ");

    $stmt->execute([
        $deviceId,
        $hostname,
        $osInfo,
        $ipAddress
    ]);

    echo json_encode(["status" => "success", "message" => "Device heartbeat recorded"]);
} catch (\PDOException $e) {
    http_response_code(500);
    echo json_encode(["status" => "error", "message" => $e->getMessage()]);
}
?>