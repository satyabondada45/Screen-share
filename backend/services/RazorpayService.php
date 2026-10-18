<?php

final class RazorpayService
{
    private static function credentials(): array
    {
        $keyId = getenv('RAZORPAY_KEY_ID') ?: '';
        $keySecret = getenv('RAZORPAY_KEY_SECRET') ?: '';
        if ($keyId === '' || $keySecret === '') {
            throw new RuntimeException('Razorpay credentials are not configured on the server.');
        }
        return [$keyId, $keySecret];
    }

    public static function publicKeyId(): string
    {
        return self::credentials()[0];
    }

    private static function request(string $method, string $path, ?array $body = null): array
    {
        [$keyId, $keySecret] = self::credentials();
        if (!function_exists('curl_init')) {
            throw new RuntimeException('The server PHP cURL extension is required for Razorpay.');
        }
        $handle = curl_init('https://api.razorpay.com/v1/' . ltrim($path, '/'));
        $options = [
            CURLOPT_RETURNTRANSFER => true,
            CURLOPT_CUSTOMREQUEST => $method,
            CURLOPT_USERPWD => $keyId . ':' . $keySecret,
            CURLOPT_HTTPAUTH => CURLAUTH_BASIC,
            CURLOPT_CONNECTTIMEOUT => 5,
            CURLOPT_TIMEOUT => 15,
            CURLOPT_HTTPHEADER => ['Content-Type: application/json']
        ];
        if ($body !== null) {
            $options[CURLOPT_POSTFIELDS] = json_encode($body, JSON_THROW_ON_ERROR);
        }
        curl_setopt_array($handle, $options);
        $response = curl_exec($handle);
        $status = (int)curl_getinfo($handle, CURLINFO_RESPONSE_CODE);
        $error = curl_error($handle);
        curl_close($handle);

        if (!is_string($response)) {
            error_log('Razorpay API transport failed: ' . $error);
            throw new RuntimeException('Razorpay could not be reached. Please retry.');
        }
        $decoded = json_decode($response, true);
        if ($status < 200 || $status >= 300 || !is_array($decoded)) {
            error_log('Razorpay API rejected the request with HTTP ' . $status);
            throw new RuntimeException('Razorpay rejected the payment request.');
        }
        return $decoded;
    }

    public static function createOrder(int $amountPaise, string $receipt, array $notes): array
    {
        return self::request('POST', 'orders', [
            'amount' => $amountPaise,
            'currency' => 'INR',
            'receipt' => $receipt,
            'notes' => $notes
        ]);
    }

    public static function fetchOrder(string $orderId): array
    {
        return self::request('GET', 'orders/' . rawurlencode($orderId));
    }

    public static function fetchPayment(string $paymentId): array
    {
        return self::request('GET', 'payments/' . rawurlencode($paymentId));
    }

    public static function verifyCheckoutSignature(string $orderId, string $paymentId, string $signature): bool
    {
        [, $keySecret] = self::credentials();
        $expected = hash_hmac('sha256', $orderId . '|' . $paymentId, $keySecret);
        return hash_equals($expected, $signature);
    }

    public static function verifyWebhookSignature(string $body, string $signature): bool
    {
        $secret = getenv('RAZORPAY_WEBHOOK_SECRET') ?: '';
        if ($secret === '' || $signature === '') {
            return false;
        }
        return hash_equals(hash_hmac('sha256', $body, $secret), $signature);
    }
}
