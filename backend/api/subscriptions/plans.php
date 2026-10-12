<?php
// backend/api/subscriptions/plans.php
header('Content-Type: application/json');
header('Access-Control-Allow-Origin: *');
header('Access-Control-Allow-Methods: GET, OPTIONS');
header('Access-Control-Allow-Headers: Content-Type, Authorization');

if ($_SERVER['REQUEST_METHOD'] === 'OPTIONS') {
    exit(0);
}

require_once __DIR__ . '/../../services/SubscriptionService.php';

try {
    $plans = SubscriptionService::getActivePlans();
    $formatted = array_map(function ($plan) {
        $features = $plan['features_decoded'] ?? [];
        return [
            'id' => (int)$plan['id'],
            'name' => $plan['name'],
            'slug' => $plan['slug'] ?? '',
            'description' => $plan['description'] ?? '',
            'price' => (float)$plan['price'],
            'price_formatted' => '₹' . number_format((float)$plan['price'], 2),
            'duration_value' => (int)($plan['duration_value'] ?? $plan['duration_days']),
            'duration_unit' => $plan['duration_unit'] ?? 'days',
            'duration_days' => (int)$plan['duration_days'],
            'concurrent_sessions' => (int)$plan['concurrent_sessions'],
            'max_devices' => (int)$plan['max_devices'],
            'recording' => !empty($features['recording']),
            'reverse_control' => !empty($features['reverse_control']),
            'max_quality' => $features['max_quality'] ?? 'full',
            'status' => $plan['status']
        ];
    }, $plans);

    echo json_encode([
        'status' => 'success',
        'count' => count($formatted),
        'plans' => $formatted
    ]);
} catch (Throwable $e) {
    http_response_code(500);
    echo json_encode([
        'status' => 'error',
        'message' => 'Failed to load subscription plans: ' . $e->getMessage()
    ]);
}
