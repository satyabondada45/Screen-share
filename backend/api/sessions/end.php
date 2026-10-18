<?php
// DeskStream Session End API Endpoint
// Authoritatively concludes remote sessions, sets ended_at and status='completed',
// with true idempotency and support for both viewer and host disconnection events.
if (!headers_sent()) {
    header("Content-Type: application/json; charset=UTF-8");
    header("Access-Control-Allow-Origin: *");
    header("Access-Control-Allow-Methods: POST, OPTIONS");
    header("Access-Control-Allow-Headers: Content-Type, Authorization, X-DeskStream-Session, X-Auth-Token");
}

if (($_SERVER['REQUEST_METHOD'] ?? '') === 'OPTIONS') {
    http_response_code(204);
    exit();
}

$dbCandidates = [
    __DIR__ . '/../../config/database.php',
    __DIR__ . '/../config/database.php',
    dirname(__DIR__, 2) . '/config/database.php',
    ($_SERVER['DOCUMENT_ROOT'] ?? '') . '/DeskStream/config/database.php',
    ($_SERVER['DOCUMENT_ROOT'] ?? '') . '/config/database.php',
];
$dbFound = false;
foreach ($dbCandidates as $candidate) {
    if ($candidate && file_exists($candidate)) {
        require_once $candidate;
        $dbFound = true;
        break;
    }
}
if (!$dbFound || !isset($pdo)) {
    http_response_code(500);
    echo json_encode(["status" => "error", "message" => "Database connection unavailable."]);
    exit();
}

if (session_status() === PHP_SESSION_NONE) {
    session_start();
}

// 1. Session token restoration from custom headers
if (empty($_SESSION['user_id'])) {
    $token = null;
    if (!empty($_SERVER['HTTP_X_DESKSTREAM_SESSION'])) {
        $token = trim($_SERVER['HTTP_X_DESKSTREAM_SESSION']);
    } elseif (!empty($_SERVER['HTTP_X_AUTH_TOKEN'])) {
        $token = trim($_SERVER['HTTP_X_AUTH_TOKEN']);
    } elseif (!empty($_SERVER['HTTP_AUTHORIZATION']) && preg_match('/Bearer\s+(\S+)/i', $_SERVER['HTTP_AUTHORIZATION'], $m)) {
        $token = $m[1];
    }
    if ($token && preg_match('/^[a-zA-Z0-9,-]{16,128}$/', $token)) {
        if (session_status() === PHP_SESSION_ACTIVE) {
            session_write_close();
        }
        session_id($token);
        session_start();
    }
}

$raw = file_get_contents("php://input");
$data = json_decode($raw, true) ?: [];

$sessionKey = trim((string)($data['session_key'] ?? ''));
$requestToken = trim((string)($data['request_token'] ?? ''));
$sessionId = !empty($data['session_id']) ? (int)$data['session_id'] : 0;
$deviceId = trim((string)($data['device_id'] ?? $data['system_id'] ?? ''));
$machineIdentifier = trim((string)($data['machine_identifier'] ?? ''));
$bandwidthMb = (float)($data['bandwidth_used_mb'] ?? 0.00);
if ($bandwidthMb < 0 || !is_finite($bandwidthMb)) {
    $bandwidthMb = 0.00;
}

if ($sessionKey === '' && $requestToken === '' && $sessionId === 0) {
    http_response_code(400);
    echo json_encode(["status" => "error", "message" => "Missing session_key, request_token, or session_id"]);
    exit();
}

$callerUserId = filter_var($_SESSION['user_id'] ?? null, FILTER_VALIDATE_INT, ['options' => ['min_range' => 1]]);

