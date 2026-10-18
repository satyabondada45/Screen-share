<?php
// DeskStream Session Start API Endpoint
// Authoritatively initializes or refreshes active streaming sessions in MySQL.
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

$deviceId = trim((string)($data['device_id'] ?? $data['target_system_id'] ?? ''));
$sessionKey = trim((string)($data['session_key'] ?? ''));
$requestToken = trim((string)($data['request_token'] ?? ''));
$machineIdentifier = trim((string)($data['machine_identifier'] ?? ''));
$connectionMode = strtolower(trim((string)($data['mode'] ?? $data['connection_mode'] ?? 'desktop')));
if (!in_array($connectionMode, ['desktop', 'reverse'], true)) {
    $connectionMode = 'desktop';
}

if ($deviceId === '' || $sessionKey === '' || $requestToken === '') {
    http_response_code(400);
    echo json_encode([
        "status" => "error",
        "message" => "Missing device_id, session_key, or request_token."
    ]);
    exit();
}

try {
    // 2. Fetch host device
    $devStmt = $pdo->prepare("SELECT id, user_id, system_id, device_uid, machine_identifier FROM devices WHERE device_uid = ? OR system_id = ? LIMIT 1");
    $devStmt->execute([$deviceId, $deviceId]);
    $device = $devStmt->fetch(PDO::FETCH_ASSOC);

    if (!$device) {
        http_response_code(404);
        echo json_encode(["status" => "error", "message" => "Target device not found."]);
        exit();
    }

    $pdo->beginTransaction();

    // 3. Fetch connection request
    $requestStmt = $pdo->prepare("
        SELECT id, request_token, requester_user_id, requester_system_id, target_user_id, target_system_id, status
        FROM connection_requests
        WHERE request_token = ? AND status IN ('accepted', 'connected')
        LIMIT 1
        FOR UPDATE
    ");
    $requestStmt->execute([$requestToken]);
    $connectionRequest = $requestStmt->fetch(PDO::FETCH_ASSOC);

    if (!$connectionRequest) {
        $pdo->rollBack();
        http_response_code(403);
        echo json_encode([
            "status" => "error",
            "code" => "CONNECTION_NOT_AUTHORIZED",
            "message" => "An accepted connection request for this session is required."
        ]);
        exit();
    }

    // 4. Authenticate caller (either web user session or host agent with device credentials)
    $callerUserId = filter_var($_SESSION['user_id'] ?? null, FILTER_VALIDATE_INT, ['options' => ['min_range' => 1]]);
    $isHostAgentAuth = false;

    if ($callerUserId === false) {
        // Check host agent machine authentication
        if ($machineIdentifier !== '' && (
            (string)($device['machine_identifier'] ?? '') === $machineIdentifier
            || (string)($connectionRequest['target_system_id']) === (string)($device['system_id'])
        )) {
            $isHostAgentAuth = true;
            $callerUserId = (int)$device['user_id'];
        }
    }

    if ($callerUserId === false && !$isHostAgentAuth) {
        $pdo->rollBack();
        http_response_code(401);
        echo json_encode([
            "status" => "error",
            "code" => "AUTH_REQUIRED",
            "message" => "Sign in or device authentication required to start a session."
        ]);
        exit();
    }

    // Participant verification
    $allowedParticipants = array_filter([
        (int)($connectionRequest['requester_user_id'] ?? 0),
        (int)($connectionRequest['target_user_id'] ?? 0),
        (int)($device['user_id'] ?? 0)
    ]);
    if (!$isHostAgentAuth && !in_array((int)$callerUserId, $allowedParticipants, true)) {
        $pdo->rollBack();
        http_response_code(403);
        echo json_encode([
            "status" => "error",
            "code" => "FORBIDDEN",
            "message" => "Account does not have permission to join this session."
        ]);
        exit();
    }

    // 5. Subscription Entitlement Check
    $serviceCandidates = [
        __DIR__ . '/../../services/SubscriptionService.php',
        __DIR__ . '/../services/SubscriptionService.php',
        dirname(__DIR__, 2) . '/services/SubscriptionService.php',
    ];
    foreach ($serviceCandidates as $serviceFile) {
        if (file_exists($serviceFile)) {
            require_once $serviceFile;
            break;
        }
    }

    $accountIds = array_unique(array_filter([
        (int)($device['user_id'] ?? 0),
        (int)($connectionRequest['requester_user_id'] ?? 0),
        (int)($connectionRequest['target_user_id'] ?? 0)
    ]));
    sort($accountIds, SORT_NUMERIC);
    $accountPlanSlugs = [];
    foreach ($accountIds as $accountId) {
        if (class_exists('SubscriptionService')) {
            $sessionAuth = SubscriptionService::checkConcurrentSessionLimit(
                (int)$accountId,
                $requestToken
            );
            if (empty($sessionAuth['authorized'])) {
                $pdo->rollBack();
                http_response_code(403);
                echo json_encode([
                    "status" => "error",
                    "code" => "CONCURRENT_SESSION_LIMIT_EXCEEDED",
                    "message" => $sessionAuth['message']
                ]);
                exit();
            }
            $accountPlanSlugs[$accountId] = SubscriptionService::getUserEntitlements((int)$accountId)['plan_slug'];
        } else {
            $accountPlanSlugs[$accountId] = 'free';
        }
    }

    // 6. Check if session already exists for this connection_request_id or session_key
    $existingStmt = $pdo->prepare("
        SELECT id, session_key, status,
               last_activity_at >= UTC_TIMESTAMP() - INTERVAL 90 SECOND AS is_recent
        FROM sessions
        WHERE connection_request_id = ? OR session_key = ?
        LIMIT 1
        FOR UPDATE
    ");
    $existingStmt->execute([$connectionRequest['id'], $sessionKey]);
    $existingSession = $existingStmt->fetch(PDO::FETCH_ASSOC);

    // Check if connection_mode column exists
    $colCheck = $pdo->query("SHOW COLUMNS FROM `sessions` LIKE 'connection_mode'")->fetch();
    $hasConnectionModeCol = (bool)$colCheck;

    if ($existingSession && $existingSession['status'] === 'active' && (int)$existingSession['is_recent'] === 1) {
        // Already active and recent — update activity timestamp and return
        $pdo->prepare("UPDATE sessions SET last_activity_at = UTC_TIMESTAMP() WHERE id = ?")->execute([$existingSession['id']]);
        $pdo->prepare("UPDATE connection_requests SET status = 'connected', updated_at = UTC_TIMESTAMP() WHERE id = ?")->execute([$connectionRequest['id']]);
        $pdo->commit();
        echo json_encode([
            "status" => "success",
            "session_id" => (int)$existingSession['id'],
            "session_key" => $existingSession['session_key']
        ]);
        exit();
    }

    $reqPlan = $accountPlanSlugs[(int)($connectionRequest['requester_user_id'] ?? 0)] ?? null;
    $hostPlan = $accountPlanSlugs[(int)($device['user_id'] ?? 0)] ?? null;

    if ($existingSession) {
        // Re-activate or refresh existing record
        $modeSql = $hasConnectionModeCol ? ", connection_mode = :connection_mode" : "";
        $refreshSql = "
            UPDATE sessions
            SET session_key = :session_key,
                status = 'active',
                started_at = UTC_TIMESTAMP(),
                ended_at = NULL,
                last_activity_at = UTC_TIMESTAMP(),
                bandwidth_used_mb = 0.00,
                requester_plan_slug = :requester_plan_slug,
                host_plan_slug = :host_plan_slug
                {$modeSql}
            WHERE id = :id
        ";
        $refreshParams = [
            ':session_key' => $sessionKey,
            ':requester_plan_slug' => $reqPlan,
            ':host_plan_slug' => $hostPlan,
            ':id' => $existingSession['id']
        ];
        if ($hasConnectionModeCol) {
            $refreshParams[':connection_mode'] = $connectionMode;
        }
        $refreshStmt = $pdo->prepare($refreshSql);
        $refreshStmt->execute($refreshParams);

        $pdo->prepare("UPDATE connection_requests SET status = 'connected', updated_at = UTC_TIMESTAMP() WHERE id = ?")->execute([$connectionRequest['id']]);
        $pdo->commit();

        echo json_encode([
            "status" => "success",
            "session_id" => (int)$existingSession['id'],
            "session_key" => $sessionKey
        ]);
        exit();
    }

    // 7. Insert new authoritative session record
    $modeColSql = $hasConnectionModeCol ? ", connection_mode" : "";
    $modeValSql = $hasConnectionModeCol ? ", :connection_mode" : "";
    $insertSql = "
        INSERT INTO sessions
            (session_key, host_device_id, requester_user_id, connection_request_id,
             requester_plan_slug, host_plan_slug, last_activity_at, started_at, status{$modeColSql})
        VALUES (:session_key, :host_device_id, :requester_user_id, :connection_request_id,
                :requester_plan_slug, :host_plan_slug, UTC_TIMESTAMP(), UTC_TIMESTAMP(), 'active'{$modeValSql})
    ";
    $insertParams = [
        ':session_key' => $sessionKey,
        ':host_device_id' => $device['id'],
        ':requester_user_id' => $connectionRequest['requester_user_id'],
        ':connection_request_id' => $connectionRequest['id'],
        ':requester_plan_slug' => $reqPlan,
        ':host_plan_slug' => $hostPlan
    ];
    if ($hasConnectionModeCol) {
        $insertParams[':connection_mode'] = $connectionMode;
    }

    $insertStmt = $pdo->prepare($insertSql);
    $insertStmt->execute($insertParams);
    $sessionId = (int)$pdo->lastInsertId();

    $pdo->prepare("UPDATE connection_requests SET status = 'connected', updated_at = UTC_TIMESTAMP() WHERE id = ?")->execute([$connectionRequest['id']]);
    $pdo->commit();

    echo json_encode([
        "status" => "success",
        "session_id" => $sessionId,
        "session_key" => $sessionKey
    ]);

} catch (\Throwable $e) {
    if ($pdo->inTransaction()) {
        $pdo->rollBack();
    }
    error_log("[SESSION START ERROR] " . $e->getMessage());
    http_response_code(500);
    echo json_encode(["status" => "error", "message" => "Failed to start session: " . $e->getMessage()]);
}