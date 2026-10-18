<?php
// DeskStream Active Sessions API Endpoint
// Returns active sessions for the authenticated user and cleans up stale sessions.
if (!headers_sent()) {
    header("Content-Type: application/json; charset=UTF-8");
    header("Access-Control-Allow-Origin: *");
    header("Access-Control-Allow-Methods: GET, OPTIONS");
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

// 1. Session token restoration
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

$currentUserId = filter_var($_SESSION['user_id'] ?? null, FILTER_VALIDATE_INT, ['options' => ['min_range' => 1]]);
if ($currentUserId === false) {
    http_response_code(401);
    echo json_encode([
        "status" => "error",
        "code" => "AUTH_REQUIRED",
        "message" => "Sign in to view active sessions."
    ]);
    exit();
}

try {
    // 2. Server-side sweep of inactive sessions
    $pdo->exec("
        UPDATE `sessions`
        SET `ended_at` = COALESCE(`last_activity_at`, `started_at`),
            `status` = 'completed'
        WHERE `status` = 'active'
          AND (`last_activity_at` < (UTC_TIMESTAMP() - INTERVAL 90 SECOND)
               OR (`last_activity_at` IS NULL AND `started_at` < (UTC_TIMESTAMP() - INTERVAL 180 SECOND)))
    ");

    // 3. Fetch active sessions for current user
    $stmt = $pdo->prepare("
        SELECT s.id, s.session_key, s.host_device_id, s.started_at, s.last_activity_at,
               s.status, s.bandwidth_used_mb,
               d.name AS host_device_name, d.system_id AS host_system_id, d.device_uid AS host_device_uid,
               cr.requester_name, cr.requester_system_id
        FROM `sessions` s
        JOIN `devices` d ON d.id = s.host_device_id
        LEFT JOIN `connection_requests` cr ON cr.id = s.connection_request_id
        WHERE s.status = 'active'
          AND (s.requester_user_id = ? OR d.user_id = ? OR cr.target_user_id = ?)
        ORDER BY s.started_at DESC
    ");
    $stmt->execute([(int)$currentUserId, (int)$currentUserId, (int)$currentUserId]);
    $active = $stmt->fetchAll(PDO::FETCH_ASSOC);

    echo json_encode([
        "status" => "success",
        "count" => count($active),
        "active_sessions" => $active
    ]);

} catch (\Throwable $e) {
    error_log("[SESSION ACTIVE ERROR] " . $e->getMessage());
    http_response_code(500);
    echo json_encode(["status" => "error", "message" => "Failed to load active sessions."]);
}
