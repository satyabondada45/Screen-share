<?php
header('Content-Type: application/json');
require_once __DIR__ . '/../../services/SubscriptionService.php';
require_once __DIR__ . '/../../services/RazorpayService.php';
require_once __DIR__ . '/_auth.php';

if (($_SERVER['REQUEST_METHOD'] ?? '') !== 'POST') {
    http_response_code(405);
    echo json_encode(['status' => 'error', 'message' => 'Method not allowed.']);
    exit();
}

$userId = paymentRequestUserId();
$data = paymentJsonInput();
$planId = filter_var($data['plan_id'] ?? null, FILTER_VALIDATE_INT, ['options' => ['min_range' => 1]]);
if ($planId === false) {
    http_response_code(400);
    echo json_encode(['status' => 'error', 'message' => 'Select a valid paid plan.']);
    exit();
}

try {
    $plan = SubscriptionService::getPlan((int)$planId);
    if (!$plan || ($plan['status'] ?? '') !== 'active'
        || !in_array($plan['slug'] ?? '', ['monthly', 'yearly'], true)
        || (float)$plan['price'] <= 0) {
        http_response_code(400);
        echo json_encode(['status' => 'error', 'message' => 'The selected paid plan is unavailable.']);
        exit();
    }

    $amountPaise = (int)round((float)$plan['price'] * 100);
    if ($amountPaise < 1) {
        throw new InvalidArgumentException('The selected plan price is invalid.');
    }
    $receipt = 'ds' . $userId . bin2hex(random_bytes(12));
    $order = RazorpayService::createOrder($amountPaise, $receipt, [
        'user_id' => (string)$userId,
        'plan_id' => (string)$plan['id']
    ]);
    $orderId = $order['id'] ?? null;
    if (!is_string($orderId) || $orderId === ''
        || (int)($order['amount'] ?? 0) !== $amountPaise
        || strtoupper((string)($order['currency'] ?? '')) !== 'INR'
        || ($order['status'] ?? '') !== 'created') {
        throw new RuntimeException('Razorpay returned an invalid order response.');
    }

    $features = $plan['features_decoded'] ?? [];
    $snapshot = [
        'name' => (string)$plan['name'],
        'slug' => (string)$plan['slug'],
        'price' => (string)$plan['price'],
        'duration_value' => (int)$plan['duration_value'],
        'duration_unit' => (string)$plan['duration_unit'],
        'duration_days' => (int)$plan['duration_days'],
        'concurrent_sessions' => (int)$plan['concurrent_sessions'],
        'max_devices' => (int)$plan['max_devices'],
        'features' => is_array($features) ? $features : []
    ];
    $stmt = $pdo->prepare("
        INSERT INTO `payments`
            (`user_id`, `amount`, `transaction_id`, `payment_method`, `status`,
             `gateway_order_id`, `currency`, `plan_id`, `plan_snapshot`, `created_at`)
        VALUES (?, ?, ?, 'razorpay', 'pending', ?, 'INR', ?, ?, UTC_TIMESTAMP())
    ");
    $stmt->execute([
        $userId,
        $plan['price'],
        $orderId,
        $orderId,
        $plan['id'],
        json_encode($snapshot, JSON_THROW_ON_ERROR)
    ]);

    echo json_encode([
        'status' => 'success',
        'key_id' => RazorpayService::publicKeyId(),
        'order_id' => $orderId,
        'amount' => $amountPaise,
        'currency' => 'INR',
        'plan_name' => $plan['name']
    ]);
} catch (Throwable $error) {
    error_log('Razorpay order creation failed for user ' . $userId . ': ' . $error->getMessage());
    http_response_code(503);
    echo json_encode(['status' => 'error', 'message' => 'Unable to create a verified payment order. Please retry later.']);
}