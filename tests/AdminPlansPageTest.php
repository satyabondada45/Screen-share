<?php
/**
 * Test admin/plans.php behavior directly
 */

require_once __DIR__ . '/../backend/config/database.php';
require_once __DIR__ . '/../admin/includes/db.php';
require_once __DIR__ . '/../admin/includes/auth.php';

echo "Testing Admin Plans Management Page...\n";

function runAdminPage(string $file, array $get = [], array $post = [], array $session = []): array {
    $script = "
        \$_SERVER['REQUEST_METHOD'] = " . var_export($post ? 'POST' : 'GET', true) . ";
        \$_SERVER['SCRIPT_NAME'] = '/admin/plans.php';
        \$_GET = " . var_export($get, true) . ";
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
        'stderr' => $err
    ];
}

// 1. Unauthenticated request should terminate immediately (exit on auth failure)
$resUnauth = runAdminPage(__DIR__ . '/../admin/plans.php');
if ($resUnauth['stdout'] === '' && $resUnauth['code'] === 0) {
    echo "[PASS] Unauthenticated request halted without rendering protected page.\n";
} else {
    echo "[FAIL] Unauthenticated request rendered content: " . substr($resUnauth['stdout'], 0, 100) . "\n";
    exit(1);
}

// 2. Authenticated admin request should render the plans table
$adminSession = [
    'admin_logged_in' => true,
    'admin_user_id' => 1,
    'admin_username' => 'admin',
    'admin_csrf_token' => 'test_token_12345'
];
$resAuth = runAdminPage(__DIR__ . '/../admin/plans.php', [], [], $adminSession);
if (strpos($resAuth['stdout'], 'Welcome Plan') === false || strpos($resAuth['stdout'], 'Free Plan') === false) {
    echo "[FAIL] Plans table missing Welcome or Free plan: " . substr($resAuth['stdout'], 0, 300) . "\n";
    exit(1);
}
echo "[PASS] Authenticated admin request renders plans list.\n";

// 3. Submitting invalid CSRF token
$resInvalidCsrf = runAdminPage(__DIR__ . '/../admin/plans.php', [], [
    'plan_submit' => '1',
    'csrf_token' => 'invalid_token',
    'name' => 'Should Not Create',
    'price' => '50.00'
], $adminSession);

$pdo = admin_db();
$checkNotCreated = $pdo->query("SELECT id FROM plans WHERE name = 'Should Not Create'")->fetchColumn();
if ($checkNotCreated) {
    echo "[FAIL] Plan was created despite invalid CSRF token!\n";
    exit(1);
}
echo "[PASS] Invalid CSRF token rejected plan creation.\n";

// 4. Submitting valid plan creation with valid CSRF token
$resCreate = runAdminPage(__DIR__ . '/../admin/plans.php', [], [
    'plan_submit' => '1',
    'csrf_token' => 'test_token_12345',
    'name' => 'Admin Test Tier',
    'slug' => 'admin-test-tier',
    'price' => '150.00',
    'duration_unit' => 'days',
    'duration_value' => '45',
    'concurrent_sessions' => '2',
    'max_devices' => '3',
    'status' => 'active',
    'can_record' => '1',
    'max_quality' => '1080p',
    'description' => 'Created via admin test'
], $adminSession);

$createdId = $pdo->query("SELECT id FROM plans WHERE slug = 'admin-test-tier'")->fetchColumn();
if (!$createdId) {
    echo "[FAIL] Valid plan was not created in database!\n";
    exit(1);
}
echo "[PASS] Valid plan created and persisted to database (ID: {$createdId}).\n";

// 5. Edit plan
$resEdit = runAdminPage(__DIR__ . '/../admin/plans.php', [], [
    'plan_submit' => '1',
    'csrf_token' => 'test_token_12345',
    'edit_id' => $createdId,
    'name' => 'Admin Test Tier Updated',
    'slug' => 'admin-test-tier',
    'price' => '175.00',
    'duration_unit' => 'days',
    'duration_value' => '60',
    'concurrent_sessions' => '4',
    'max_devices' => '5',
    'status' => 'active',
    'description' => 'Updated via admin test'
], $adminSession);

$updatedRow = $pdo->query("SELECT name, price, concurrent_sessions FROM plans WHERE id = {$createdId}")->fetch(PDO::FETCH_ASSOC);
if (($updatedRow['name'] ?? '') !== 'Admin Test Tier Updated' || (float)($updatedRow['price'] ?? 0) !== 175.0) {
    echo "[FAIL] Plan edit was not saved to database!\n";
    exit(1);
}
echo "[PASS] Plan edit successfully updated database record.\n";

// 6. Toggle status
$resToggle = runAdminPage(__DIR__ . '/../admin/plans.php', [
    'action' => 'toggle',
    'id' => $createdId,
    'token' => 'test_token_12345'
], [], $adminSession);

$toggledStatus = $pdo->query("SELECT status FROM plans WHERE id = {$createdId}")->fetchColumn();
if ($toggledStatus !== 'inactive') {
    echo "[FAIL] Status toggle failed to deactivate plan!\n";
    exit(1);
}
echo "[PASS] Status toggle deactivated plan to 'inactive'.\n";

// Clean up
$pdo->exec("DELETE FROM plans WHERE id = {$createdId}");
echo "[PASS] Test plan cleaned up successfully.\n";

echo "All Admin Plans Page checks passed!\n";
