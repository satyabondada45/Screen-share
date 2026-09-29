<?php
header("Content-Type: application/json");
header("Access-Control-Allow-Origin: *");
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

$data = json_decode(file_get_contents("php://input"), true) ?? $_GET;
$sessionId = $data['session_id'] ?? null;

if (!$sessionId) {
    echo json_encode(["status" => "error", "message" => "Missing session_id"]);
    exit();
}

try {
    // 1. Verify participant authorization
    $stmt = $pdo->prepare("
        SELECT s.session_id, d1.user_id AS user_a, d2.user_id AS user_b, 
               s.streaming_device_id AS dev_a_id, s.viewing_device_id AS dev_b_id
        FROM sessions s
        JOIN devices d1 ON s.streaming_device_id = d1.id
        JOIN devices d2 ON s.viewing_device_id = d2.id
        WHERE s.session_id = :session_id
    ");
    $stmt->execute(['session_id' => $sessionId]);
    $session = $stmt->fetch(PDO::FETCH_ASSOC);

    if (!$session) {
        http_response_code(404);
        echo json_encode(["status" => "error", "message" => "Session not found"]);
        exit();
    }
    
    if ($session['user_a'] != $currentUserId && $session['user_b'] != $currentUserId) {
        http_response_code(403);
        echo json_encode(["status" => "error", "message" => "Forbidden"]);
        exit();
    }

    // 2. Lookup existing conversation
    $cstmt = $pdo->prepare("SELECT conversation_id FROM chat_conversations WHERE session_id = :session_id LIMIT 1");
    $cstmt->execute(['session_id' => $sessionId]);
    $conv = $cstmt->fetch(PDO::FETCH_ASSOC);

    if ($conv) {
        echo json_encode(["status" => "success", "conversation_id" => $conv['conversation_id']]);
        exit();
    }

    // 3. Create new conversation
    function generateUuid() {
        return sprintf('%04x%04x-%04x-%04x-%04x-%04x%04x%04x',
            mt_rand(0, 0xffff), mt_rand(0, 0xffff), mt_rand(0, 0xffff),
            mt_rand(0, 0x0fff) | 0x4000, mt_rand(0, 0x3fff) | 0x8000,
            mt_rand(0, 0xffff), mt_rand(0, 0xffff), mt_rand(0, 0xffff)
        );
    }
    $conversationId = generateUuid();
    
    // Get device_uid for devices
    $dstmt = $pdo->prepare("SELECT id, device_uid FROM devices WHERE id IN (?, ?)");
    $dstmt->execute([$session['dev_a_id'], $session['dev_b_id']]);
    $devs = $dstmt->fetchAll(PDO::FETCH_KEY_PAIR);
    
    $devA_uid = $devs[$session['dev_a_id']] ?? null;
    $devB_uid = $devs[$session['dev_b_id']] ?? null;

    $insert = $pdo->prepare("
        INSERT INTO chat_conversations (conversation_id, session_id, user_a_id, user_b_id, device_a_id, device_b_id, status) 
        VALUES (:conversation_id, :session_id, :user_a, :user_b, :device_a, :device_b, 'ACTIVE')
    ");
    try {
        $insert->execute([
            'conversation_id' => $conversationId,
            'session_id' => $sessionId,
            'user_a' => $session['user_a'],
            'user_b' => $session['user_b'],
            'device_a' => $devA_uid,
            'device_b' => $devB_uid
        ]);
        echo json_encode(["status" => "success", "conversation_id" => $conversationId]);
    } catch (\PDOException $e) {
        if ($e->getCode() == 23000) {
            $cstmt->execute(['session_id' => $sessionId]);
            $conv = $cstmt->fetch(PDO::FETCH_ASSOC);
            echo json_encode(["status" => "success", "conversation_id" => $conv['conversation_id']]);
        } else {
            throw $e;
        }
    }
} catch (\PDOException $e) {
    error_log("[CHAT API ERROR] " . $e->getMessage());
    http_response_code(500);
    echo json_encode(["status" => "error", "message" => "Database error"]);
}
?>
