<?php
// backend/api/connections/incoming.php
header('Content-Type: application/json');
header('Access-Control-Allow-Origin: *');

require_once __DIR__ . '/../../config/database.php';

$systemId = preg_replace('/[^0-9]/', '', (string)($_GET['system_id'] ?? $_GET['target_id'] ?? ''));

if (empty($systemId)) {
    http_response_code(400);
    echo json_encode(["status" => "error", "message" => "system_id is required"]);
    exit();
}

try {
    // Auto-expire stale requests
    $pdo->exec("UPDATE connection_requests SET status = 'expired' WHERE status = 'pending' AND expires_at < NOW()");

    $stmt = $pdo->prepare("
        SELECT id, request_token, requester_system_id, requester_name, created_at, expires_at 
        FROM connection_requests 
        WHERE target_system_id = ? AND status = 'pending' AND expires_at > NOW() 
        ORDER BY id DESC 
        LIMIT 1
    ");
    $stmt->execute([$systemId]);
    $request = $stmt->fetch(PDO::FETCH_ASSOC);

    echo json_encode([
        "status" => "success",
        "has_request" => !empty($request),
        "request" => $request ?: null
    ]);

} catch (\PDOException $e) {
    http_response_code(500);
    echo json_encode(["status" => "error", "message" => $e->getMessage()]);
}
