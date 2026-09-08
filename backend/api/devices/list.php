<?php
// backend/api/devices/list.php
header("Content-Type: application/json");
header("Access-Control-Allow-Origin: *");

// Safe diagnostics: log real errors to the server error log ONLY.
// display_errors is kept OFF so JSON responses are never corrupted by HTML/notices.
error_reporting(E_ALL);
ini_set('display_errors', '0');
ini_set('log_errors', '1');

if (session_status() === PHP_SESSION_NONE) {
    session_start();
}

if (empty($_SESSION['user_id'])) {
    http_response_code(401);
    echo json_encode(["status" => "error", "message" => "Unauthorized"]);
    exit();
}
$currentUserId = (int)$_SESSION['user_id'];

require_once __DIR__ . '/../../config/database.php';

if (!isset($pdo) || !($pdo instanceof PDO)) {
    error_log("[DEVICE API ERROR] database connection unavailable");
    http_response_code(500);
    echo json_encode(["status" => "error", "message" => "Database connection failed"]);
    exit();
}

error_log("[DEVICE API] account_id=$currentUserId");

try {
    error_log("[DEVICE API] query started");

    // 1. Mark offline if heartbeat is older than 30s
    $pdo->exec("
        UPDATE devices 
        SET is_online = 0 
        WHERE (last_seen_at IS NULL OR last_seen_at < (NOW() - INTERVAL 30 SECOND)) AND is_online = 1
    ");

    // 2. Fetch all registered devices/systems with dynamically calculated online status
    // NOTE: use prepare() + execute() — query() executes immediately and cannot bind :user_id.
    $stmt = $pdo->prepare("
        SELECT 
            id, 
            user_id,
            COALESCE(system_id, device_uid) AS system_id,
            device_uid, 
            machine_identifier,
            name, 
            os_type, 
            ip_address, 
            CASE 
                WHEN is_online = 1 AND last_seen_at >= (NOW() - INTERVAL 30 SECOND) THEN 1 
                ELSE 0 
            END AS is_online, 
            last_seen_at,
            created_at 
        FROM devices 
        WHERE user_id = :user_id
        ORDER BY is_online DESC, last_seen_at DESC, id DESC
    ");
    $stmt->execute(['user_id' => $currentUserId]);

    $devices = $stmt->fetchAll(PDO::FETCH_ASSOC);

    error_log("[DEVICE API] devices found=" . count($devices));

    echo json_encode([
        "status" => "success",
        "count" => count($devices),
        "devices" => $devices
    ]);
    error_log("[DEVICE API] returning JSON successfully");
} catch (\PDOException $e) {
    error_log("[DEVICE API ERROR] " . $e->getMessage());
    http_response_code(500);
    echo json_encode(["status" => "error", "message" => "Device list query failed"]);
}