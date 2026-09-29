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

$data = json_decode(file_get_contents("php://input"), true);
$messageId = $data['message_id'] ?? null;
$conversationId = $data['conversation_id'] ?? null;
$sessionId = $data['session_id'] ?? null;
$messageText = $data['message_text'] ?? '';

if (!$messageId || !$conversationId || !$sessionId || trim($messageText) === '') {
    echo json_encode(["status" => "error", "message" => "Missing required fields"]);
    exit();
}

try {
    // 1. Verify participant authorization
    $stmt = $pdo->prepare("SELECT user_a_id, user_b_id, device_a_id, device_b_id FROM chat_conversations WHERE conversation_id = ? AND session_id = ?");
    $stmt->execute([$conversationId, $sessionId]);
    $conv = $stmt->fetch(PDO::FETCH_ASSOC);

    if (!$conv) {
        http_response_code(404);
        echo json_encode(["status" => "error", "message" => "Conversation not found"]);
        exit();
    }
    
    if ($conv['user_a_id'] != $currentUserId && $conv['user_b_id'] != $currentUserId) {
        http_response_code(403);
        echo json_encode(["status" => "error", "message" => "Forbidden"]);
        exit();
    }

    $isUserA = ($conv['user_a_id'] == $currentUserId);
    $senderUserId = $isUserA ? $conv['user_a_id'] : $conv['user_b_id'];
    $receiverUserId = $isUserA ? $conv['user_b_id'] : $conv['user_a_id'];
    $senderDeviceId = $isUserA ? $conv['device_a_id'] : $conv['device_b_id'];
    $receiverDeviceId = $isUserA ? $conv['device_b_id'] : $conv['device_a_id'];

    $insert = $pdo->prepare("
        INSERT INTO chat_messages 
        (message_id, conversation_id, session_id, sender_user_id, receiver_user_id, sender_device_id, receiver_device_id, message_type, message_text, status) 
        VALUES (:message_id, :conversation_id, :session_id, :sender_user, :receiver_user, :sender_device, :receiver_device, 'TEXT', :message_text, 'SENT')
    ");

    try {
        $insert->execute([
            'message_id' => $messageId,
            'conversation_id' => $conversationId,
            'session_id' => $sessionId,
            'sender_user' => $senderUserId,
            'receiver_user' => $receiverUserId,
            'sender_device' => $senderDeviceId,
            'receiver_device' => $receiverDeviceId,
            'message_text' => $messageText
        ]);
        echo json_encode(["status" => "success", "message_id" => $messageId]);
    } catch (\PDOException $e) {
        if ($e->getCode() == 23000) {
            echo json_encode(["status" => "success", "message_id" => $messageId, "duplicate" => true]);
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
