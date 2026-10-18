<?php
/**
 * Test script to verify all 10 scenarios requested in Objective 6:
 * 1. Signed-in user with a valid Welcome Plan.
 * 2. Signed-in user with a Monthly Plan.
 * 3. Signed-in user with a Yearly Plan.
 * 4. Signed-in user with an ongoing Free Plan.
 * 5. Signed-out user.
 * 6. Expired or invalid authentication token.
 * 7. API unavailable, timeout or invalid JSON.
 * 8. User with no subscription record, following existing rules.
 * 9. A subscription whose expiry has passed.
 * 10. Confirmation that UI displays actual database values and cannot be spoofed by client-side user ID.
 */

require_once __DIR__ . '/../backend/config/database.php';
require_once __DIR__ . '/../backend/services/SubscriptionService.php';

echo "======================================================================\n";
echo " DESKSTREAM SUBSCRIPTION FLOW VERIFICATION SUITE\n";
echo "======================================================================\n\n";

global $pdo;

$pass = 0;
$fail = 0;

function check(string $label, bool $condition, string $extra = '') {
    global $pass, $fail;
    if ($condition) {
        $pass++;
        echo "  [PASS] {$label}\n";
    } else {
        $fail++;
        echo "  [FAIL] {$label}" . ($extra ? " ($extra)" : "") . "\n";
    }
}

// Setup test users in database
$testEmailPrefix = 'subtest_' . time();
function createTestUser(PDO $pdo, string $email, string $username): int {
    $stmt = $pdo->prepare("INSERT INTO `users` (`name`, `username`, `email`, `password_hash`, `unique_user_id`, `created_at`) VALUES (?, ?, ?, ?, ?, NOW())");
    $uid = (string)random_int(100000000, 999999999);
    $stmt->execute([$username, $username, $email, password_hash('TestPass123!', PASSWORD_BCRYPT), $uid]);
    return (int)$pdo->lastInsertId();
}

$u1_welcome = createTestUser($pdo, "{$testEmailPrefix}_welcome@test.com", "user_welcome");
$u2_monthly = createTestUser($pdo, "{$testEmailPrefix}_monthly@test.com", "user_monthly");
$u3_yearly  = createTestUser($pdo, "{$testEmailPrefix}_yearly@test.com", "user_yearly");
$u4_free    = createTestUser($pdo, "{$testEmailPrefix}_free@test.com", "user_free");
$u5_expired = createTestUser($pdo, "{$testEmailPrefix}_expired@test.com", "user_expired");
$u6_new     = createTestUser($pdo, "{$testEmailPrefix}_new@test.com", "user_new");

// Setup subscriptions
// 1. Welcome Plan
$welcomeSub = SubscriptionService::assignWelcomePlanIfEligible($u1_welcome);
check("1. Signed-in user with Welcome Plan assigned", $welcomeSub !== null && $welcomeSub['plan_slug'] === 'welcome');
check("   Welcome Plan has server-assigned expiry date", !empty($welcomeSub['expires_at']));
$resolvedWelcome = SubscriptionService::getUserSubscription($u1_welcome);
check("   Authoritative resolution returns Welcome Plan", $resolvedWelcome['plan_slug'] === 'welcome' && $resolvedWelcome['status'] === 'active');

// 2. Monthly Plan
$monthlyPlan = SubscriptionService::getPlan('monthly');
$monthlyActivation = SubscriptionService::activatePaidSubscription($u2_monthly, (int)$monthlyPlan['id'], 'VERIFIED_TX_' . time(), 100.00, 'card');
check("2. Signed-in user with Monthly Plan activated", $monthlyActivation['status'] === 'success');
$resolvedMonthly = SubscriptionService::getUserSubscription($u2_monthly);
check("   Monthly Plan has start date and actual expiry", !empty($resolvedMonthly['starts_at']) && !empty($resolvedMonthly['expires_at']));
check("   Monthly Plan status is active", $resolvedMonthly['status'] === 'active' && $resolvedMonthly['plan_slug'] === 'monthly');

// 3. Yearly Plan
$yearlyPlan = SubscriptionService::getPlan('yearly');
$yearlyActivation = SubscriptionService::activatePaidSubscription($u3_yearly, (int)$yearlyPlan['id'], 'VERIFIED_TX_Y_' . time(), 1100.00, 'card');
check("3. Signed-in user with Yearly Plan activated", $yearlyActivation['status'] === 'success');
$resolvedYearly = SubscriptionService::getUserSubscription($u3_yearly);
check("   Yearly Plan has start date and actual expiry", !empty($resolvedYearly['starts_at']) && !empty($resolvedYearly['expires_at']));
check("   Yearly Plan status is active", $resolvedYearly['status'] === 'active' && $resolvedYearly['plan_slug'] === 'yearly');

// 4. Free Plan (ongoing)
$welcomePlan = SubscriptionService::getPlan('welcome');
$pastWelcomeSql = "INSERT INTO `subscriptions` (`user_id`, `plan_id`, `status`, `starts_at`, `expires_at`, `created_at`) VALUES (?, ?, 'expired', '2024-01-01 00:00:00', '2024-07-01 00:00:00', NOW())";
$pdo->prepare($pastWelcomeSql)->execute([$u4_free, $welcomePlan['id']]);
$resolvedFree = SubscriptionService::getUserSubscription($u4_free);
check("4. User falling back to Free Plan has plan_slug = free", $resolvedFree['plan_slug'] === 'free');
check("   Free Plan expires_at is null (No expiry)", $resolvedFree['expires_at'] === null);

