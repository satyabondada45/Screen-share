<?php
// tests/test_welcome_plan_and_searchable_selector.php
// Comprehensive verification of Welcome Plan duration configuration, immutable expiry,
// customer search API, bounded results, and assignment workflow.

require_once __DIR__ . '/../backend/config/database.php';
require_once __DIR__ . '/../backend/services/SubscriptionService.php';
require_once __DIR__ . '/../admin/includes/db.php';
require_once __DIR__ . '/../admin/includes/auth.php';

$pdo = admin_db();

echo "======================================================================\n";
echo " DESKSTREAM WELCOME PLAN, IMMUTABLE EXPIRY & CUSTOMER SEARCH TEST SUITE\n";
echo "======================================================================\n\n";

$passes = 0;
$fails = 0;

function assertTest(string $case, string $desc, bool $condition, string $detail = '') {
    global $passes, $fails;
    if ($condition) {
        $passes++;
        echo "  [PASS] {$case}: {$desc}\n";
    } else {
        $fails++;
        echo "  [FAIL] {$case}: {$desc}" . ($detail ? " ($detail)" : "") . "\n";
    }
}

// Helper to call api_customers.php directly in buffer
function callCustomerSearchApi(array $params, int $adminUserId): array {
    $_SESSION['admin_logged_in'] = true;
    $_SESSION['admin_user_id'] = $adminUserId;
    $_GET = $params;
    ob_start();
    include __DIR__ . '/../admin/api_customers.php';
    $raw = ob_get_clean();
    return json_decode($raw ?: '', true) ?: [];
}

// Helper to render subscriptions.php in buffer
function renderSubscriptionsPage(array $getParams, int $adminUserId): string {
    $_SESSION['admin_logged_in'] = true;
    $_SESSION['admin_user_id'] = $adminUserId;
    $_GET = $getParams;
    $_SERVER['REQUEST_METHOD'] = 'GET';
    ob_start();
    include __DIR__ . '/../admin/subscriptions.php';
    return ob_get_clean() ?: '';
}

// ── ADMIN USER RESOLUTION ──────────────────────────────────────────────────
$adminId = 1;
if (SubscriptionService::tableExists($pdo, 'admin_users')) {
    $aRow = $pdo->query("SELECT id FROM `admin_users` ORDER BY id ASC LIMIT 1")->fetch(PDO::FETCH_ASSOC);
    if ($aRow) $adminId = (int)$aRow['id'];
}

// ── GET OR ENSURE WELCOME PLAN ──────────────────────────────────────────────
$wPlan = SubscriptionService::getPlan('welcome');
if (!$wPlan) {
    die("FATAL: Welcome Plan does not exist in database.\n");
}
$wPlanId = (int)$wPlan['id'];

// ── TEST 1: Register Account with Welcome duration set to 6 months ──────────
echo "--- TEST 1: Register Account with Welcome Duration = 6 Months ---\n";
// Set to 6 months
$pdo->prepare("UPDATE `plans` SET `duration_value` = 6, `duration_unit` = 'months', `duration_days` = 180 WHERE `id` = ?")->execute([$wPlanId]);

$custAEmail = 'cust_a_' . bin2hex(random_bytes(4)) . '@example.com';
$custAName = 'Customer Alpha';
$custAPass = 'SecretPass123!';
$custAUid = (string)random_int(100000000, 999999999);

// Execute registration via register.php logic
$pdo->beginTransaction();
$insStmt = $pdo->prepare("INSERT INTO users (name, username, email, password_hash, unique_user_id, created_at) VALUES (?, ?, ?, ?, ?, NOW())");
$insStmt->execute([$custAName, $custAName, $custAEmail, password_hash($custAPass, PASSWORD_DEFAULT), $custAUid]);
$custAId = (int)$pdo->lastInsertId();
$subA = SubscriptionService::assignWelcomePlanIfEligible($custAId, true);
$pdo->commit();

assertTest("Test 1.1", "Customer A created with active Welcome subscription", $subA !== null && ($subA['plan_slug'] ?? '') === 'welcome');
assertTest("Test 1.2", "Customer A subscription has saved starts_at", !empty($subA['starts_at']));
assertTest("Test 1.3", "Customer A subscription has saved expires_at", !empty($subA['expires_at']));

