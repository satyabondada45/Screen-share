<?php
// backend/api/connections/request.php
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

$targetSystemId = preg_replace('/[^0-9]/', '', (string)($data['target_system_id'] ?? $data['target_id'] ?? ''));
$requesterSystemId = preg_replace('/[^0-9]/', '', (string)($data['requester_system_id'] ?? $data['requester_id'] ?? ''));
$requesterName = trim($data['requester_name'] ?? 'Dashboard Viewer');
$requesterUserId = !empty($data['requester_user_id']) ? (int)$data['requester_user_id'] : null;

if (empty($targetSystemId)) {
    http_response_code(400);
    echo json_encode(["status" => "error", "message" => "Target System ID is required."]);
    exit();
}

if (!empty($requesterSystemId) && $requesterSystemId === $targetSystemId) {
    http_response_code(400);
    echo json_encode(["status" => "error", "message" => "Cannot initiate a remote connection to your own computer."]);
    exit();
}

try {
    // 1. Find target device directly by system_id or device_uid
    $targetStmt = $pdo->prepare("
        SELECT id, user_id, system_id, device_uid, name, ip_address, is_online, last_seen_at,
               TIMESTAMPDIFF(SECOND, last_seen_at, NOW()) AS sec_since_seen
        FROM devices 
        WHERE system_id = ? OR device_uid = ?
        ORDER BY (is_online = 1 AND last_seen_at >= (NOW() - INTERVAL 30 SECOND)) DESC, last_seen_at DESC 
        LIMIT 1
    ");
    $targetStmt->execute([$targetSystemId, $targetSystemId]);
    $targetSystem = $targetStmt->fetch(PDO::FETCH_ASSOC);

    if (!$targetSystem) {
        http_response_code(404);
        echo json_encode(["status" => "error", "message" => "Device with ID '{$targetSystemId}' does not exist or has not registered."]);
        exit();
    }

    // 2. Check online status (active if marked online and heartbeat received within last 30s)
    $secSinceSeen = isset($targetSystem['sec_since_seen']) ? (int)$targetSystem['sec_since_seen'] : 999;
    $isActuallyOnline = ($targetSystem['is_online'] == 1) && ($targetSystem['last_seen_at'] !== null) && ($secSinceSeen <= 30);

    if (!$isActuallyOnline) {
        http_response_code(409);
        echo json_encode([
            "status" => "error", 
            "message" => "Target computer '{$targetSystem['name']}' is currently offline."
        ]);
        exit();
    }

    // 3. Generate unique request token
    $requestToken = bin2hex(random_bytes(16));

    // 4. Create pending connection request record (expires in 60s)
    $insertStmt = $pdo->prepare("
        INSERT INTO connection_requests 
        (request_token, requester_user_id, requester_system_id, requester_name, target_user_id, target_system_id, status, created_at, expires_at)
        VALUES (?, ?, ?, ?, ?, ?, 'pending', NOW(), NOW() + INTERVAL 60 SECOND)
    ");

    $insertStmt->execute([
        $requestToken,
        $requesterUserId,
        $requesterSystemId ?: '000000000',
        $requesterName,
        $targetSystem['user_id'],
        $targetSystemId
    ]);

    $requestId = $pdo->lastInsertId();

    echo json_encode([
        "status" => "success",
        "message" => "Connection request created. Awaiting approval from target computer.",
        "request" => [
            "id" => $requestId,
            "request_token" => $requestToken,
            "target_system_id" => $targetSystemId,
            "target_name" => $targetSystem['name'],
            "target_ip" => $targetSystem['ip_address'],
            "status" => "pending",
            "expires_in_seconds" => 60
        ]
    ]);

} catch (\PDOException $e) {
    http_response_code(500);
    echo json_encode(["status" => "error", "message" => "Database error: " . $e->getMessage()]);
}