try {
    $pdo->beginTransaction();

    // 2. Locate target session record
    $findSql = "
        SELECT s.id, s.session_key, s.host_device_id, s.requester_user_id, s.connection_request_id,
               s.status, s.started_at, s.ended_at, s.bandwidth_used_mb,
               d.user_id AS host_user_id, d.system_id AS host_system_id, d.device_uid AS host_device_uid,
               d.machine_identifier AS host_machine_id,
               cr.requester_user_id AS cr_requester_user_id, cr.target_user_id AS cr_target_user_id
        FROM `sessions` s
        JOIN `devices` d ON d.id = s.host_device_id
        LEFT JOIN `connection_requests` cr ON cr.id = s.connection_request_id
        WHERE " . ($sessionId > 0 ? "s.id = :sid" : ($sessionKey !== '' ? "s.session_key = :skey" : "cr.request_token = :rtoken")) . "
        LIMIT 1
        FOR UPDATE
    ";
    $findStmt = $pdo->prepare($findSql);
    if ($sessionId > 0) {
        $findStmt->execute([':sid' => $sessionId]);
    } elseif ($sessionKey !== '') {
        $findStmt->execute([':skey' => $sessionKey]);
    } else {
        $findStmt->execute([':rtoken' => $requestToken]);
    }

    $session = $findStmt->fetch(PDO::FETCH_ASSOC);

    // If not found by session_key directly, try lookup via connection_requests
    if (!$session && $requestToken !== '') {
        $altStmt = $pdo->prepare("
            SELECT s.id, s.session_key, s.host_device_id, s.requester_user_id, s.connection_request_id,
                   s.status, s.started_at, s.ended_at, s.bandwidth_used_mb,
                   d.user_id AS host_user_id, d.system_id AS host_system_id, d.device_uid AS host_device_uid,
                   d.machine_identifier AS host_machine_id,
                   cr.requester_user_id AS cr_requester_user_id, cr.target_user_id AS cr_target_user_id
            FROM `sessions` s
            JOIN `devices` d ON d.id = s.host_device_id
            JOIN `connection_requests` cr ON cr.id = s.connection_request_id
            WHERE cr.request_token = :rtoken
            LIMIT 1
            FOR UPDATE
        ");
        $altStmt->execute([':rtoken' => $requestToken]);
        $session = $altStmt->fetch(PDO::FETCH_ASSOC);
    }

    if (!$session) {
        $pdo->rollBack();
        http_response_code(404);
        echo json_encode(["status" => "error", "message" => "No session matching the provided identifier was found."]);
        exit();
    }

    // 3. Authenticate caller (web user session participant OR host agent machine)
    $isHostAgent = false;
    if ($deviceId !== '' && (
        $deviceId === (string)$session['host_system_id']
        || $deviceId === (string)$session['host_device_uid']
    )) {
        if ($machineIdentifier === '' || $machineIdentifier === (string)$session['host_machine_id']) {
            $isHostAgent = true;
        }
    }

    $allowedUserIds = array_filter([
        (int)($session['requester_user_id'] ?? 0),
        (int)($session['host_user_id'] ?? 0),
        (int)($session['cr_requester_user_id'] ?? 0),
        (int)($session['cr_target_user_id'] ?? 0),
    ]);

    if (!$isHostAgent && ($callerUserId === false || !in_array((int)$callerUserId, $allowedUserIds, true))) {
        $pdo->rollBack();
        http_response_code(401);
        echo json_encode(["status" => "error", "code" => "AUTH_REQUIRED", "message" => "Sign in to end this session."]);
        exit();
    }

    // 4. Idempotency Check: if already completed, do not overwrite ended_at or return error
    if (strtolower($session['status']) === 'completed') {
        $pdo->commit();
        echo json_encode([
            "status" => "success",
            "message" => "Session already concluded.",
            "session_id" => (int)$session['id'],
            "already_completed" => true,
            "ended_at" => $session['ended_at']
        ]);
        exit();
    }

    // 5. Finalize active session
    $endStmt = $pdo->prepare("
        UPDATE `sessions`
        SET `ended_at` = UTC_TIMESTAMP(),
            `status` = 'completed',
            `bandwidth_used_mb` = GREATEST(`bandwidth_used_mb`, :bandwidth),
            `last_activity_at` = UTC_TIMESTAMP()
        WHERE `id` = :id
    ");
    $endStmt->execute([
        ':bandwidth' => $bandwidthMb,
        ':id' => $session['id']
    ]);

    // 6. Update connection_request status to 'disconnected'
    if (!empty($session['connection_request_id'])) {
        $crStmt = $pdo->prepare("
            UPDATE `connection_requests`
            SET `status` = 'disconnected', `updated_at` = UTC_TIMESTAMP()
            WHERE `id` = :crid AND `status` IN ('accepted', 'connected')
        ");
        $crStmt->execute([':crid' => $session['connection_request_id']]);
    }

    $pdo->commit();

    echo json_encode([
        "status" => "success",
        "message" => "Session concluded successfully.",
        "session_id" => (int)$session['id']
    ]);

} catch (\Throwable $e) {
    if ($pdo->inTransaction()) {
        $pdo->rollBack();
    }
    error_log("[SESSION END ERROR] " . $e->getMessage());
    http_response_code(500);
    echo json_encode(["status" => "error", "message" => "Failed to conclude session: " . $e->getMessage()]);
}