<?php
/**
 * Test API endpoints for subscriptions offline via isolated PHP processes
 */

echo "Testing Subscription API Endpoints...\n";

function runEndpoint(string $file, array $query = [], array $post = [], array $session = []): array {
    $script = "
        \$_SERVER['REQUEST_METHOD'] = " . var_export($post ? 'POST' : 'GET', true) . ";
        \$_GET = " . var_export($query, true) . ";
        \$_POST = " . var_export($post, true) . ";
        session_start();
        \$_SESSION = " . var_export($session, true) . ";
        require " . var_export($file, true) . ";
    ";
    
    $descriptors = [
        0 => ["pipe", "r"],
        1 => ["pipe", "w"],
        2 => ["pipe", "w"]
    ];
    $proc = proc_open('C:\\xampp\\php\\php.exe', $descriptors, $pipes);
    fwrite($pipes[0], "<?php " . $script);
    fclose($pipes[0]);
    $out = stream_get_contents($pipes[1]);
    $err = stream_get_contents($pipes[2]);
    fclose($pipes[1]);
    fclose($pipes[2]);
    $code = proc_close($proc);
    
    return [
        'code' => $code,
        'stdout' => $out,
        'stderr' => $err,
        'json' => json_decode($out, true)
    ];
}

// 1. Test plans.php
$resPlans = runEndpoint(__DIR__ . '/../backend/api/subscriptions/plans.php');
if (!isset($resPlans['json']['status']) || $resPlans['json']['status'] !== 'success' || count($resPlans['json']['plans']) < 4) {
    echo "[FAIL] plans.php output invalid: " . $resPlans['stdout'] . "\n";
    exit(1);
}
echo "[PASS] plans.php returned " . count($resPlans['json']['plans']) . " active plans.\n";

// 2. Test current.php unauthorized
$resCurrUnauth = runEndpoint(__DIR__ . '/../backend/api/subscriptions/current.php');
if (($resCurrUnauth['json']['status'] ?? '') !== 'error') {
    echo "[FAIL] current.php unauthorized did not return error: " . $resCurrUnauth['stdout'] . "\n";
    exit(1);
}
echo "[PASS] current.php rejected unauthenticated request.\n";

// 3. Test current.php authorized
$resCurrAuth = runEndpoint(__DIR__ . '/../backend/api/subscriptions/current.php', ['user_id' => 1], [], ['user_id' => 1]);
if (($resCurrAuth['json']['status'] ?? '') !== 'success' || !isset($resCurrAuth['json']['entitlements'])) {
    echo "[FAIL] current.php authorized failed: " . $resCurrAuth['stdout'] . "\n";
    exit(1);
}
echo "[PASS] current.php resolved entitlements for user 1 successfully.\n";

// 4. Test that callers cannot request another account's entitlement data
$resCurrOtherUser = runEndpoint(__DIR__ . '/../backend/api/subscriptions/current.php', ['user_id' => 2], [], ['user_id' => 1]);
if (($resCurrOtherUser['json']['status'] ?? '') !== 'success'
    || (int)($resCurrOtherUser['json']['account']['id'] ?? 0) !== 1) {
    echo "[FAIL] current.php did not bind the subscription to the authenticated user: " . $resCurrOtherUser['stdout'] . "\n";
    exit(1);
}
echo "[PASS] current.php ignored a forged user ID and returned only the authenticated account.\n";

// 5. Test features.php
$resFeatures = runEndpoint(__DIR__ . '/../backend/api/subscriptions/features.php', ['user_id' => 1], [], ['user_id' => 1]);
if (($resFeatures['json']['status'] ?? '') !== 'success' || !isset($resFeatures['json']['entitlements']['can_record'])) {
    echo "[FAIL] features.php failed: " . $resFeatures['stdout'] . "\n";
    exit(1);
}
echo "[PASS] features.php returned valid feature flags.\n";

$monthlyPlanId = 0;
foreach ($resPlans['json']['plans'] as $plan) {
    if (($plan['slug'] ?? '') === 'monthly') {
        $monthlyPlanId = (int)$plan['id'];
        break;
    }
}
if ($monthlyPlanId < 1) {
    echo "[FAIL] plans.php did not return the Monthly Plan.\n";
    exit(1);
}

// 6. Test subscribe.php without payment proof on paid plan
$resSubNoPay = runEndpoint(__DIR__ . '/../backend/api/subscriptions/subscribe.php', [], [
    'user_id' => 1,
    'plan_id' => $monthlyPlanId
], ['user_id' => 1]);
if (($resSubNoPay['json']['status'] ?? '') !== 'error' || strpos($resSubNoPay['json']['message'] ?? '', 'Razorpay') === false) {
    echo "[FAIL] subscribe.php did not enforce payment proof for paid plan: " . $resSubNoPay['stdout'] . "\n";
    exit(1);
}
echo "[PASS] subscribe.php enforced payment verification for paid plan.\n";

echo "All 6 Subscription API Endpoint checks passed!\n";