$startATs = strtotime($subA['starts_at']);
$expATs = strtotime($subA['expires_at']);
$monthsDiffA = (int)round(($expATs - $startATs) / (30.4375 * 86400));
assertTest("Test 1.4", "Customer A expiry spans exactly ~6 calendar months", $monthsDiffA === 6, "months: $monthsDiffA");
$savedCustAExp = $subA['expires_at'];

// ── TEST 2: Change Welcome duration to 3 months ─────────────────────────────
echo "\n--- TEST 2: Change Welcome Duration Configuration to 3 Months ---\n";
// Simulate welcome duration setting update via admin logic
$newMonths = 3;
$oldMonths = (int)($pdo->query("SELECT duration_value FROM `plans` WHERE `id` = $wPlanId")->fetchColumn() ?: 6);
$pdo->prepare("UPDATE `plans` SET `duration_value` = ?, `duration_unit` = 'months', `duration_days` = ? WHERE `id` = ?")
    ->execute([$newMonths, $newMonths * 30, $wPlanId]);
admin_log_action($pdo, 'welcome_duration_update', "Changed Welcome Plan duration for new registrations from {$oldMonths} months to {$newMonths} months.");

$refetchedMonths = (int)$pdo->query("SELECT duration_value FROM `plans` WHERE `id` = $wPlanId")->fetchColumn();
assertTest("Test 2.1", "Welcome Plan duration updated in database to 3 months", $refetchedMonths === 3);

$auditCheck = $pdo->query("SELECT details FROM `audit_logs` WHERE `action` = 'welcome_duration_update' ORDER BY id DESC LIMIT 1")->fetchColumn();
assertTest("Test 2.2", "Audit log recorded duration change", str_contains((string)$auditCheck, '3 months'));

// ── TEST 3: Verify Customer A's saved expiry & entitlements remain unchanged ─
echo "\n--- TEST 3: Verify Customer A's Saved Expiry & Entitlements Unchanged ---\n";
$refetchedSubA = SubscriptionService::getUserSubscription($custAId);
assertTest("Test 3.1", "Customer A retains original saved expiry timestamp exactly", $refetchedSubA['expires_at'] === $savedCustAExp, "saved: {$savedCustAExp}, current: {$refetchedSubA['expires_at']}");

$entitlementsA = SubscriptionService::getUserEntitlements($custAId);
assertTest("Test 3.2", "Customer A entitlements remain full (recording allowed)", ($entitlementsA['can_record'] ?? false) === true);
assertTest("Test 3.3", "Customer A entitlements remain full (reverse control allowed)", ($entitlementsA['can_reverse_control'] ?? false) === true);
assertTest("Test 3.4", "Customer A entitlements remain full quality", ($entitlementsA['max_quality'] ?? '') === 'full');

// ── TEST 4: Register Customer B under 3-month duration ───────────────────────
echo "\n--- TEST 4: Register Customer B Under 3-Month Welcome Duration ---\n";
$custBEmail = 'cust_b_' . bin2hex(random_bytes(4)) . '@example.com';
$custBName = 'Customer Beta';
$custBPass = 'SecretPass123!';
$custBUid = (string)random_int(100000000, 999999999);

$pdo->beginTransaction();
$insStmt->execute([$custBName, $custBName, $custBEmail, password_hash($custBPass, PASSWORD_DEFAULT), $custBUid]);
$custBId = (int)$pdo->lastInsertId();
$subB = SubscriptionService::assignWelcomePlanIfEligible($custBId, true);
$pdo->commit();

assertTest("Test 4.1", "Customer B created with active Welcome subscription", $subB !== null && ($subB['plan_slug'] ?? '') === 'welcome');
$startBTs = strtotime($subB['starts_at']);
$expBTs = strtotime($subB['expires_at']);
$monthsDiffB = (int)round(($expBTs - $startBTs) / (30.4375 * 86400));
assertTest("Test 4.2", "Customer B expiry spans exactly ~3 calendar months", $monthsDiffB === 3, "months: $monthsDiffB");
$savedCustBExp = $subB['expires_at'];

// ── TEST 5: Change configuration to 12 months & verify neither changes ──────
echo "\n--- TEST 5: Re-change Duration to 12 Months & Verify Neither Customer Changes ---\n";
$pdo->prepare("UPDATE `plans` SET `duration_value` = 12, `duration_unit` = 'months', `duration_days` = 365 WHERE `id` = ?")->execute([$wPlanId]);

