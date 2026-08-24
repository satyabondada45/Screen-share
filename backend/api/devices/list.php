<?php
header("Content-Type: application/json");
header("Access-Control-Allow-Origin: *");

// Fix relative path directly to config/database.php
$configPath = __DIR__ . '/../../config/database.php';
if (!file_exists($configPath)) {
    http_response_code(500);
    echo json_encode(["status" => "error", "message" => "database.php not found at " . $configPath]);
    exit;
}

require_once $configPath;

try {
    // 1. Mark offline if heartbeat is older than 60s
    $pdo->exec("
        UPDATE devices 
        SET is_online = 0 
        WHERE last_seen_at < (NOW() - INTERVAL 60 SECOND) AND is_online = 1
    ");

    // 2. Fetch all devices
    $stmt = $pdo->query("
        SELECT 
            id, 
            device_uid, 
            name, 
            os_type, 
            ip_address, 
            is_online, 
            last_seen_at,
            created_at 
        FROM devices 
        ORDER BY is_online DESC, last_seen_at DESC
    ");

    $devices = $stmt->fetchAll(PDO::FETCH_ASSOC);

    echo json_encode([
        "status" => "success",
        "count" => count($devices),
        "devices" => $devices
    ]);
} catch (\PDOException $e) {
    http_response_code(500);
    echo json_encode(["status" => "error", "message" => $e->getMessage()]);
}