<?php
header("Content-Type: application/json");
header("Access-Control-Allow-Origin: *");

require_once __DIR__ . '/../../config/database.php';

$data = json_decode(file_get_contents("php://input"), true);
$sessionKey = $data['session_key'] ?? null;
$bandwidthMb = $data['bandwidth_used_mb'] ?? 0.00;

if (!$sessionKey) {
    http_response_code(400);
    echo json_encode(["status" => "error", "message" => "Missing session_key"]);
    exit;
}

try {
    $stmt = $pdo->prepare("
        UPDATE sessions 
        SET 
            ended_at = NOW(),
            status = 'completed',
            bandwidth_used_mb = :bandwidth
        WHERE session_key = :session_key AND status = 'active'
    ");

    $stmt->execute([
        ':session_key' => $sessionKey,
        ':bandwidth'   => $bandwidthMb
    ]);

    echo json_encode(["status" => "success", "message" => "Session concluded"]);
} catch (\PDOException $e) {
    http_response_code(500);
    echo json_encode(["status" => "error", "message" => $e->getMessage()]);
}