$checkCustA = SubscriptionService::getUserSubscription($custAId);
$checkCustB = SubscriptionService::getUserSubscription($custBId);

assertTest("Test 5.1", "Customer A expiry still unmodified after second config change", $checkCustA['expires_at'] === $savedCustAExp);
assertTest("Test 5.2", "Customer B expiry still unmodified after second config change", $checkCustB['expires_at'] === $savedCustBExp);

// Restore default 6 months
$pdo->prepare("UPDATE `plans` SET `duration_value` = 6, `duration_unit` = 'months', `duration_days` = 180 WHERE `id` = ?")->execute([$wPlanId]);

// ── TEST 6: Customer Search API Tests ───────────────────────────────────────
echo "\n--- TEST 6: Search Customer by ID, Name, Email via api_customers.php ---\n";
// 6.1 Search by ID
$dataId = callCustomerSearchApi(['q' => (string)$custAId, 'limit' => '25'], $adminId);
assertTest("Test 6.1", "Search by customer ID finds Customer A", !empty($dataId['users']) && (int)$dataId['users'][0]['id'] === $custAId);

// 6.2 Search by Name
$dataName = callCustomerSearchApi(['q' => 'Customer Alpha', 'limit' => '25'], $adminId);
assertTest("Test 6.2", "Search by customer Name finds Customer A", !empty($dataName['users']) && (int)$dataName['users'][0]['id'] === $custAId);

// 6.3 Search by Email
$dataEmail = callCustomerSearchApi(['q' => $custBEmail, 'limit' => '25'], $adminId);
assertTest("Test 6.3", "Search by customer Email finds Customer B", !empty($dataEmail['users']) && (int)$dataEmail['users'][0]['id'] === $custBId);

// ── TEST 7: Dataset with 100+ accounts does not load into dropdown ──────────
echo "\n--- TEST 7: Dataset with 100+ Accounts & UI DOM Scalability ---\n";
$totalUsersCount = (int)$pdo->query("SELECT COUNT(*) FROM `users`")->fetchColumn();
if ($totalUsersCount < 100) {
    $needed = 100 - $totalUsersCount;
    $pdo->beginTransaction();
    $bulk = $pdo->prepare("INSERT INTO `users` (`name`, `username`, `email`, `password_hash`, `unique_user_id`, `created_at`) VALUES (?, ?, ?, 'dummy', ?, NOW())");
    for ($i = 0; $i < $needed; $i++) {
        $uIdStr = (string)random_int(100000000, 999999999);
        $bulk->execute(["Bulk User $i", "bulk_user_$i", "bulk_{$i}_" . bin2hex(random_bytes(3)) . "@example.com", $uIdStr]);
    }
    $pdo->commit();
    $totalUsersCount = (int)$pdo->query("SELECT COUNT(*) FROM `users`")->fetchColumn();
}
assertTest("Test 7.1", "Database has >= 100 customer accounts", $totalUsersCount >= 100, "count: $totalUsersCount");

// Render subscriptions.php page in isolated process
$subPageHtml = renderSubscriptionsPage([], $adminId);

// Check if customer_search_input is present and that massive <option> list is NOT present
$hasSearchInput = str_contains($subPageHtml, 'id="customer_search_input"');
$optionMatches = preg_match_all('/<option value="\d+"/', $subPageHtml);
assertTest("Test 7.2", "Searchable customer selector input exists on Subscriptions page", $hasSearchInput);
assertTest("Test 7.3", "UI does not load 100+ customer accounts into one dropdown", $optionMatches < 20, "options found: $optionMatches");

// ── TEST 8: Pagination and Bounded Search Results ────────────────────────────
echo "\n--- TEST 8: Verify Bounded Search Results and Pagination ---\n";
$p1Data = callCustomerSearchApi(['q' => '', 'limit' => '15', 'page' => '1'], $adminId);
assertTest("Test 8.1", "API returns exactly bounded limit of 15 accounts", count($p1Data['users'] ?? []) === 15);
assertTest("Test 8.2", "API indicates has_more is true when more accounts exist", ($p1Data['has_more'] ?? false) === true);

$p2Data = callCustomerSearchApi(['q' => '', 'limit' => '15', 'page' => '2'], $adminId);
$p1Ids = array_column($p1Data['users'] ?? [], 'id');
$p2Ids = array_column($p2Data['users'] ?? [], 'id');
$overlap = array_intersect($p1Ids, $p2Ids);
assertTest("Test 8.3", "Page 2 returns distinct accounts without overlap", empty($overlap) && count($p2Data['users'] ?? []) === 15);

