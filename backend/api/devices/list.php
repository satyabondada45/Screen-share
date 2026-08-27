<?php
// backend/api/devices/list.php
header("Content-Type: application/json");
header("Access-Control-Allow-Origin: *");

require_once __DIR__ . '/../../config/database.php';

try {
    // 1. Mark offline if heartbeat is older than 30s
    $pdo->exec("
        UPDATE devices 
        SET is_online = 0 
        WHERE (last_seen_at IS NULL OR last_seen_at < (NOW() - INTERVAL 30 SECOND)) AND is_online = 1
    ");

    // 2. Fetch all registered devices/systems with dynamically calculated online status
    $stmt = $pdo->query("
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
        ORDER BY is_online DESC, last_seen_at DESC, id DESC
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