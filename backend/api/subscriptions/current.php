<?php
// backend/api/subscriptions/current.php
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

$userId = !empty($_GET['user_id']) ? (int)$_GET['user_id'] : ($_SESSION['user_id'] ?? null);

if (!$userId) {
    http_response_code(401);
    echo json_encode([
        'status' => 'error',
        'message' => 'Unauthorized: Please provide user_id or log in.'
    ]);
    exit();
}

try {
    $sub = SubscriptionService::getUserSubscription((int)$userId);
    $entitlements = SubscriptionService::getUserEntitlements((int)$userId);

    echo json_encode([
        'status' => 'success',
        'subscription' => [
            'id' => (int)($sub['id'] ?? 0),
            'plan_id' => (int)($sub['plan_id'] ?? 0),
            'plan_name' => $sub['plan_name'] ?? 'Free Plan',
            'plan_slug' => $sub['plan_slug'] ?? 'free',
            'price' => (float)($sub['plan_price'] ?? 0),
            'status' => $sub['status'] ?? 'active',
            'starts_at' => $sub['starts_at'] ?? null,
            'expires_at' => $sub['expires_at'] ?? null,
            'is_fallback' => !empty($sub['is_fallback']),
            'is_unlimited' => empty($sub['expires_at'])
        ],
        'entitlements' => $entitlements
    ]);
} catch (Throwable $e) {
    http_response_code(500);
    echo json_encode([
        'status' => 'error',
        'message' => 'Failed to resolve current subscription: ' . $e->getMessage()
    ]);
}
