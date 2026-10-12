<?php
header("Content-Type: application/json");
require_once "../../config/database.php";

$data = json_decode(file_get_contents("php://input"), true);
$deviceId = $data['device_id'] ?? null;
$sessionKey = $data['session_key'] ?? null;

if (!$deviceId || !$sessionKey) {
    http_response_code(400);
    echo json_encode(["status" => "error", "message" => "Missing device_id or session_key"]);
    exit();
}

try {
    $devStmt = $pdo->prepare("SELECT id, user_id FROM devices WHERE device_uid = ? OR system_id = ? LIMIT 1");
    $devStmt->execute([$deviceId, $deviceId]);
    $device = $devStmt->fetch(PDO::FETCH_ASSOC);

    if (!$device) {
        http_response_code(404);
        echo json_encode(["status" => "error", "message" => "Device not found."]);
        exit();
    }

    if (!empty($device['user_id'])) {
        require_once __DIR__ . '/../../services/SubscriptionService.php';
        $sessionAuth = SubscriptionService::checkConcurrentSessionLimit((int)$device['user_id'], $sessionKey);
        if (empty($sessionAuth['authorized'])) {
            http_response_code(403);
            echo json_encode([
                "status" => "error",
                "code" => "CONCURRENT_SESSION_LIMIT_EXCEEDED",
                "message" => $sessionAuth['message']
            ]);
            exit();
        }
    }

    $stmt = $pdo->prepare("
        INSERT INTO sessions (session_key, host_device_id, started_at, status)
        VALUES (:session_key, :host_device_id, NOW(), 'active')
    ");
    $stmt->execute([
        ':session_key' => $sessionKey,
        ':host_device_id' => $device['id']
    ]);

    echo json_encode(["status" => "success", "session_id" => $pdo->lastInsertId()]);
} catch (\Throwable $e) {
    http_response_code(500);
    echo json_encode(["status" => "error", "message" => "Failed to start session: " . $e->getMessage()]);
}    