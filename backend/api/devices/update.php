<?php
header("Content-Type: application/json");
header("Access-Control-Allow-Origin: *");
header("Access-Control-Allow-Headers: Content-Type");

if ($_SERVER['REQUEST_METHOD'] === 'OPTIONS') {
    exit(0);
}

require_once __DIR__ . '/../../config/database.php';

$input = json_decode(file_get_contents("php://input"), true) ?? $_POST;
$deviceUid = $input['device_uid'] ?? null;
$alias = $input['alias'] ?? $input['name'] ?? null;

if (!$deviceUid || !$alias) {
    http_response_code(400);
    echo json_encode(["status" => "error", "message" => "Missing device_uid or alias"]);
    exit;
}

try {
    $stmt = $pdo->prepare("UPDATE devices SET name = :name WHERE device_uid = :device_uid");
    $stmt->execute([
        ':name' => $alias,
        ':device_uid' => $deviceUid
    ]);

    echo json_encode(["status" => "success", "message" => "Device alias updated successfully"]);
} catch (\PDOException $e) {
    http_response_code(500);
    echo json_encode(["status" => "error", "message" => $e->getMessage()]);
}
