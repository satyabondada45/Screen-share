<?php
header("Content-Type: application/json");
header("Access-Control-Allow-Origin: *");

require_once __DIR__ . '/../../config/database.php';

$data = json_decode(file_get_contents("php://input"), true);

if (!isset($data['device_id'])) {
    http_response_code(400);
    echo json_encode(["status" => "error", "message" => "Missing device_id"]);
    exit;
}

$deviceId = $data['device_id'];
$osInfo = $data['os_info'] ?? ($data['os_type'] ?? 'windows');
$hostname = $data['hostname'] ?? ($data['name'] ?? 'RustDevice');
$ipAddress = $_SERVER['REMOTE_ADDR'] ?? '127.0.0.1';

try {
    $stmt = $pdo->prepare("
        INSERT INTO devices (device_uid, name, os_type, ip_address, is_online, last_seen_at, created_at)
        VALUES (:device_uid, :name, :os_type, :ip_address, 1, NOW(), NOW())
        ON DUPLICATE KEY UPDATE 
            name = :name,
            os_type = :os_type,
            is_online = 1,
            ip_address = :ip_address,
            last_seen_at = NOW()
    ");

    $stmt->execute([
        ':device_uid' => $deviceId,
        ':name' => $hostname,
        ':os_type' => $osInfo,
        ':ip_address' => $ipAddress
    ]);

    echo json_encode(["status" => "success", "message" => "Device registered"]);
} catch (\PDOException $e) {
    http_response_code(500);
    echo json_encode(["status" => "error", "message" => $e->getMessage()]);
}