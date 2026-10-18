<?php
header('Content-Type: application/json');
require_once __DIR__ . '/../../config/database.php';
require_once __DIR__ . '/../../services/SubscriptionService.php';
require_once __DIR__ . '/../../services/RazorpayService.php';

if (($_SERVER['REQUEST_METHOD'] ?? '') !== 'POST') {
    http_response_code(405);
    echo json_encode(['status' => 'error', 'message' => 'Method not allowed.']);
    exit();
}

$rawBody = file_get_contents('php://input');
$signature = (string)($_SERVER['HTTP_X_RAZORPAY_SIGNATURE'] ?? '');
if (!is_string($rawBody) || !RazorpayService::verifyWebhookSignature($rawBody, $signature)) {
    http_response_code(403);
    echo json_encode(['status' => 'error', 'message' => 'Webhook signature verification failed.']);
    exit();
}

$event = json_decode($rawBody, true);
$eventType = $event['event'] ?? '';
if ($eventType !== 'payment.captured') {
    http_response_code(200);
    echo json_encode(['status' => 'ignored']);
    exit();
}

$entity = $event['payload']['payment']['entity'] ?? null;
if (!is_array($entity) || ($entity['status'] ?? '') !== 'captured'
    || empty($entity['id']) || empty($entity['order_id'])) {
    http_response_code(400);
    echo json_encode(['status' => 'error', 'message' => 'Captured payment details are missing.']);
    exit();
}

try {
    $orderId = (string)$entity['order_id'];
    $paymentId = (string)$entity['id'];
    $gatewayOrder = RazorpayService::fetchOrder($orderId);
    $gatewayPayment = RazorpayService::fetchPayment($paymentId);
    if (($gatewayPayment['order_id'] ?? '') !== $orderId
        || ($gatewayPayment['status'] ?? '') !== 'captured'
        || (int)($gatewayPayment['amount'] ?? -1) !== (int)($gatewayOrder['amount'] ?? -2)
        || strtoupper((string)($gatewayPayment['currency'] ?? '')) !== strtoupper((string)($gatewayOrder['currency'] ?? ''))) {
        http_response_code(402);
        echo json_encode(['status' => 'error', 'message' => 'Gateway payment does not match its order.']);
        exit();
    }

    $result = SubscriptionService::activateVerifiedRazorpayPayment($orderId, $paymentId, $gatewayPayment);
    echo json_encode(['status' => 'success', 'subscription' => $result]);
} catch (Throwable $error) {
    error_log('Razorpay webhook activation failed: ' . $error->getMessage());
    http_response_code(503);
    echo json_encode(['status' => 'error', 'message' => 'Webhook activation could not be completed.']);
}