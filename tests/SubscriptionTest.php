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

// Calendar-year expiry must preserve the activation month and clamp leap days.
$expiryMethod = new ReflectionMethod(SubscriptionService::class, 'calculatePlanExpiry');
$expiryMethod->setAccessible(true);
$yearEndActivation = new DateTimeImmutable('2024-12-31 10:15:30', new DateTimeZone('UTC'));
$yearEndExpiry = $expiryMethod->invoke(null, $yearEndActivation, 1, 'years');
assertTest(
    "Yearly expiry preserves December 31 activation date",
    $yearEndExpiry->format('Y-m-d H:i:s') === '2025-12-31 10:15:30'
);
$leapDayActivation = new DateTimeImmutable('2024-02-29 10:15:30', new DateTimeZone('UTC'));
$leapDayExpiry = $expiryMethod->invoke(null, $leapDayActivation, 1, 'years');
assertTest(
    "Yearly expiry clamps leap day to February 28 in a non-leap year",
    $leapDayExpiry->format('Y-m-d H:i:s') === '2025-02-28 10:15:30'
);

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
assertTest("Monthly Plan duration is 1 calendar month", ($monthlyPlan['duration_value'] ?? 0) == 1 && ($monthlyPlan['duration_unit'] ?? '') === 'months');

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
assertTest(
    "New account successfully receives Welcome Plan",
    $sub1 !== null && ($sub1['plan_slug'] ?? '') === 'welcome'
);
assertTest("Welcome duration was snapshotted at assignment", ($sub1['duration_value'] ?? null) === 6);
assertTest("Welcome Plan status is active", ($sub1['status'] ?? '') === 'active');

// Verify calendar-month arithmetic with end-of-month clamping.
$starts = new DateTimeImmutable($sub1['starts_at'], new DateTimeZone('UTC'));
$expires = new DateTimeImmutable($sub1['expires_at'], new DateTimeZone('UTC'));
$expectedMonth = $starts->modify('first day of this month')->modify('+6 months');
$expectedDay = min(
    (int)$starts->format('j'),
    (int)$expectedMonth->modify('last day of this month')->format('j')
);
$expectedExpiry = $expectedMonth->setDate(
    (int)$expectedMonth->format('Y'),
    (int)$expectedMonth->format('n'),
    $expectedDay
)->setTime(
    (int)$starts->format('H'),
    (int)$starts->format('i'),
    (int)$starts->format('s')
);
assertTest("Welcome Plan expires exactly six calendar months after activation", $expires == $expectedExpiry);

// 2nd assignment attempt (idempotency check)
$sub2 = SubscriptionService::assignWelcomePlanIfEligible($testUserId);
assertTest("Second assignment attempt is rejected (strictly assigned once)", $sub2 === null);

// Verify plan edits only affect future registrations.
$welcomeExpiryBeforeEdit = $sub1['expires_at'];
$pdo->prepare("UPDATE plans SET duration_value = 3, duration_unit = 'months', duration_days = 90 WHERE id = ?")
    ->execute([$welcomePlan['id']]);
$savedWelcomeEmail = 'test_welcome_new_' . bin2hex(random_bytes(4)) . '@test.local';
$savedWelcomeUid = (string)random_int(100000000, 999999999);
$pdo->prepare("
    INSERT INTO users (name, username, email, password_hash, unique_user_id, role, status, created_at)
    VALUES ('Welcome Snapshot Test', 'welcome_snapshot_test', ?, 'hash', ?, 'user', 'active', NOW())
")->execute([$savedWelcomeEmail, $savedWelcomeUid]);
$newDurationUserId = (int)$pdo->lastInsertId();
$newDurationSub = SubscriptionService::assignWelcomePlanIfEligible($newDurationUserId);
$existingExpiryAfterEdit = $pdo->prepare("SELECT expires_at FROM subscriptions WHERE id = ?");
$existingExpiryAfterEdit->execute([$sub1['id']]);
assertTest(
    "Existing Welcome expiry remains unchanged after duration edit",
    $existingExpiryAfterEdit->fetchColumn() === $welcomeExpiryBeforeEdit
);
$newDurationStart = new DateTimeImmutable($newDurationSub['starts_at'], new DateTimeZone('UTC'));
$newDurationExpiry = new DateTimeImmutable($newDurationSub['expires_at'], new DateTimeZone('UTC'));
$expectedThreeMonth = $newDurationStart->modify('first day of this month')->modify('+3 months');
$expectedThreeDay = min(
    (int)$newDurationStart->format('j'),
    (int)$expectedThreeMonth->modify('last day of this month')->format('j')
);
$expectedThreeExpiry = $expectedThreeMonth->setDate(
    (int)$expectedThreeMonth->format('Y'),
    (int)$expectedThreeMonth->format('n'),
    $expectedThreeDay
)->setTime(
    (int)$newDurationStart->format('H'),
    (int)$newDurationStart->format('i'),
    (int)$newDurationStart->format('s')
);
assertTest(
    "Registrations after Welcome duration edit receive 3 months",
    ($newDurationSub['duration_value'] ?? null) === 3
        && ($newDurationSub['duration_unit'] ?? null) === 'months'
        && $newDurationExpiry == $expectedThreeExpiry
);
$pdo->prepare("
    UPDATE plans
    SET duration_value = ?, duration_unit = ?, duration_days = ?
    WHERE id = ?
")->execute([
    $welcomePlan['duration_value'],
    $welcomePlan['duration_unit'],
    $welcomePlan['duration_days'],
    $welcomePlan['id']
]);

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
assertTest("Free user device registration is not limited to one device", $entFree['max_devices'] > 1);

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

