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

$conversationId = $_GET['conversation_id'] ?? null;

if (!$conversationId) {
    echo json_encode(["status" => "error", "message" => "Missing conversation_id"]);
    exit();
}

try {
    // 1. Verify participant authorization
    $stmt = $pdo->prepare("SELECT user_a_id, user_b_id FROM chat_conversations WHERE conversation_id = ?");
    $stmt->execute([$conversationId]);
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

    $hstmt = $pdo->prepare("
        SELECT message_id, conversation_id, session_id, sender_user_id, receiver_user_id, sender_device_id, receiver_device_id, message_type, message_text, file_transfer_id, status, sent_at, delivered_at, read_at
        FROM chat_messages 
        WHERE conversation_id = ? 
        ORDER BY sent_at ASC, id ASC
    ");
    $hstmt->execute([$conversationId]);
    $messages = $hstmt->fetchAll(PDO::FETCH_ASSOC);

    echo json_encode(["status" => "success", "messages" => $messages]);
} catch (\PDOException $e) {
    error_log("[CHAT API ERROR] " . $e->getMessage());
    http_response_code(500);
    echo json_encode(["status" => "error", "message" => "Database error"]);
}
?>
