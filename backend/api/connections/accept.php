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
require_once __DIR__ . '/../../services/SubscriptionService.php';

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
    if (!in_array($req['status'], ['pending', 'accepted'], true)) {
        http_response_code(409);
        echo json_encode(["status" => "error", "message" => "This connection request is no longer pending."]);
        exit();
    }

    // 2. Strict Authorization Validation: Ensure the caller is the target computer
    if (!empty($targetSystemId) && $req['target_system_id'] !== $targetSystemId) {
        http_response_code(403);
        echo json_encode(["status" => "error", "message" => "Unauthorized: Only the designated target computer can accept this connection."]);
        exit();
    }

    // 3. Expiration Check
    $requestExpiresAt = new DateTimeImmutable($req['expires_at'], new DateTimeZone('UTC'));
    if ($requestExpiresAt <= new DateTimeImmutable('now', new DateTimeZone('UTC')) || $req['status'] === 'expired') {
        $pdo->prepare("UPDATE connection_requests SET status = 'expired' WHERE id = ?")->execute([$req['id']]);
        http_response_code(410);
        echo json_encode(["status" => "error", "message" => "Connection request has expired."]);
        exit();
    }

    $pdo->beginTransaction();
    $targetDeviceStmt = $pdo->prepare("
        SELECT id
        FROM devices
        WHERE system_id = ? OR device_uid = ?
        LIMIT 1
        FOR UPDATE
    ");
    $targetDeviceStmt->execute([$req['target_system_id'], $req['target_system_id']]);
    if (!$targetDeviceStmt->fetchColumn()) {
        $pdo->rollBack();
        http_response_code(404);
        echo json_encode(["status" => "error", "message" => "The target device is no longer registered."]);
        exit();
    }

    $accountIds = array_unique(array_filter([
        (int)($req['requester_user_id'] ?? 0),
        (int)($req['target_user_id'] ?? 0)
    ]));
    sort($accountIds, SORT_NUMERIC);
    foreach ($accountIds as $accountId) {
        $sessionAuth = SubscriptionService::checkConcurrentSessionLimit(
            (int)$accountId,
            (string)$req['request_token']
        );
        if (empty($sessionAuth['authorized'])) {
            $pdo->rollBack();
            http_response_code(403);
            echo json_encode([
                "status" => "error",
                "code" => $sessionAuth['code'] ?? 'CONCURRENT_SESSION_LIMIT_EXCEEDED',
                "message" => $sessionAuth['message'] ?? 'The account session limit has been reached.'
            ]);
            exit();
        }
    }

    $otherAcceptedStmt = $pdo->prepare("
        SELECT id
        FROM connection_requests
        WHERE target_system_id = ? AND id <> ?
          AND status = 'accepted' AND expires_at > UTC_TIMESTAMP()
        LIMIT 1
        FOR UPDATE
    ");
    $otherAcceptedStmt->execute([$req['target_system_id'], $req['id']]);
    if ($otherAcceptedStmt->fetchColumn()) {
        $pdo->rollBack();
        http_response_code(409);
        echo json_encode([
            "status" => "error",
            "code" => "TARGET_SESSION_ALREADY_APPROVED",
            "message" => "This device already has an approved session starting."
        ]);
        exit();
    }

    // 4. Update status to accepted
    $updateStmt = $pdo->prepare("
        UPDATE connection_requests 
        SET status = 'accepted', updated_at = UTC_TIMESTAMP()
        WHERE id = ?
    ");
    $updateStmt->execute([$req['id']]);
    $pdo->commit();

    echo json_encode([
        "status" => "success",
        "message" => "Connection accepted.",
        "request_token" => $req['request_token'],
        "requester_system_id" => $req['requester_system_id'],
        "target_system_id" => $req['target_system_id']
    ]);

} catch (\PDOException $e) {
    if ($pdo->inTransaction()) {
        $pdo->rollBack();
    }
    http_response_code(500);
    echo json_encode(["status" => "error", "message" => "Database error: " . $e->getMessage()]);
} catch (Throwable $e) {
    if ($pdo->inTransaction()) {
        $pdo->rollBack();
    }
    error_log('Connection acceptance failed: ' . $e->getMessage());
    http_response_code(500);
    echo json_encode(["status" => "error", "message" => "Unable to accept the connection request."]);
}