// A session initiated by this account on another user's host still consumes its one slot.
$otherUserEmail = 'test_session_host_' . bin2hex(random_bytes(4)) . '@test.local';
$otherUserUid = (string)random_int(100000000, 999999999);
$pdo->prepare("
    INSERT INTO users (name, username, email, password_hash, unique_user_id, role, status, created_at)
    VALUES ('Session Host Test', 'session_host_test', ?, 'hash', ?, 'user', 'active', NOW())
")->execute([$otherUserEmail, $otherUserUid]);
$otherUserId = (int)$pdo->lastInsertId();
$otherDeviceUid = 'dev_' . bin2hex(random_bytes(6));
$pdo->prepare("
    INSERT INTO devices (user_id, device_uid, system_id, name, is_online, last_seen_at, created_at)
    VALUES (?, ?, ?, 'Other Free Test Device', 1, NOW(), NOW())
")->execute([$otherUserId, $otherDeviceUid, $otherDeviceUid]);
$otherDeviceId = (int)$pdo->lastInsertId();
$requestToken = bin2hex(random_bytes(16));
$pdo->prepare("
    INSERT INTO connection_requests
        (request_token, requester_user_id, requester_system_id, requester_name, target_user_id,
         target_system_id, status, created_at, expires_at)
    VALUES (?, ?, ?, 'Free Test User', ?, ?, 'accepted', UTC_TIMESTAMP(), UTC_TIMESTAMP() + INTERVAL 60 SECOND)
")->execute([$requestToken, $freeUserId, $devUid, $otherUserId, $otherDeviceUid]);
$connectionRequestId = (int)$pdo->lastInsertId();
$crossDeviceSessionKey = 'sess_' . bin2hex(random_bytes(8));
$pdo->prepare("
    INSERT INTO sessions
        (session_key, host_device_id, requester_user_id, connection_request_id, started_at, status)
    VALUES (?, ?, ?, ?, UTC_TIMESTAMP(), 'active')
")->execute([$crossDeviceSessionKey, $otherDeviceId, $freeUserId, $connectionRequestId]);
$crossDeviceCheck = SubscriptionService::checkConcurrentSessionLimit($freeUserId);
assertTest(
    "Free session limit counts the account as requester on another user's device",
    $crossDeviceCheck['authorized'] === false
        && $crossDeviceCheck['code'] === 'CONCURRENT_SESSION_LIMIT_EXCEEDED'
);
$pdo->prepare("DELETE FROM sessions WHERE connection_request_id = ?")->execute([$connectionRequestId]);
$pdo->prepare("DELETE FROM connection_requests WHERE id = ?")->execute([$connectionRequestId]);

$pendingRequestToken = bin2hex(random_bytes(16));
$pdo->prepare("
    INSERT INTO connection_requests
        (request_token, requester_user_id, requester_system_id, requester_name, target_user_id,
         target_system_id, status, created_at, expires_at)
    VALUES (?, ?, ?, 'Free Test User', ?, ?, 'pending', UTC_TIMESTAMP(), UTC_TIMESTAMP() + INTERVAL 60 SECOND)
")->execute([$pendingRequestToken, $freeUserId, $devUid, $otherUserId, $otherDeviceUid]);
$pendingRequestCheck = SubscriptionService::checkConcurrentSessionLimit($freeUserId);
assertTest(
    "Free user's pending connection request reserves its only session slot",
    $pendingRequestCheck['authorized'] === false
        && $pendingRequestCheck['code'] === 'CONCURRENT_SESSION_LIMIT_EXCEEDED'
);
$currentRequestCheck = SubscriptionService::checkConcurrentSessionLimit($freeUserId, $pendingRequestToken);
assertTest(
    "Current pending request is excluded while authorizing its session start",
    $currentRequestCheck['authorized'] === true
);
$pdo->prepare("DELETE FROM connection_requests WHERE request_token = ?")->execute([$pendingRequestToken]);
$pdo->prepare("DELETE FROM devices WHERE id = ?")->execute([$otherDeviceId]);
$pdo->prepare("DELETE FROM users WHERE id = ?")->execute([$otherUserId]);

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
// 8. Reject the legacy client-supplied payment activation path
// ----------------------------------------------------------------------
echo "\n--- TEST GROUP 8: Paid Plan Activation & Payment Verification ---\n";
$unauthorizedManualActivationRejected = false;
try {
    SubscriptionService::activateManualSubscription(
        $freeUserId,
        (int)$monthlyPlan['id'],
        $freeUserId,
        'Test user cannot grant a plan'
    );
} catch (InvalidArgumentException $error) {
    $unauthorizedManualActivationRejected = true;
}
assertTest("Non-admin account cannot manually activate paid plans", $unauthorizedManualActivationRejected);

$adminEmail = 'test_subscription_admin_' . bin2hex(random_bytes(4)) . '@test.local';
$adminUid = (string)random_int(100000000, 999999999);
$pdo->prepare("
    INSERT INTO users (name, username, email, password_hash, unique_user_id, role, status, created_at)
    VALUES ('Subscription Test Admin', 'subscription_admin_test', ?, 'hash', ?, 'admin', 'active', NOW())
")->execute([$adminEmail, $adminUid]);
$testAdminId = (int)$pdo->lastInsertId();
$manualActivation = SubscriptionService::activateManualSubscription(
    $freeUserId,
    (int)$monthlyPlan['id'],
    $testAdminId,
    'Regression test manual activation'
);
$manualSubscription = $pdo->prepare("
    SELECT status, activation_source, activated_by, activation_reason, expires_at
    FROM subscriptions
    WHERE id = ?
");
$manualSubscription->execute([$manualActivation['subscription_id']]);
$manualRecord = $manualSubscription->fetch(PDO::FETCH_ASSOC);
assertTest(
    "Authorized admin manually activates the selected paid plan",
    $manualActivation['status'] === 'success'
        && $manualActivation['activation_source'] === 'manual'
        && $manualRecord
        && $manualRecord['status'] === 'active'
);
assertTest(
    "Manual activation records admin, reason, and a one-month expiry",
    (int)($manualRecord['activated_by'] ?? 0) === $testAdminId
        && ($manualRecord['activation_reason'] ?? '') === 'Regression test manual activation'
        && !empty($manualRecord['expires_at'])
);
assertTest(
    "Manually activated paid plan grants paid entitlements",
    SubscriptionService::getUserEntitlements($freeUserId)['plan_slug'] === 'monthly'
);

$originalMonthlyFeatures = (string)$monthlyPlan['features'];
try {
    $changedFeatures = $monthlyPlan['features_decoded'];
    $changedFeatures['max_quality'] = '480p';
    $pdo->prepare("UPDATE plans SET features = ?, concurrent_sessions = 2 WHERE id = ?")
        ->execute([json_encode($changedFeatures, JSON_THROW_ON_ERROR), $monthlyPlan['id']]);
    $snapshotEntitlements = SubscriptionService::getUserEntitlements($freeUserId);
    assertTest(
        "Editing a plan definition does not rewrite existing subscription entitlements",
        $snapshotEntitlements['max_quality'] === ($monthlyPlan['features_decoded']['max_quality'] ?? 'full')
            && $snapshotEntitlements['max_concurrent_sessions'] === (int)$monthlyPlan['concurrent_sessions'],
        json_encode([
            'actual_quality' => $snapshotEntitlements['max_quality'],
            'expected_quality' => $monthlyPlan['features_decoded']['max_quality'] ?? 'full',
            'actual_sessions' => $snapshotEntitlements['max_concurrent_sessions'],
            'expected_sessions' => (int)$monthlyPlan['concurrent_sessions']
        ])
    );
} finally {
    $pdo->prepare("UPDATE plans SET features = ?, concurrent_sessions = ? WHERE id = ?")
        ->execute([$originalMonthlyFeatures, $monthlyPlan['concurrent_sessions'], $monthlyPlan['id']]);
}

$razorpayOrderId = 'order_test_' . bin2hex(random_bytes(8));
$razorpayPaymentId = 'pay_test_' . bin2hex(random_bytes(8));
$monthlySnapshot = [
    'name' => $monthlyPlan['name'],
    'slug' => $monthlyPlan['slug'],
    'price' => (string)$monthlyPlan['price'],
    'duration_value' => (int)$monthlyPlan['duration_value'],
    'duration_unit' => (string)$monthlyPlan['duration_unit'],
    'duration_days' => (int)$monthlyPlan['duration_days'],
    'concurrent_sessions' => (int)$monthlyPlan['concurrent_sessions'],
    'max_devices' => (int)$monthlyPlan['max_devices'],
    'features' => $monthlyPlan['features_decoded']
];
$pdo->prepare("
    INSERT INTO payments
        (user_id, amount, transaction_id, payment_method, status, gateway_order_id,
         currency, plan_id, plan_snapshot, created_at)
    VALUES (?, ?, ?, 'razorpay', 'pending', ?, 'INR', ?, ?, UTC_TIMESTAMP())
")->execute([
    $freeUserId,
    $monthlyPlan['price'],
    $razorpayOrderId,
    $razorpayOrderId,
    $monthlyPlan['id'],
    json_encode($monthlySnapshot, JSON_THROW_ON_ERROR)
]);
$razorpayPayment = [
    'id' => $razorpayPaymentId,
    'order_id' => $razorpayOrderId,
    'status' => 'captured',
    'amount' => (int)round((float)$monthlyPlan['price'] * 100),
    'currency' => 'INR'
];
$razorpayActivation = SubscriptionService::activateVerifiedRazorpayPayment(
    $razorpayOrderId,
    $razorpayPaymentId,
    $razorpayPayment
);
$razorpayRetry = SubscriptionService::activateVerifiedRazorpayPayment(
    $razorpayOrderId,
    $razorpayPaymentId,
    $razorpayPayment
);
$razorpayPaymentRow = $pdo->prepare("
    SELECT status, gateway_payment_id, signature_verified_at, subscription_id
    FROM payments
    WHERE gateway_order_id = ?
");
$razorpayPaymentRow->execute([$razorpayOrderId]);
$verifiedPaymentRecord = $razorpayPaymentRow->fetch(PDO::FETCH_ASSOC);
assertTest(
    "Captured Razorpay payment activates the plan from its order snapshot",
    $razorpayActivation['status'] === 'success'
        && $razorpayActivation['idempotent'] === false
        && $razorpayActivation['plan_name'] === $monthlyPlan['name']
);
assertTest(
    "Razorpay webhook retry reuses the same subscription exactly once",
    $razorpayRetry['idempotent'] === true
        && $razorpayRetry['subscription_id'] === $razorpayActivation['subscription_id']
        && (int)$verifiedPaymentRecord['subscription_id'] === $razorpayActivation['subscription_id']
);
assertTest(
    "Verified payment is recorded with its gateway reference and verification timestamp",
    $verifiedPaymentRecord['status'] === 'completed'
        && $verifiedPaymentRecord['gateway_payment_id'] === $razorpayPaymentId
        && !empty($verifiedPaymentRecord['signature_verified_at'])
);
$invalidRazorpayAmountRejected = false;
try {
    $invalidPayment = $razorpayPayment;
    $invalidPayment['amount']++;
    SubscriptionService::activateVerifiedRazorpayPayment(
        $razorpayOrderId,
        $razorpayPaymentId,
        $invalidPayment
    );
} catch (InvalidArgumentException $error) {
    $invalidRazorpayAmountRejected = true;
}
assertTest("Razorpay activation rejects an amount that differs from the stored order", $invalidRazorpayAmountRejected);

$legacyActivationRejected = false;
try {
    SubscriptionService::activatePaidSubscription(
        $freeUserId,
        (int)$monthlyPlan['id'],
        'CLIENT_SUPPLIED_' . bin2hex(random_bytes(8)),
        100.00,
        'card'
    );
} catch (LogicException $error) {
    $legacyActivationRejected = true;
}
assertTest("Client-supplied transaction IDs cannot activate paid plans", $legacyActivationRejected);

// Clean up test data
$pdo->prepare("DELETE FROM sessions WHERE host_device_id = ?")->execute([$freeDevId]);
$pdo->prepare("DELETE FROM devices WHERE id = ?")->execute([$freeDevId]);
$pdo->prepare("DELETE FROM payments WHERE user_id IN (?, ?, ?)")->execute([$testUserId, $freeUserId, $newDurationUserId]);
$pdo->prepare("DELETE FROM subscriptions WHERE user_id IN (?, ?, ?)")->execute([$testUserId, $freeUserId, $newDurationUserId]);
$pdo->prepare("DELETE FROM users WHERE id IN (?, ?, ?, ?)")->execute([$testUserId, $freeUserId, $newDurationUserId, $testAdminId]);

echo "\n======================================================================\n";
echo " TEST SUITE SUMMARY: {$passCount} PASSED, {$failCount} FAILED\n";
echo "======================================================================\n";

if ($failCount > 0) {
    exit(1);
}
