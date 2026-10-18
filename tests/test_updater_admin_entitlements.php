<?php
/**
 * test_updater_admin_entitlements.php
 * Automated regression test covering:
 * 1. Updater manifest generation, signing, and retrieval
 * 2. Admin plan activation SQL resilience (no role column error)
 * 3. Plan allocation persistence and expiry calculation
 * 4. Free Plan vs Welcome/Paid entitlements (Reverse control, 720p cap, concurrent sessions)
 */

require_once __DIR__ . '/../backend/config/database.php';
require_once __DIR__ . '/../backend/services/SubscriptionService.php';

$passed = 0;
$failed = 0;

function assertCheck(string $desc, bool $condition, string $detail = '') {
    global $passed, $failed;
    if ($condition) {
        $passed++;
        echo "  [PASS] $desc\n";
    } else {
        $failed++;
        echo "  [FAIL] $desc " . ($detail ? "($detail)" : "") . "\n";
    }
}

echo "======================================================================\n";
echo " DESKSTREAM UPDATER, ADMIN ACTIVATION & ENTITLEMENTS VERIFICATION\n";
echo "======================================================================\n\n";

// ── PART 1: UPDATER ENDPOINT & MANIFEST ───────────────────────────────────────
echo "--- PART 1: Updater Endpoint & Manifest Verification ---\n";

$manifestPath = __DIR__ . '/../updates/stable.json';
assertCheck("Local manifest file exists", file_exists($manifestPath));
if (file_exists($manifestPath)) {
    $manifestData = json_decode(file_get_contents($manifestPath), true);
    assertCheck("Manifest contains app_id", !empty($manifestData['app_id']));
    assertCheck("Manifest version matches SemVer", (bool)preg_match('/^\d+\.\d+\.\d+$/', $manifestData['version'] ?? ''));
    assertCheck("Manifest contains SHA-256 package hash", strlen($manifestData['package']['sha256'] ?? '') === 64);
    assertCheck("Manifest contains 64-byte Ed25519 signature", strlen($manifestData['signature']['value'] ?? '') === 128);
}

$checkOutput = shell_exec('C:\\xampp\\php\\php.exe "' . __DIR__ . '/../backend/api/updates/check.php"');
$apiManifest = json_decode((string)$checkOutput, true);
assertCheck("check.php outputs valid JSON", is_array($apiManifest));
assertCheck("check.php provides correct app_id", ($apiManifest['app_id'] ?? '') === 'com.friendssoftwaresolutions.deskstream');
assertCheck("check.php provides signature info", !empty($apiManifest['signature']['value']));

// ── PART 2: ADMIN ACTIVATION & SQL RESILIENCE ─────────────────────────────────
echo "\n--- PART 2: Admin Plan Activation SQL Resilience ---\n";

$userEmail = 'test_alloc_' . bin2hex(random_bytes(4)) . '@example.com';
$userUid = 'usr_' . bin2hex(random_bytes(6));
$pdo->prepare("INSERT INTO users (email, name, password_hash, unique_user_id, created_at) VALUES (?, 'Test Target', 'dummy', ?, NOW())")
    ->execute([$userEmail, $userUid]);
$testUserId = (int)$pdo->lastInsertId();

// Verify non-admin rejection
try {
    SubscriptionService::activateManualSubscription($testUserId, 6, 999999, 'Test unauthorized');
    assertCheck("Non-admin activation should be rejected", false);
} catch (InvalidArgumentException $e) {
    assertCheck("Non-admin activation rejected with InvalidArgumentException", true);
} catch (Throwable $e) {
    assertCheck("Non-admin activation rejected without SQL crash", false, $e->getMessage());
}

// Find a valid admin or simulate admin session
$adminId = 1;
try {
    $actMonthly = SubscriptionService::activateManualSubscription($testUserId, 6, $adminId, 'Test admin activation');
    assertCheck("Admin successfully activated Monthly Plan without 'Unknown column role' error", $actMonthly['status'] === 'success');
    assertCheck("Monthly plan activation has expires_at", !empty($actMonthly['expires_at']));
    
    // Check database persistence
    $sub = SubscriptionService::getUserSubscription($testUserId);
    assertCheck("Allocated plan persists in database", $sub['plan_slug'] === 'monthly');
    assertCheck("Allocated plan has status active", $sub['status'] === 'active');
    assertCheck("Allocated plan start date is recorded", !empty($sub['starts_at']));
    assertCheck("Allocated plan expiry date matches", $sub['expires_at'] === $actMonthly['expires_at']);
} catch (Throwable $e) {
    assertCheck("Admin activation succeeded without SQL error", false, $e->getMessage());
}

