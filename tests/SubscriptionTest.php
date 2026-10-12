<?php
/**
 * DeskStream Offline Test Suite: Subscription Plan Creation, Persistence, and Entitlements
 */

require_once __DIR__ . '/../backend/config/database.php';
require_once __DIR__ . '/../backend/services/SubscriptionService.php';
require_once __DIR__ . '/../admin/includes/db.php';

echo "======================================================================\n";
echo " DESKSTREAM OFFLINE SUBSCRIPTION TEST SUITE\n";
echo " Running against local database: screen_share_db\n";
echo "======================================================================\n\n";

$passCount = 0;
$failCount = 0;

function assertTest(string $description, bool $condition, string $details = '') {
    global $passCount, $failCount;
    if ($condition) {
        $passCount++;
        echo "  [PASS] {$description}\n";
    } else {
        $failCount++;
        echo "  [FAIL] {$description}" . ($details ? " -> {$details}" : "") . "\n";
    }
}

// ----------------------------------------------------------------------
// 1. Database Schema & Migration Verification
// ----------------------------------------------------------------------
echo "--- TEST GROUP 1: Database Schema Verification ---\n";
$cols = admin_table_columns($pdo, 'plans');
assertTest("Column 'name' exists in plans", in_array('name', $cols, true));
assertTest("Column 'slug' exists in plans", in_array('slug', $cols, true));
assertTest("Column 'price' exists in plans", in_array('price', $cols, true));
assertTest("Column 'duration_unit' exists in plans", in_array('duration_unit', $cols, true));
assertTest("Column 'duration_value' exists in plans", in_array('duration_value', $cols, true));
assertTest("Column 'concurrent_sessions' exists in plans", in_array('concurrent_sessions', $cols, true));
assertTest("Column 'features' exists in plans", in_array('features', $cols, true));
assertTest("Column 'status' exists in plans", in_array('status', $cols, true));

// ----------------------------------------------------------------------
// 2. The Four Canonical Plans in Database
// ----------------------------------------------------------------------
echo "\n--- TEST GROUP 2: Canonical Plans in Database ---\n";
$welcomePlan = SubscriptionService::getPlan('welcome');
$freePlan = SubscriptionService::getPlan('free');
$monthlyPlan = SubscriptionService::getPlan('monthly');
$yearlyPlan = SubscriptionService::getPlan('yearly');

assertTest("Welcome Plan exists and is active", $welcomePlan && $welcomePlan['status'] === 'active');
assertTest("Welcome Plan price is ₹0.00", (float)($welcomePlan['price'] ?? -1) === 0.0);
assertTest("Welcome Plan duration is 6 months", ($welcomePlan['duration_value'] ?? 0) == 6 && ($welcomePlan['duration_unit'] ?? '') === 'months');

assertTest("Free Plan exists and is active", $freePlan && $freePlan['status'] === 'active');
assertTest("Free Plan price is ₹0.00", (float)($freePlan['price'] ?? -1) === 0.0);
assertTest("Free Plan duration is unlimited", ($freePlan['duration_unit'] ?? '') === 'unlimited');
assertTest("Free Plan concurrent_sessions is 1", (int)($freePlan['concurrent_sessions'] ?? 0) === 1);
$freeFeatures = $freePlan['features_decoded'] ?? [];
assertTest("Free Plan recording is disabled in features", empty($freeFeatures['recording']));
assertTest("Free Plan reverse_control is disabled in features", empty($freeFeatures['reverse_control']));
assertTest("Free Plan max_quality is 720p", ($freeFeatures['max_quality'] ?? '') === '720p');

assertTest("Monthly Plan exists and is active", $monthlyPlan && $monthlyPlan['status'] === 'active');
assertTest("Monthly Plan price is ₹100.00", (float)($monthlyPlan['price'] ?? 0) === 100.0);
assertTest("Monthly Plan duration is 30 days", ($monthlyPlan['duration_value'] ?? 0) == 30 && ($monthlyPlan['duration_unit'] ?? '') === 'days');

assertTest("Yearly Plan exists and is active", $yearlyPlan && $yearlyPlan['status'] === 'active');
assertTest("Yearly Plan price is ₹1,100.00", (float)($yearlyPlan['price'] ?? 0) === 1100.0);
assertTest("Yearly Plan duration is 1 year", ($yearlyPlan['duration_value'] ?? 0) == 1 && ($yearlyPlan['duration_unit'] ?? '') === 'years');

