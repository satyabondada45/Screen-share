<?php
header('Content-Type: application/json');
require_once __DIR__ . '/../../config/database.php';
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
$orderId = trim((string)($data['razorpay_order_id'] ?? ''));
$paymentId = trim((string)($data['razorpay_payment_id'] ?? ''));
$signature = trim((string)($data['razorpay_signature'] ?? ''));
if ($orderId === '' || $paymentId === '' || $signature === '') {
    http_response_code(400);
    echo json_encode(['status' => 'error', 'message' => 'The payment reference is incomplete.']);
    exit();
}

try {
    if (!RazorpayService::verifyCheckoutSignature($orderId, $paymentId, $signature)) {
        http_response_code(403);
        echo json_encode(['status' => 'error', 'message' => 'Payment signature verification failed.']);
        exit();
    }

    $stmt = $pdo->prepare("
        SELECT `id`, `amount`, `currency`, `status`, `gateway_payment_id`
        FROM `payments`
        WHERE `gateway_order_id` = ? AND `user_id` = ?
        LIMIT 1
    ");
    $stmt->execute([$orderId, $userId]);
    $record = $stmt->fetch(PDO::FETCH_ASSOC);
    if (!$record) {
        http_response_code(404);
        echo json_encode(['status' => 'error', 'message' => 'Payment order was not found for this account.']);
        exit();
    }

    $gatewayOrder = RazorpayService::fetchOrder($orderId);
    $gatewayPayment = RazorpayService::fetchPayment($paymentId);
    $expectedAmount = (int)round((float)$record['amount'] * 100);
    if ((int)($gatewayOrder['amount'] ?? -1) !== $expectedAmount
        || strtoupper((string)($gatewayOrder['currency'] ?? '')) !== strtoupper((string)$record['currency'])
        || (int)($gatewayPayment['amount'] ?? -1) !== $expectedAmount
        || strtoupper((string)($gatewayPayment['currency'] ?? '')) !== strtoupper((string)$record['currency'])
        || ($gatewayPayment['order_id'] ?? '') !== $orderId
        || ($gatewayPayment['status'] ?? '') !== 'captured') {
        http_response_code(402);
        echo json_encode(['status' => 'error', 'message' => 'Razorpay has not confirmed a captured payment for this plan.']);
        exit();
    }

    $result = SubscriptionService::activateVerifiedRazorpayPayment(
        $orderId,
        $paymentId,
        $gatewayPayment
    );
    echo json_encode(['status' => 'success', 'subscription' => $result]);
} catch (Throwable $error) {
    error_log('Razorpay payment verification failed for user ' . $userId . ': ' . $error->getMessage());
    http_response_code(503);
    echo json_encode(['status' => 'error', 'message' => 'Payment verification could not be completed. Please retry.']);
}