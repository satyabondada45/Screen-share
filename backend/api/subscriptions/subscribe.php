<?php
header('Content-Type: application/json');
header('Access-Control-Allow-Methods: POST, OPTIONS');
header('Access-Control-Allow-Headers: Content-Type, X-CSRF-Token');

if (($_SERVER['REQUEST_METHOD'] ?? '') === 'OPTIONS') {
    exit(0);
}

if (session_status() === PHP_SESSION_NONE) {
    session_start();
}
require_once __DIR__ . '/../../services/SubscriptionService.php';

if (($_SERVER['REQUEST_METHOD'] ?? '') !== 'POST') {
    http_response_code(405);
    echo json_encode(['status' => 'error', 'message' => 'Method not allowed.']);
    exit();
}

$userId = filter_var(
    $_SESSION['user_id'] ?? null,
    FILTER_VALIDATE_INT,
    ['options' => ['min_range' => 1]]
);
if ($userId === false) {
    http_response_code(401);
    echo json_encode(['status' => 'error', 'message' => 'Sign in to manage your subscription.']);
    exit();
}

$data = json_decode(file_get_contents('php://input'), true) ?: $_POST;
if (!empty($data['user_id']) && (int)$data['user_id'] !== (int)$userId) {
    http_response_code(403);
    echo json_encode(['status' => 'error', 'message' => 'The requested account does not match the signed-in user.']);
    exit();
}

$planId = filter_var($data['plan_id'] ?? null, FILTER_VALIDATE_INT, ['options' => ['min_range' => 1]]);
if ($planId === false) {
    http_response_code(400);
    echo json_encode(['status' => 'error', 'message' => 'A valid plan_id is required.']);
    exit();
}

try {
    $plan = SubscriptionService::getPlan((int)$planId);
    if (!$plan || ($plan['status'] ?? '') !== 'active') {
        http_response_code(404);
        echo json_encode(['status' => 'error', 'message' => 'The selected plan is unavailable.']);
        exit();
    }
    if (in_array($plan['slug'] ?? '', ['monthly', 'yearly'], true)) {
        http_response_code(409);
        echo json_encode([
            'status' => 'error',
            'message' => 'Paid plans require a Razorpay order and server-side payment verification.',
            'payment_endpoint' => '../payments/create.php'
        ]);
        exit();
    }
    if (($plan['slug'] ?? '') === 'welcome') {
        http_response_code(409);
        echo json_encode([
            'status' => 'error',
            'message' => 'The Welcome Plan is assigned once during eligible account registration.'
        ]);
        exit();
    }
    if (($plan['slug'] ?? '') === 'free') {
        echo json_encode([
            'status' => 'success',
            'message' => 'Free Plan entitlements are active.',
            'subscription' => SubscriptionService::getEffectiveFreePlan((int)$userId),
            'entitlements' => SubscriptionService::getUserEntitlements((int)$userId)
        ]);
        exit();
    }

    http_response_code(400);
    echo json_encode(['status' => 'error', 'message' => 'This plan cannot be activated through this endpoint.']);
} catch (Throwable $error) {
    error_log('Subscription request failed for user ' . $userId . ': ' . $error->getMessage());
    http_response_code(500);
    echo json_encode(['status' => 'error', 'message' => 'Subscription request could not be completed.']);
}
