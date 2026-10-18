<?php
// DeskStream Session Heartbeat API Endpoint
// Keeps active sessions alive and validates entitlement consistency.
if (!headers_sent()) {
    header('Content-Type: application/json; charset=UTF-8');
    header('Access-Control-Allow-Origin: *');
    header('Access-Control-Allow-Methods: POST, OPTIONS');
    header('Access-Control-Allow-Headers: Content-Type, Authorization, X-DeskStream-Session, X-Auth-Token');
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

$data = json_decode(file_get_contents('php://input'), true) ?: [];
$requestToken = trim((string)($data['request_token'] ?? ''));
$sessionKey = trim((string)($data['session_key'] ?? ''));
$deviceId = trim((string)($data['device_id'] ?? $data['system_id'] ?? ''));
$machineIdentifier = trim((string)($data['machine_identifier'] ?? ''));

if ($requestToken === '' && $sessionKey === '') {
    http_response_code(400);
    echo json_encode(['status' => 'error', 'message' => 'A valid request token or session key is required.']);
    exit();
}

$callerUserId = filter_var($_SESSION['user_id'] ?? null, FILTER_VALIDATE_INT, ['options' => ['min_range' => 1]]);

try {
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

    $pdo->beginTransaction();

    $whereClause = "";
    $params = [];
    if ($requestToken !== '' && $sessionKey !== '') {
        $whereClause = "cr.request_token = ? AND s.session_key = ?";
        $params = [$requestToken, $sessionKey];
    } elseif ($sessionKey !== '') {
        $whereClause = "s.session_key = ?";
        $params = [$sessionKey];
    } else {
        $whereClause = "cr.request_token = ?";
        $params = [$requestToken];
    }

    $stmt = $pdo->prepare("
        SELECT s.id, s.connection_request_id, s.requester_user_id, s.requester_plan_slug, s.host_plan_slug,
               d.user_id AS host_user_id, d.system_id AS host_system_id, d.device_uid AS host_device_uid,
               d.machine_identifier AS host_machine_id,
               cr.requester_user_id AS cr_requester_user_id, cr.target_user_id AS cr_target_user_id
        FROM `sessions` s
        JOIN `devices` d ON d.id = s.host_device_id
        LEFT JOIN `connection_requests` cr ON cr.id = s.connection_request_id
        WHERE {$whereClause}
          AND s.status = 'active'
        LIMIT 1
        FOR UPDATE
    ");
    $stmt->execute($params);
    $session = $stmt->fetch(PDO::FETCH_ASSOC);

    if (!$session) {
        $pdo->rollBack();
        http_response_code(404);
        echo json_encode(['status' => 'error', 'message' => 'The active session could not be found.']);
        exit();
    }

    // Authenticate participant
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
        echo json_encode(['status' => 'error', 'message' => 'Sign in to keep this session active.']);
        exit();
    }

    // Entitlement consistency check
    if (class_exists('SubscriptionService') && !empty($session['requester_user_id']) && !empty($session['host_user_id'])) {
        $requesterEntitlements = SubscriptionService::getUserEntitlements((int)$session['requester_user_id']);
        $hostEntitlements = SubscriptionService::getUserEntitlements((int)$session['host_user_id']);
        if ($session['requester_plan_slug'] !== $requesterEntitlements['plan_slug']
            || $session['host_plan_slug'] !== $hostEntitlements['plan_slug']) {
            $pdo->prepare("
                UPDATE `sessions`
                SET `status` = 'completed', `ended_at` = UTC_TIMESTAMP(), `last_activity_at` = UTC_TIMESTAMP()
                WHERE `id` = ? AND `status` = 'active'
            ")->execute([(int)$session['id']]);

            if (!empty($session['connection_request_id'])) {
                $pdo->prepare("
                    UPDATE `connection_requests`
                    SET `status` = 'disconnected', `updated_at` = UTC_TIMESTAMP()
                    WHERE `id` = ? AND `status` IN ('accepted', 'connected')
                ")->execute([(int)$session['connection_request_id']]);
            }
            $pdo->commit();
            http_response_code(409);
            echo json_encode([
                'status' => 'error',
                'code' => 'ENTITLEMENTS_CHANGED',
                'message' => 'A participant’s plan changed. Reconnect to continue under current plan restrictions.'
            ]);
            exit();
        }
    }

    // Touch last_activity_at
    $touchStmt = $pdo->prepare("
        UPDATE `sessions`
        SET `last_activity_at` = UTC_TIMESTAMP()
        WHERE `id` = ? AND `status` = 'active'
    ");
    $touchStmt->execute([(int)$session['id']]);
    $pdo->commit();

    echo json_encode(['status' => 'success']);

} catch (\Throwable $error) {
    if ($pdo->inTransaction()) {
        $pdo->rollBack();
    }
    error_log('Session heartbeat failed: ' . $error->getMessage());
    http_response_code(500);
    echo json_encode(['status' => 'error', 'message' => 'Unable to refresh the session reservation.']);
}
