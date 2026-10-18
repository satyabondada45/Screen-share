<?php

function paymentRequestUserId(): int
{
    if (session_status() === PHP_SESSION_NONE) {
        session_start();
    }
    $userId = filter_var(
        $_SESSION['user_id'] ?? null,
        FILTER_VALIDATE_INT,
        ['options' => ['min_range' => 1]]
    );
    if ($userId === false) {
        http_response_code(401);
        echo json_encode(['status' => 'error', 'message' => 'Sign in to make a payment.']);
        exit();
    }

    $expectedToken = $_SESSION['payment_csrf_token'] ?? '';
    $providedToken = $_SERVER['HTTP_X_CSRF_TOKEN'] ?? '';
    if (!is_string($expectedToken) || $expectedToken === ''
        || !is_string($providedToken) || !hash_equals($expectedToken, $providedToken)) {
        http_response_code(403);
        echo json_encode(['status' => 'error', 'message' => 'Invalid request token. Reload the subscription page and retry.']);
        exit();
    }
    return (int)$userId;
}

function paymentJsonInput(): array
{
    $data = json_decode(file_get_contents('php://input'), true);
    if (!is_array($data)) {
        http_response_code(400);
        echo json_encode(['status' => 'error', 'message' => 'A valid JSON request is required.']);
        exit();
    }
    return $data;
}
