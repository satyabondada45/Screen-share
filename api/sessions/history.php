<?php
// DeskStream Session History API Endpoint
// Returns authenticated user's remote session history with account isolation,
// duration calculation, and peer workstation metadata.
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

$currentUserId = filter_var($_SESSION['user_id'] ?? null, FILTER_VALIDATE_INT, ['options' => ['min_range' => 1]]);
if ($currentUserId === false) {
    http_response_code(401);
    echo json_encode([
        "status" => "error",
        "code" => "AUTH_REQUIRED",
        "message" => "Sign in to view remote session history."
    ]);
    exit();
}

try {
    // 2. Server-side sweep: Finalize stale active sessions older than 120 seconds of inactivity
    $pdo->exec("
        UPDATE `sessions`
        SET `ended_at` = COALESCE(`last_activity_at`, `started_at`),
            `status` = 'completed'
        WHERE `status` = 'active'
          AND (`last_activity_at` < (UTC_TIMESTAMP() - INTERVAL 120 SECOND)
               OR (`last_activity_at` IS NULL AND `started_at` < (UTC_TIMESTAMP() - INTERVAL 300 SECOND)))
    ");

    // 3. Check schema columns
    $colCheck = $pdo->query("SHOW COLUMNS FROM `sessions` LIKE 'connection_mode'")->fetch();
    $hasConnectionMode = (bool)$colCheck;
    $modeColSelect = $hasConnectionMode ? "s.connection_mode," : "'desktop' AS connection_mode,";

    // 4. Pagination params
    $limit = isset($_GET['limit']) ? max(1, min(100, (int)$_GET['limit'])) : 20;
    $offset = isset($_GET['offset']) ? max(0, (int)$_GET['offset']) : 0;
    $statusFilter = strtolower(trim((string)($_GET['status'] ?? 'all')));

    $statusWhere = "";
    $statusParams = [];
    if (in_array($statusFilter, ['active', 'completed', 'rejected'], true)) {
        $statusWhere = "AND LOWER(s.status) = ?";
        $statusParams[] = $statusFilter;
    }

    // 5. Total count query with strict account isolation
    $countSql = "
        SELECT COUNT(DISTINCT s.id)
        FROM `sessions` s
        JOIN `devices` d ON d.id = s.host_device_id
        LEFT JOIN `connection_requests` cr ON cr.id = s.connection_request_id
        WHERE (s.requester_user_id = ? OR d.user_id = ? OR cr.target_user_id = ?)
        {$statusWhere}
    ";
    $countStmt = $pdo->prepare($countSql);
    $countStmt->execute(array_merge([(int)$currentUserId, (int)$currentUserId, (int)$currentUserId], $statusParams));
    $totalCount = (int)$countStmt->fetchColumn();

    // 6. Paginated sessions query
    $dataSql = "
        SELECT s.id, s.session_key, s.host_device_id, s.started_at, s.ended_at, s.status,
               s.bandwidth_used_mb, s.requester_user_id, s.connection_request_id,
               s.last_activity_at,
               {$modeColSelect}
               TIMESTAMPDIFF(SECOND, s.started_at, COALESCE(s.ended_at, s.last_activity_at, s.started_at)) AS duration_seconds,
               d.name AS host_device_name, d.system_id AS host_system_id, d.device_uid AS host_device_uid,
               d.os_type AS host_os_type, d.ip_address AS host_ip_address, d.user_id AS host_user_id,
               cr.requester_name, cr.requester_system_id, cr.request_token
        FROM `sessions` s
        JOIN `devices` d ON d.id = s.host_device_id
        LEFT JOIN `connection_requests` cr ON cr.id = s.connection_request_id
        WHERE (s.requester_user_id = ? OR d.user_id = ? OR cr.target_user_id = ?)
        {$statusWhere}
        ORDER BY s.started_at DESC, s.id DESC
        LIMIT ? OFFSET ?
    ";

    $dataStmt = $pdo->prepare($dataSql);
    $bindIdx = 1;
    $dataStmt->bindValue($bindIdx++, (int)$currentUserId, PDO::PARAM_INT);
    $dataStmt->bindValue($bindIdx++, (int)$currentUserId, PDO::PARAM_INT);
    $dataStmt->bindValue($bindIdx++, (int)$currentUserId, PDO::PARAM_INT);
    foreach ($statusParams as $sp) {
        $dataStmt->bindValue($bindIdx++, $sp, PDO::PARAM_STR);
    }
    $dataStmt->bindValue($bindIdx++, $limit, PDO::PARAM_INT);
    $dataStmt->bindValue($bindIdx++, $offset, PDO::PARAM_INT);
    $dataStmt->execute();
    $rows = $dataStmt->fetchAll(PDO::FETCH_ASSOC);

    $formattedSessions = [];
    foreach ($rows as $r) {
        $durationSec = max(0, (int)($r['duration_seconds'] ?? 0));
        $mins = floor($durationSec / 60);
        $secs = $durationSec % 60;
        $hours = floor($mins / 60);
        $mins = $mins % 60;

        $durationFormatted = "";
        if ($hours > 0) {
            $durationFormatted = "{$hours}h {$mins}m";
        } elseif ($mins > 0) {
            $durationFormatted = "{$mins}m {$secs}s";
        } else {
            $durationFormatted = "{$secs}s";
        }

        $isHost = ((int)($r['host_user_id'] ?? 0) === (int)$currentUserId);
        $remoteSystemId = $isHost ? ($r['requester_system_id'] ?: 'Remote') : ($r['host_system_id'] ?: $r['host_device_uid']);
        $remoteName = $isHost ? ($r['requester_name'] ?: 'Remote Viewer') : ($r['host_device_name'] ?: 'Workstation');

        $formattedSessions[] = [
            "id" => (int)$r['id'],
            "session_key" => $r['session_key'],
            "status" => strtolower($r['status'] ?? 'completed'),
            "mode" => strtolower($r['connection_mode'] ?? 'desktop'),
            "started_at" => !empty($r['started_at']) ? gmdate('Y-m-d\TH:i:s\Z', strtotime($r['started_at'])) : null,
            "ended_at" => !empty($r['ended_at']) ? gmdate('Y-m-d\TH:i:s\Z', strtotime($r['ended_at'])) : null,
            "duration_seconds" => $durationSec,
            "duration_formatted" => $durationFormatted,
            "bandwidth_used_mb" => (float)($r['bandwidth_used_mb'] ?? 0.0),
            "role" => $isHost ? 'host' : 'viewer',
            "peer" => [
                "system_id" => $remoteSystemId,
                "name" => $remoteName,
                "os_type" => $r['host_os_type'] ?: 'windows',
                "ip_address" => $r['host_ip_address'] ?: ''
            ]
        ];
    }

    echo json_encode([
        "status" => "success",
        "total" => $totalCount,
        "count" => count($formattedSessions),
        "limit" => $limit,
        "offset" => $offset,
        "sessions" => $formattedSessions
    ], JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES);

} catch (\Throwable $e) {
    error_log("[SESSION HISTORY ERROR] " . $e->getMessage());
    http_response_code(500);
    echo json_encode(["status" => "error", "message" => "Unable to load session history."]);
}