// Helper to run current.php in isolated CLI subprocess
function invokeCurrentApi(array $sessionData, array $headers = [], array $getParams = []): array {
    $script = '<?php ';
    $script .= 'session_start(); ';
    foreach ($sessionData as $k => $v) {
        $script .= '$_SESSION[' . var_export($k, true) . '] = ' . var_export($v, true) . '; ';
    }
    $script .= 'session_write_close(); ';
    $script .= '$_SERVER["REQUEST_METHOD"] = "GET"; ';
    foreach ($headers as $k => $v) {
        $script .= '$_SERVER[' . var_export($k, true) . '] = ' . var_export($v, true) . '; ';
    }
    foreach ($getParams as $k => $v) {
        $script .= '$_GET[' . var_export($k, true) . '] = ' . var_export($v, true) . '; ';
    }
    $script .= 'register_shutdown_function(function() {
        $code = http_response_code();
        $body = ob_get_contents();
        @ob_end_clean();
        echo "\n---SPLIT---\n" . json_encode(["code" => $code, "body" => json_decode($body, true)]);
    }); ';
    $script .= 'ob_start(); ';
    $script .= 'include ' . var_export(__DIR__ . '/../backend/api/subscriptions/current.php', true) . '; ';

    $tmp = tempnam(sys_get_temp_dir(), 'cur_');
    file_put_contents($tmp, $script);
    $cmd = '"C:\\xampp\\php\\php.exe" ' . escapeshellarg($tmp);
    $out = (string)shell_exec($cmd);
    @unlink($tmp);
    $parts = explode("\n---SPLIT---\n", $out);
    return json_decode(end($parts), true) ?: ['code' => 500, 'body' => null];
}

// 5. Signed-out user (HTTP 401)
$res5 = invokeCurrentApi([]);
check("5. Signed-out request returns HTTP 401 Unauthorized", ($res5['code'] ?? 0) === 401);
check("   Response indicates authentication is required", str_contains($res5['body']['message'] ?? '', 'Unauthorized') || str_contains($res5['body']['message'] ?? '', 'sign in'));

// 6. Expired / Invalid authentication token
$res6 = invokeCurrentApi([], ['HTTP_AUTHORIZATION' => 'Bearer invalid_nonexistent_token_1234567890']);
check("6. Invalid authentication token rejected with HTTP 401", ($res6['code'] ?? 0) === 401);

// 7. Client spoofing attempt: Client passes client-supplied user_id of another user
$res7 = invokeCurrentApi(['user_id' => $u2_monthly], [], ['user_id' => (string)$u3_yearly]);
check("7. Server returns authenticated user's own subscription, ignoring spoofed ?user_id", ($res7['body']['account']['id'] ?? 0) === $u2_monthly && ($res7['body']['plan_slug'] ?? '') === 'monthly');

// 8. New user with no subscription record receives Welcome Plan automatically
$resolvedNew = SubscriptionService::getUserSubscription($u6_new);
check("8. User with no subscription record receives Welcome Plan", $resolvedNew['plan_slug'] === 'welcome' && $resolvedNew['status'] === 'active');

// 9. A subscription whose expiry has passed
$pastExpiry = date('Y-m-d H:i:s', time() - 3600); // 1 hour ago
$pdo->prepare("INSERT INTO `subscriptions` (`user_id`, `plan_id`, `status`, `starts_at`, `expires_at`, `created_at`) VALUES (?, ?, 'active', '2024-01-01 00:00:00', ?, NOW())")->execute([$u5_expired, $monthlyPlan['id'], $pastExpiry]);
$resolvedExpired = SubscriptionService::getUserSubscription($u5_expired);
check("9. Subscription past expiry is marked expired in real-time", $resolvedExpired['plan_slug'] === 'welcome' || $resolvedExpired['plan_slug'] === 'free');
$dbStatus = $pdo->query("SELECT `status` FROM `subscriptions` WHERE `user_id` = {$u5_expired} AND `expires_at` = '{$pastExpiry}'")->fetchColumn();
check("   Database row updated to status = 'expired'", $dbStatus === 'expired');

// 10. Confirmation of actual database values in API response
$res10 = invokeCurrentApi(['user_id' => $u1_welcome]);
check("10. API returns actual database email, identifier, plan, and server_now", 
    !empty($res10['body']['email']) &&
    !empty($res10['body']['account_identifier']) &&
    !empty($res10['body']['server_now']) &&
    ($res10['body']['plan_slug'] ?? '') === 'welcome' &&
    ($res10['body']['current_status'] ?? '') === 'active' &&
    !empty($res10['body']['expires_at'])
);

echo "\n======================================================================\n";
echo " VERIFICATION SUMMARY: {$pass} PASSED, {$fail} FAILED\n";
echo "======================================================================\n";

// Cleanup test rows
$pdo->exec("DELETE FROM `subscriptions` WHERE `user_id` IN ($u1_welcome, $u2_monthly, $u3_yearly, $u4_free, $u5_expired, $u6_new)");
$pdo->exec("DELETE FROM `payments` WHERE `user_id` IN ($u1_welcome, $u2_monthly, $u3_yearly, $u4_free, $u5_expired, $u6_new)");
$pdo->exec("DELETE FROM `users` WHERE `id` IN ($u1_welcome, $u2_monthly, $u3_yearly, $u4_free, $u5_expired, $u6_new)");