// ── PART 3: REAL ENTITLEMENT ENFORCEMENT ──────────────────────────────────────
echo "\n--- PART 3: Real Entitlement Enforcement ---\n";

// Monthly user entitlements
$monthlyEnt = SubscriptionService::getUserEntitlements($testUserId);
assertCheck("Monthly plan: reverse control is enabled", $monthlyEnt['can_reverse_control'] === true);
assertCheck("Monthly plan: max quality is full", $monthlyEnt['max_quality'] === 'full');
$monthlyRevAuth = SubscriptionService::authorizeAction($testUserId, 'reverse_control');
assertCheck("Monthly plan: reverse_control action authorized", $monthlyRevAuth['authorized'] === true);

// Expire monthly subscription and record that user already used Welcome Plan in the past
$pdo->prepare("INSERT INTO subscriptions (user_id, plan_id, status, starts_at, expires_at, created_at) VALUES (?, 4, 'expired', DATE_SUB(NOW(), INTERVAL 7 MONTH), DATE_SUB(NOW(), INTERVAL 1 MONTH), NOW())")->execute([$testUserId]);
$pdo->prepare("UPDATE subscriptions SET status = 'expired' WHERE user_id = ?")->execute([$testUserId]);
$freeSub = SubscriptionService::getUserSubscription($testUserId);
assertCheck("Expired subscription transitions to Free Plan", $freeSub['plan_slug'] === 'free');
assertCheck("Free Plan has no expiry (null)", empty($freeSub['expires_at']));

$freeEnt = SubscriptionService::getUserEntitlements($testUserId);
assertCheck("Free Plan: reverse control is disabled", $freeEnt['can_reverse_control'] === false);
assertCheck("Free Plan: max quality is capped at 720p", $freeEnt['max_quality'] === '720p');
assertCheck("Free Plan: concurrent sessions is strictly 1", $freeEnt['max_concurrent_sessions'] === 1);

// Test reverse_control authorization boundary for Free user
$freeRevAuth = SubscriptionService::authorizeAction($testUserId, 'reverse_control');
assertCheck("Free Plan: reverse_control action rejected by server boundary", $freeRevAuth['authorized'] === false && $freeRevAuth['code'] === 'REVERSE_CONTROL_DISABLED');

// Test 720p quality cap for Free user
$qualHigh = SubscriptionService::authorizeAction($testUserId, 'quality', ['height' => 1080]);
assertCheck("Free Plan: 1080p stream rejected by server boundary", $qualHigh['authorized'] === false && $qualHigh['code'] === 'QUALITY_CAPPED');
$qual720 = SubscriptionService::authorizeAction($testUserId, 'quality', ['height' => 720]);
assertCheck("Free Plan: 720p stream accepted", $qual720['authorized'] === true);

// Test Welcome Plan immutable assigned expiry
echo "\n--- PART 4: Welcome Plan Immutability ---\n";
$newWelcomeEmail = 'welcome_immut_' . bin2hex(random_bytes(4)) . '@example.com';
$welcomeUid = 'usr_' . bin2hex(random_bytes(6));
$pdo->prepare("INSERT INTO users (email, name, password_hash, unique_user_id, created_at) VALUES (?, 'Welcome User', 'dummy', ?, NOW())")
    ->execute([$newWelcomeEmail, $welcomeUid]);
$welcomeUserId = (int)$pdo->lastInsertId();

$assignedWelcome = SubscriptionService::assignWelcomePlanIfEligible($welcomeUserId);
assertCheck("New user assigned Welcome Plan", !empty($assignedWelcome));
$initialExpiry = $assignedWelcome['expires_at'] ?? '';
assertCheck("Welcome Plan has saved expiry timestamp", !empty($initialExpiry));

// Re-fetching retrieves the exact saved expiry
$fetchedWelcome = SubscriptionService::getUserSubscription($welcomeUserId);
assertCheck("Re-fetched Welcome Plan matches saved expiry date", $fetchedWelcome['expires_at'] === $initialExpiry);

// Clean up test rows
$pdo->prepare("DELETE FROM subscriptions WHERE user_id IN (?, ?)")->execute([$testUserId, $welcomeUserId]);
$pdo->prepare("DELETE FROM users WHERE id IN (?, ?)")->execute([$testUserId, $welcomeUserId]);

echo "\n======================================================================\n";
echo " RESULTS: $passed PASSED, $failed FAILED\n";
echo "======================================================================\n";

exit($failed > 0 ? 1 : 0);