// ----------------------------------------------------------------------
// 3. Plan CRUD & Persistence Verification
// ----------------------------------------------------------------------
echo "\n--- TEST GROUP 3: Plan CRUD and Persistence ---\n";
// Create custom plan
$customPlanName = 'Test Custom ' . bin2hex(random_bytes(4));
$insertStmt = $pdo->prepare("
    INSERT INTO plans (name, slug, description, price, duration_days, duration_value, duration_unit, max_devices, concurrent_sessions, status, features, created_at)
    VALUES (?, ?, 'Test custom description', 250.00, 60, 60, 'days', 3, 2, 'active', ?, NOW())
");
$customFeatures = json_encode(['recording' => true, 'reverse_control' => false, 'max_quality' => '1080p']);
$insertStmt->execute([$customPlanName, 'custom-' . time(), $customFeatures]);
$customPlanId = (int)$pdo->lastInsertId();

$fetchedCustom = SubscriptionService::getPlan($customPlanId);
assertTest("Newly created custom plan persisted to database", $fetchedCustom && $fetchedCustom['name'] === $customPlanName);
assertTest("Custom plan features JSON stored and decoded correctly", ($fetchedCustom['features_decoded']['max_quality'] ?? '') === '1080p');

// Edit custom plan
$pdo->prepare("UPDATE plans SET price = 299.00, status = 'inactive' WHERE id = ?")->execute([$customPlanId]);
$editedCustom = SubscriptionService::getPlan($customPlanId);
assertTest("Editing plan updates price in database", (float)$editedCustom['price'] === 299.00);
assertTest("Deactivating plan updates status to 'inactive' in database", $editedCustom['status'] === 'inactive');

// Clean up custom plan
$pdo->prepare("DELETE FROM plans WHERE id = ?")->execute([$customPlanId]);
assertTest("Plan deletion cleaned up test row", SubscriptionService::getPlan($customPlanId) === null);

// ----------------------------------------------------------------------
// 4. Welcome Plan Assignment & Idempotency
// ----------------------------------------------------------------------
echo "\n--- TEST GROUP 4: Welcome Plan Assignment & Idempotency ---\n";
// Create isolated test user
$testEmail = 'test_welcome_' . bin2hex(random_bytes(4)) . '@test.local';
$userStmt = $pdo->prepare("
    INSERT INTO users (name, username, email, password_hash, unique_user_id, role, status, created_at)
    VALUES ('Welcome Test User', 'welcome_test', ?, 'hash', ?, 'user', 'active', NOW())
");
$testUid = (string)random_int(100000000, 999999999);
$userStmt->execute([$testEmail, $testUid]);
$testUserId = (int)$pdo->lastInsertId();

// 1st assignment (new account)
$sub1 = SubscriptionService::assignWelcomePlanIfEligible($testUserId);
assertTest("New account successfully receives Welcome Plan", $sub1 !== null && $sub1['plan_name'] === 'Welcome Plan');
assertTest("Welcome Plan status is active", ($sub1['status'] ?? '') === 'active');

// Verify calendar-month arithmetic (approx 180-184 days depending on months)
$starts = new DateTime($sub1['starts_at']);
$expires = new DateTime($sub1['expires_at']);
$diff = $starts->diff($expires);
assertTest("Welcome Plan expires in exactly 6 months", $diff->m === 6 || ($diff->y === 0 && $diff->m === 6));

// 2nd assignment attempt (idempotency check)
$sub2 = SubscriptionService::assignWelcomePlanIfEligible($testUserId);
assertTest("Second assignment attempt is rejected (strictly assigned once)", $sub2 === null);

// ----------------------------------------------------------------------
// 5. Entitlement Resolution (Welcome Plan vs Free Plan)
// ----------------------------------------------------------------------
echo "\n--- TEST GROUP 5: Entitlement Enforcement Logic ---\n";
// Test user currently on Welcome Plan
$entWelcome = SubscriptionService::getUserEntitlements($testUserId);
assertTest("Welcome user has can_record = true", $entWelcome['can_record'] === true);
assertTest("Welcome user has can_reverse_control = true", $entWelcome['can_reverse_control'] === true);
assertTest("Welcome user has max_quality = full", $entWelcome['max_quality'] === 'full');
assertTest("Welcome user has max_concurrent_sessions >= 5", $entWelcome['max_concurrent_sessions'] >= 5);

$authRecWelcome = SubscriptionService::authorizeAction($testUserId, 'recording');
assertTest("Welcome user recording action authorized", $authRecWelcome['authorized'] === true);

$authRevWelcome = SubscriptionService::authorizeAction($testUserId, 'reverse_control');
assertTest("Welcome user reverse_control action authorized", $authRevWelcome['authorized'] === true);

$authQualWelcome = SubscriptionService::authorizeAction($testUserId, 'quality', ['height' => 1080]);
assertTest("Welcome user 1080p quality authorized", $authQualWelcome['authorized'] === true);

// Create second test user with NO Welcome Plan (Free Plan user)
$freeUserEmail = 'test_free_' . bin2hex(random_bytes(4)) . '@test.local';
$freeUid = (string)random_int(100000000, 999999999);
$pdo->prepare("
    INSERT INTO users (name, username, email, password_hash, unique_user_id, role, status, created_at)
    VALUES ('Free Test User', 'free_test', ?, 'hash', ?, 'user', 'active', NOW())
")->execute([$freeUserEmail, $freeUid]);
$freeUserId = (int)$pdo->lastInsertId();

// Prevent welcome assignment by creating an expired historical record
$pdo->prepare("
    INSERT INTO subscriptions (user_id, plan_id, status, starts_at, expires_at, created_at)
    VALUES (?, ?, 'expired', NOW() - INTERVAL 7 MONTH, NOW() - INTERVAL 1 MONTH, NOW())
")->execute([$freeUserId, $welcomePlan['id']]);

$entFree = SubscriptionService::getUserEntitlements($freeUserId);
assertTest("Free user resolved plan is 'Free Plan'", $entFree['plan_name'] === 'Free Plan');
assertTest("Free user has can_record = false", $entFree['can_record'] === false);
assertTest("Free user has can_reverse_control = false", $entFree['can_reverse_control'] === false);
assertTest("Free user has max_quality = 720p", $entFree['max_quality'] === '720p');
assertTest("Free user has max_concurrent_sessions = 1", $entFree['max_concurrent_sessions'] === 1);

$authRecFree = SubscriptionService::authorizeAction($freeUserId, 'recording');
assertTest("Free user recording blocked by authorization layer", $authRecFree['authorized'] === false && $authRecFree['code'] === 'RECORDING_DISABLED');

$authRevFree = SubscriptionService::authorizeAction($freeUserId, 'reverse_control');
assertTest("Free user reverse_control blocked by authorization layer", $authRevFree['authorized'] === false && $authRevFree['code'] === 'REVERSE_CONTROL_DISABLED');

$authQualHigh = SubscriptionService::authorizeAction($freeUserId, 'quality', ['height' => 1080]);
assertTest("Free user 1080p streaming rejected (capped at 720p)", $authQualHigh['authorized'] === false && $authQualHigh['code'] === 'QUALITY_CAPPED');

$authQual720 = SubscriptionService::authorizeAction($freeUserId, 'quality', ['height' => 720]);
assertTest("Free user 720p streaming authorized", $authQual720['authorized'] === true);

// ----------------------------------------------------------------------
// 6. Concurrent Session Enforcement
// ----------------------------------------------------------------------
echo "\n--- TEST GROUP 6: Concurrent Session Enforcement ---\n";
// Create device for free user
$devUid = 'dev_' . bin2hex(random_bytes(6));
$pdo->prepare("
    INSERT INTO devices (user_id, device_uid, system_id, name, is_online, last_seen_at, created_at)
    VALUES (?, ?, ?, 'Free Test Device', 1, NOW(), NOW())
")->execute([$freeUserId, $devUid, $devUid]);
$freeDevId = (int)$pdo->lastInsertId();

// Initially 0 sessions
$session1Check = SubscriptionService::checkConcurrentSessionLimit($freeUserId);
assertTest("Free user allowed to initiate 1st session", $session1Check['authorized'] === true);

// Start 1st active session in DB
$sKey1 = 'sess_' . bin2hex(random_bytes(8));
$pdo->prepare("
    INSERT INTO sessions (session_key, host_device_id, started_at, status)
    VALUES (?, ?, NOW(), 'active')
")->execute([$sKey1, $freeDevId]);

// Attempt to initiate 2nd session
$session2Check = SubscriptionService::checkConcurrentSessionLimit($freeUserId);
assertTest("Free user 2nd concurrent session rejected atomically", $session2Check['authorized'] === false && $session2Check['code'] === 'CONCURRENT_SESSION_LIMIT_EXCEEDED');

// Close 1st session
$pdo->prepare("UPDATE sessions SET status = 'completed', ended_at = NOW() WHERE session_key = ?")->execute([$sKey1]);

// Check again after closing
$sessionAfterClose = SubscriptionService::checkConcurrentSessionLimit($freeUserId);
assertTest("Free user allowed to initiate session after previous session ended", $sessionAfterClose['authorized'] === true);

// ----------------------------------------------------------------------
// 7. Expiration and Automatic Fallback
// ----------------------------------------------------------------------
echo "\n--- TEST GROUP 7: Subscription Expiration & Free Fallback ---\n";
// Create expired Monthly subscription for free user
$pdo->prepare("
    INSERT INTO subscriptions (user_id, plan_id, status, starts_at, expires_at, created_at)
    VALUES (?, ?, 'active', NOW() - INTERVAL 31 DAY, NOW() - INTERVAL 1 DAY, NOW())
")->execute([$freeUserId, $monthlyPlan['id']]);
$expiredSubId = (int)$pdo->lastInsertId();

$resolvedAfterExpiry = SubscriptionService::getUserSubscription($freeUserId);
assertTest("Expired subscription automatically marked expired", $resolvedAfterExpiry['plan_slug'] === 'free');
$checkDbStatus = $pdo->query("SELECT status FROM subscriptions WHERE id = {$expiredSubId}")->fetchColumn();
assertTest("Database subscription row status updated to 'expired'", $checkDbStatus === 'expired');

$entAfterExpiry = SubscriptionService::getUserEntitlements($freeUserId);
assertTest("User entitlements fell back to Free Plan after expiry", $entAfterExpiry['plan_slug'] === 'free' && $entAfterExpiry['can_record'] === false);

// ----------------------------------------------------------------------
// 8. Paid Plan Activation & Payment Proof Requirement
// ----------------------------------------------------------------------
echo "\n--- TEST GROUP 8: Paid Plan Activation & Payment Verification ---\n";
$txId = 'TXN_' . bin2hex(random_bytes(8));
$paidResult = SubscriptionService::activatePaidSubscription($freeUserId, (int)$monthlyPlan['id'], $txId, 100.00, 'card');
assertTest("Paid Monthly Plan successfully activated with payment proof", $paidResult['status'] === 'success');

// Verify payment record in DB
$payRow = $pdo->prepare("SELECT * FROM payments WHERE transaction_id = ?");
$payRow->execute([$txId]);
$payRecord = $payRow->fetch(PDO::FETCH_ASSOC);
assertTest("Payment record exists in database with status 'completed'", $payRecord && $payRecord['status'] === 'completed');

// Verify upgraded entitlements
$entUpgraded = SubscriptionService::getUserEntitlements($freeUserId);
assertTest("User entitlements upgraded to Monthly Plan", $entUpgraded['plan_slug'] === 'monthly' && $entUpgraded['can_record'] === true);

// Clean up test data
$pdo->prepare("DELETE FROM sessions WHERE host_device_id = ?")->execute([$freeDevId]);
$pdo->prepare("DELETE FROM devices WHERE id = ?")->execute([$freeDevId]);
$pdo->prepare("DELETE FROM payments WHERE user_id IN (?, ?)")->execute([$testUserId, $freeUserId]);
$pdo->prepare("DELETE FROM subscriptions WHERE user_id IN (?, ?)")->execute([$testUserId, $freeUserId]);
$pdo->prepare("DELETE FROM users WHERE id IN (?, ?)")->execute([$testUserId, $freeUserId]);

echo "\n======================================================================\n";
echo " TEST SUITE SUMMARY: {$passCount} PASSED, {$failCount} FAILED\n";
echo "======================================================================\n";

if ($failCount > 0) {
    exit(1);
}
