<?php
// backend/api/connections/status.php
header('Content-Type: application/json');
header('Access-Control-Allow-Origin: *');

require_once __DIR__ . '/../../config/database.php';

$token = trim($_GET['token'] ?? $_GET['request_token'] ?? '');
$requestId = !empty($_GET['id']) ? (int)$_GET['id'] : 0;

if (empty($token) && empty($requestId)) {
    http_response_code(400);
    echo json_encode(["status" => "error", "message" => "Missing request token or id"]);
    exit();
}

try {
    $stmt = $pdo->prepare("
        SELECT * FROM connection_requests 
        WHERE (request_token = ? OR id = ?)
        LIMIT 1
    ");
    $stmt->execute([$token, $requestId]);
    $req = $stmt->fetch(PDO::FETCH_ASSOC);

    if (!$req) {
        http_response_code(404);
        echo json_encode(["status" => "error", "message" => "Request not found"]);
        exit();
    }

    // Auto-expire if pending and past expires_at
    if ($req['status'] === 'pending' && strtotime($req['expires_at']) < time()) {
        $pdo->prepare("UPDATE connection_requests SET status = 'expired' WHERE id = ?")->execute([$req['id']]);
        $req['status'] = 'expired';
    }

    echo json_encode([
        "status" => "success",
        "request_status" => $req['status'],
        "request" => [
            "id" => $req['id'],
            "request_token" => $req['request_token'],
            "requester_system_id" => $req['requester_system_id'],
            "requester_name" => $req['requester_name'],
            "target_system_id" => $req['target_system_id'],
            "status" => $req['status'],
            "created_at" => $req['created_at'],
            "expires_at" => $req['expires_at']
        ]
    ]);

} catch (\PDOException $e) {
    http_response_code(500);
    echo json_encode(["status" => "error", "message" => $e->getMessage()]);
}
