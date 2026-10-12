<?php
// backend/api/subscriptions/cancel.php
header('Content-Type: application/json');
header('Access-Control-Allow-Origin: *');
header('Access-Control-Allow-Methods: POST, OPTIONS');
header('Access-Control-Allow-Headers: Content-Type, Authorization');

if ($_SERVER['REQUEST_METHOD'] === 'OPTIONS') {
    exit(0);
}

if (session_status() === PHP_SESSION_NONE) {
    session_start();
}

require_once __DIR__ . '/../../services/SubscriptionService.php';

if ($_SERVER['REQUEST_METHOD'] !== 'POST') {
    http_response_code(405);
    echo json_encode(['status' => 'error', 'message' => 'Method not allowed']);
    exit();
}

$rawInput = file_get_contents('php://input');
$data = json_decode($rawInput, true) ?: $_POST;

$userId = !empty($data['user_id']) ? (int)$data['user_id'] : ($_SESSION['user_id'] ?? null);

if (!$userId) {
    http_response_code(401);
    echo json_encode(['status' => 'error', 'message' => 'Unauthorized: Please log in or provide user_id.']);
    exit();
}

try {
    global $pdo;
    $stmt = $pdo->prepare("
        UPDATE `subscriptions` 
        SET `status` = 'cancelled' 
        WHERE `user_id` = ? AND `status` = 'active'
    ");
    $stmt->execute([(int)$userId]);
    $affected = $stmt->rowCount();

    echo json_encode([
        'status' => 'success',
        'message' => $affected > 0 ? 'Active subscription cancelled.' : 'No active paid subscription to cancel.',
        'effective_plan' => SubscriptionService::getEffectiveFreePlan((int)$userId)
    ]);
} catch (Throwable $e) {
    http_response_code(500);
    echo json_encode([
        'status' => 'error',
        'message' => 'Cancellation failed: ' . $e->getMessage()
    ]);
}
