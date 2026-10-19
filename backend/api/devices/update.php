<?php
header("Content-Type: application/json");
header("Access-Control-Allow-Origin: *");
header("Access-Control-Allow-Headers: Content-Type");

if ($_SERVER['REQUEST_METHOD'] === 'OPTIONS') {
    exit(0);
}

require_once __DIR__ . '/../../config/database.php';

$input = json_decode(file_get_contents("php://input"), true) ?? $_POST;
$deviceUid = isset($input['device_uid']) ? trim((string)$input['device_uid']) : (isset($input['system_id']) ? trim((string)$input['system_id']) : null);
$alias = isset($input['alias']) ? trim((string)$input['alias']) : (isset($input['name']) ? trim((string)$input['name']) : null);

if (!$deviceUid || $alias === null || $alias === '') {
    http_response_code(400);
    echo json_encode(["status" => "error", "message" => "Missing device_uid or alias"]);
    exit;
}

try {
    $stmt = $pdo->prepare("UPDATE devices SET name = :name WHERE device_uid = :device_uid OR system_id = :system_id");
    $stmt->execute([
        ':name' => $alias,
        ':device_uid' => $deviceUid,
        ':system_id' => $deviceUid
    ]);

    echo json_encode([
        "status" => "success",
        "message" => "Device alias updated successfully",
        "alias" => $alias,
        "device_uid" => $deviceUid
    ]);
} catch (\PDOException $e) {
    http_response_code(500);
    echo json_encode(["status" => "error", "message" => $e->getMessage()]);
}
