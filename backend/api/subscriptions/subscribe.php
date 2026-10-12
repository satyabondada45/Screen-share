<?php
// backend/api/subscriptions/subscribe.php
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
$planId = !empty($data['plan_id']) ? (int)$data['plan_id'] : 0;
$transactionId = trim((string)($data['transaction_id'] ?? ''));
$paymentMethod = trim((string)($data['payment_method'] ?? 'card'));

if (!$userId) {
    http_response_code(401);
    echo json_encode(['status' => 'error', 'message' => 'Unauthorized: Please log in or provide user_id.']);
    exit();
}

if (!$planId) {
    http_response_code(400);
    echo json_encode(['status' => 'error', 'message' => 'Missing plan_id.']);
    exit();
}

try {
    $plan = SubscriptionService::getPlan($planId);
    if (!$plan || $plan['status'] !== 'active') {
        http_response_code(404);
        echo json_encode(['status' => 'error', 'message' => 'Plan not found or inactive.']);
        exit();
    }

    $price = (float)$plan['price'];

    if ($price <= 0) {
        // ₹0 plan (Welcome or Free)
        if (($plan['slug'] ?? '') === 'welcome' || $plan['name'] === 'Welcome Plan') {
            $assigned = SubscriptionService::assignWelcomePlanIfEligible((int)$userId);
            if (!$assigned) {
                http_response_code(400);
                echo json_encode([
                    'status' => 'error',
                    'message' => 'Welcome Plan has already been claimed for this account.'
                ]);
                exit();
            }
            echo json_encode([
                'status' => 'success',
                'message' => 'Welcome Plan activated successfully.',
                'subscription' => $assigned
            ]);
            exit();
        } else {
            // Free plan
            echo json_encode([
                'status' => 'success',
                'message' => 'Free Plan is active.',
                'subscription' => SubscriptionService::getEffectiveFreePlan((int)$userId)
            ]);
            exit();
        }
    } else {
        // Paid plan: requires valid transaction proof
        if (empty($transactionId)) {
            http_response_code(402);
            echo json_encode([
                'status' => 'error',
                'message' => 'Payment required: Missing verified transaction_id.'
            ]);
            exit();
        }

        $result = SubscriptionService::activatePaidSubscription(
            (int)$userId,
            $planId,
            $transactionId,
            $price,
            $paymentMethod
        );

        echo json_encode([
            'status' => 'success',
            'message' => "Successfully subscribed to {$plan['name']}.",
            'details' => $result
        ]);
        exit();
    }
} catch (Throwable $e) {
    http_response_code(500);
    echo json_encode([
        'status' => 'error',
        'message' => 'Subscription failed: ' . $e->getMessage()
    ]);
}
