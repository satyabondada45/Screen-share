<?php
// backend/api/subscriptions/features.php
header('Content-Type: application/json');
header('Access-Control-Allow-Origin: *');
header('Access-Control-Allow-Methods: GET, OPTIONS');
header('Access-Control-Allow-Headers: Content-Type, Authorization');

if ($_SERVER['REQUEST_METHOD'] === 'OPTIONS') {
    exit(0);
}

if (session_status() === PHP_SESSION_NONE) {
    session_start();
}

require_once __DIR__ . '/../../services/SubscriptionService.php';

$sessionUserId = filter_var(
    $_SESSION['user_id'] ?? null,
    FILTER_VALIDATE_INT,
    ['options' => ['min_range' => 1]]
);
if ($sessionUserId === false) {
    http_response_code(401);
    echo json_encode([
        'status' => 'error',
        'message' => 'Unauthorized: Please sign in to view subscription entitlements.'
    ]);
    exit();
}
$requestedUserId = filter_var(
    $_GET['user_id'] ?? $sessionUserId,
    FILTER_VALIDATE_INT,
    ['options' => ['min_range' => 1]]
);
if ($requestedUserId === false || (int)$requestedUserId !== (int)$sessionUserId) {
    http_response_code(403);
    echo json_encode(['status' => 'error', 'message' => 'You may only view your own entitlements.']);
    exit();
}
$userId = (int)$sessionUserId;

try {
    $entitlements = SubscriptionService::getUserEntitlements($userId);
    echo json_encode([
        'status' => 'success',
        'entitlements' => $entitlements
    ]);
} catch (Throwable $e) {
    http_response_code(500);
    echo json_encode([
        'status' => 'error',
        'message' => 'Failed to resolve user features: ' . $e->getMessage()
    ]);
}
