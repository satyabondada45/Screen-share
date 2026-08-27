<?php
// backend/api/connections/accept.php
header('Content-Type: application/json');
header('Access-Control-Allow-Origin: *');
header('Access-Control-Allow-Methods: POST, OPTIONS');
header('Access-Control-Allow-Headers: Content-Type');

if ($_SERVER['REQUEST_METHOD'] === 'OPTIONS') {
    exit(0);
}

require_once __DIR__ . '/../../config/database.php';

if ($_SERVER['REQUEST_METHOD'] !== 'POST') {
    http_response_code(405);
    echo json_encode(["status" => "error", "message" => "Method not allowed"]);
    exit();
}

$rawInput = file_get_contents('php://input');
$data = json_decode($rawInput, true);

$token = trim($data['request_token'] ?? '');
$requestId = !empty($data['request_id']) ? (int)$data['request_id'] : 0;
$targetSystemId = preg_replace('/[^0-9]/', '', (string)($data['target_system_id'] ?? $data['system_id'] ?? ''));

if (empty($token) && empty($requestId)) {
    http_response_code(400);
    echo json_encode(["status" => "error", "message" => "Missing request token or request_id."]);
    exit();
}

try {
    // 1. Fetch connection request
    $stmt = $pdo->prepare("
        SELECT * FROM connection_requests 
        WHERE (request_token = ? OR id = ?)
        LIMIT 1
    ");
    $stmt->execute([$token, $requestId]);
    $req = $stmt->fetch(PDO::FETCH_ASSOC);

    if (!$req) {
        http_response_code(404);
        echo json_encode(["status" => "error", "message" => "Connection request not found."]);
        exit();
    }

    // 2. Strict Authorization Validation: Ensure the caller is the target computer
    if (!empty($targetSystemId) && $req['target_system_id'] !== $targetSystemId) {
        http_response_code(403);
        echo json_encode(["status" => "error", "message" => "Unauthorized: Only the designated target computer can accept this connection."]);
        exit();
    }

    // 3. Expiration Check
    if (strtotime($req['expires_at']) < time() || $req['status'] === 'expired') {
        $pdo->prepare("UPDATE connection_requests SET status = 'expired' WHERE id = ?")->execute([$req['id']]);
        http_response_code(410);
        echo json_encode(["status" => "error", "message" => "Connection request has expired."]);
        exit();
    }

    // 4. Update status to accepted
    $updateStmt = $pdo->prepare("
        UPDATE connection_requests 
        SET status = 'accepted', updated_at = NOW() 
        WHERE id = ?
    ");
    $updateStmt->execute([$req['id']]);

    echo json_encode([
        "status" => "success",
        "message" => "Connection accepted.",
        "request_token" => $req['request_token'],
        "requester_system_id" => $req['requester_system_id'],
        "target_system_id" => $req['target_system_id']
    ]);

} catch (\PDOException $e) {
    http_response_code(500);
    echo json_encode(["status" => "error", "message" => "Database error: " . $e->getMessage()]);
}