// ── TEST 9: Assign Plan Through Searchable Selector Workflow ────────────────
echo "\n--- TEST 9: Assign Plan Through Searchable Selector Workflow ---\n";
$mPlan = SubscriptionService::getPlan('monthly');
$mPlanId = (int)$mPlan['id'];

// Simulate admin manual activation
$activation = SubscriptionService::activateManualSubscription(
    $custAId,
    $mPlanId,
    $adminId,
    'Manual upgrade via searchable selector'
);

assertTest("Test 9.1", "Manual plan assignment returns success", ($activation['status'] ?? '') === 'success');
assertTest("Test 9.2", "Assigned plan has valid expires_at timestamp", !empty($activation['expires_at']));

$reloadedSub = SubscriptionService::getUserSubscription($custAId);
assertTest("Test 9.3", "Assigned Monthly Plan persisted in database as active", ($reloadedSub['plan_slug'] ?? '') === 'monthly');

// ── TEST 10: Invalid Customer ID & Unauthorized Assignment ───────────────────
echo "\n--- TEST 10: Invalid Customer ID and Unauthorized Assignment Requests ---\n";
$invalidCaught = false;
try {
    SubscriptionService::activateManualSubscription(999999999, $mPlanId, $adminId, 'Invalid test');
} catch (InvalidArgumentException $e) {
    $invalidCaught = true;
}
assertTest("Test 10.1", "Assigning plan to invalid customer ID rejected safely", $invalidCaught);

$unauthCaught = false;
try {
    SubscriptionService::activateManualSubscription($custAId, $mPlanId, 999999999, 'Unauth test');
} catch (InvalidArgumentException $e) {
    $unauthCaught = true;
}
assertTest("Test 10.2", "Unauthorized administrator ID rejected safely", $unauthCaught);

// ── TEST 11: Idempotency Against Duplicate Welcome Subscriptions ────────────
echo "\n--- TEST 11: Idempotency Against Duplicate Welcome Subscriptions ---\n";
$dupResult = SubscriptionService::assignWelcomePlanIfEligible($custBId);
assertTest("Test 11.1", "Retrying Welcome Plan assignment on Customer B returns null", $dupResult === null);

$countSubsB = (int)$pdo->query("SELECT COUNT(*) FROM `subscriptions` WHERE `user_id` = {$custBId}")->fetchColumn();
assertTest("Test 11.2", "No duplicate subscription created for Customer B", $countSubsB === 1);

// ── TEST 12: Desktop App Subscription Endpoint Stored Expiry ────────────────
echo "\n--- TEST 12: Desktop App Subscription Endpoint Stored Expiry ---\n";
$subEndpointData = SubscriptionService::getUserSubscription($custBId);
assertTest("Test 12.1", "Subscription resolution returns Customer B's Welcome Plan", ($subEndpointData['plan_slug'] ?? '') === 'welcome');
assertTest("Test 12.2", "Endpoint returns originally stored 3-month expiry timestamp", $subEndpointData['expires_at'] === $savedCustBExp);

// ── TEST 13: Paid Subscriptions & Remote Sessions Integrity ─────────────────
echo "\n--- TEST 13: Paid Subscriptions and System Integrity ---\n";
$custAEntitlements = SubscriptionService::getUserEntitlements($custAId);
assertTest("Test 13.1", "Customer A (Monthly Plan) maintains reverse_control", ($custAEntitlements['can_reverse_control'] ?? false) === true);
assertTest("Test 13.2", "Customer A (Monthly Plan) maintains full max_quality", ($custAEntitlements['max_quality'] ?? '') === 'full');

$freeUserEntitlements = SubscriptionService::getEffectiveFreePlan(999999);
assertTest("Test 13.3", "Free Plan correctly disables reverse_control", $freeUserEntitlements['features_decoded']['reverse_control'] === false);
assertTest("Test 13.4", "Free Plan correctly limits quality to 720p", $freeUserEntitlements['features_decoded']['max_quality'] === '720p');

echo "\n======================================================================\n";
echo " TEST RESULTS: {$passes} PASSED, {$fails} FAILED\n";
echo "======================================================================\n";

if ($fails > 0) {
    exit(1);
}
