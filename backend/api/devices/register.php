<?php
// backend/api/devices/register.php
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
    echo json_encode(["status" => "error", "success" => false, "message" => "Method not allowed"]);
    exit();
}

$rawInput = file_get_contents('php://input');
$data = json_decode($rawInput, true);

if (!$data) {
    http_response_code(400);
    echo json_encode(["status" => "error", "success" => false, "message" => "Invalid JSON payload"]);
    exit();
}

$machineId = trim($data['machine_identifier'] ?? $data['device_uuid'] ?? '');
$providedUid = preg_replace('/[^0-9]/', '', (string)($data['device_uid'] ?? $data['device_id'] ?? $data['system_id'] ?? ''));
$hostname   = trim($data['hostname'] ?? $data['device_name'] ?? $data['name'] ?? 'Workstation');
$osType     = strtolower(trim($data['os_info'] ?? $data['os_type'] ?? $data['operating_system'] ?? 'windows'));
$userId     = !empty($data['user_id']) ? (int)$data['user_id'] : null;

// Determine Client IP
$clientIp = $_SERVER['HTTP_X_FORWARDED_FOR'] ?? $_SERVER['REMOTE_ADDR'] ?? '127.0.0.1';
if (strpos($clientIp, ',') !== false) {
    $clientIp = trim(explode(',', $clientIp)[0]);
}

/**
 * Generates a random, collision-safe 9-digit system ID.
 */
function generateUniqueSystemId(PDO $pdo) {
    for ($i = 0; $i < 10; $i++) {
        $candidate = (string)random_int(100000000, 999999999);
        $check = $pdo->prepare("SELECT id FROM devices WHERE system_id = ? OR device_uid = ? LIMIT 1");
        $check->execute([$candidate, $candidate]);
        if (!$check->fetch()) {
            return $candidate;
        }
    }
    // Fallback timestamp + random if high density
    return substr((string)time(), -4) . (string)random_int(10000, 99999);
}

try {
    $existing = null;

    // 1. If machine_identifier (UUID) provided, check for existing permanent registration
    if (!empty($machineId)) {
        $stmt = $pdo->prepare("SELECT * FROM devices WHERE machine_identifier = ? LIMIT 1");
        $stmt->execute([$machineId]);
        $existing = $stmt->fetch(PDO::FETCH_ASSOC);
    }

    // 2. If not found by machine_identifier but provided a system_id/device_uid
    if (!$existing && !empty($providedUid)) {
        $stmt = $pdo->prepare("SELECT * FROM devices WHERE device_uid = ? OR system_id = ? LIMIT 1");
        $stmt->execute([$providedUid, $providedUid]);
        $existing = $stmt->fetch(PDO::FETCH_ASSOC);
    }

    if ($existing) {
        // Reuse existing permanent Device System ID (NEVER overwrite with user ID)
        $systemId = $existing['system_id'] ?: $existing['device_uid'];
        if (empty($systemId) && !empty($providedUid)) {
            $systemId = $providedUid;
        }
        $resolvedUserId = $userId ?: $existing['user_id'];
        
        $updateStmt = $pdo->prepare("
            UPDATE devices 
            SET name = ?,
                os_type = ?,
                ip_address = ?,
                is_online = 1,
                user_id = COALESCE(?, user_id),
                machine_identifier = COALESCE(?, machine_identifier),
                system_id = ?,
                device_uid = ?,
                last_seen_at = NOW()
            WHERE id = ?
        ");
        $updateStmt->execute([
            $hostname, 
            $osType, 
            $clientIp, 
            $resolvedUserId ?: null, 
            $machineId ?: null, 
            $systemId, 
            $systemId, 
            $existing['id']
        ]);

        echo json_encode([
            "status" => "success",
            "success" => true,
            "user_id" => $resolvedUserId,
            "message" => "Existing system recognized.",
            "system" => [
                "id" => $existing['id'],
                "system_id" => (string)$systemId,
                "device_uid" => (string)$systemId,
                "device_name" => $hostname,
                "machine_identifier" => $existing['machine_identifier'] ?: $machineId,
                "os_type" => $osType,
                "ip_address" => $clientIp,
                "status" => "online"
            ]
        ]);
        exit();
    }

    // 3. New System Registration: Use provided deterministic 9-digit System ID or generate unique one
    $systemId = (!empty($providedUid) && strlen($providedUid) === 9) ? $providedUid : generateUniqueSystemId($pdo);

    $insertStmt = $pdo->prepare("
        INSERT INTO devices (user_id, device_uid, system_id, machine_identifier, name, os_type, ip_address, is_online, last_seen_at, created_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, 1, NOW(), NOW())
    ");
    $insertStmt->execute([
        $userId,
        $systemId,
        $systemId,
        $machineId ?: null,
        $hostname,
        $osType,
        $clientIp
    ]);

    $newId = $pdo->lastInsertId();

    echo json_encode([
        "status" => "success",
        "success" => true,
        "user_id" => $userId,
        "message" => "New unique system registered successfully.",
        "system" => [
            "id" => $newId,
            "system_id" => (string)$systemId,
            "device_uid" => (string)$systemId,
            "device_name" => $hostname,
            "machine_identifier" => $machineId,
            "os_type" => $osType,
            "ip_address" => $clientIp,
            "status" => "online"
        ]
    ]);

} catch (\PDOException $e) {
    http_response_code(500);
    echo json_encode(["status" => "error", "success" => false, "message" => "Database error: " . $e->getMessage()]);
}