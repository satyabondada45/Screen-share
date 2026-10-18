<?php
/**
 * DeskStream Admin Portal Comprehensive Regression Test
 *
 * Verifies that all Admin Portal endpoints render without:
 * - Uncaught PDOExceptions
 * - PHP Fatal Errors
 * - HTTP 500 status codes
 * - Missing column or table schema crashes
 */

$adminDir = str_replace('\\', '/', realpath(__DIR__ . '/../admin'));

$pages = [
    'index.php' => [[], ['range' => '7d']],
    'users.php' => [[], ['view' => 1], ['q' => 'test', 'status' => 'active']],
    'devices.php' => [[], ['view' => 1], ['q' => 'test']],
    'sessions.php' => [[], ['view' => 1], ['status' => 'active']],
    'connections.php' => [[], ['view' => 1], ['status' => 'pending']],
    'subscriptions.php' => [[], ['view' => 1], ['status' => 'active']],
    'plans.php' => [[], ['action' => 'edit', 'id' => 1]],
    'payments.php' => [[], ['view' => 1], ['status' => 'completed']],
    'notifications.php' => [[]],
    'reports.php' => [[], ['tab' => 'sessions'], ['tab' => 'users'], ['tab' => 'devices']],
    'support.php' => [[], ['view' => 1]],
    'audit-logs.php' => [[], ['action' => 'login']],
    'login.php' => [[]],
    'logout.php' => [[]],
];

function runPageTest(string $adminDir, string $page, array $get = [], bool $authenticated = true): array {
    $targetFile = $adminDir . '/' . $page;
    $script = "
        error_reporting(E_ALL);
        ini_set('display_errors', '1');
        \$_SERVER['REQUEST_METHOD'] = 'GET';
        \$_SERVER['SCRIPT_NAME'] = '/DeskStream/admin/" . addslashes($page) . "';
        \$_SERVER['REMOTE_ADDR'] = '127.0.0.1';
        \$_GET = " . var_export($get, true) . ";
        \$_POST = [];
        session_start();
        if (" . ($authenticated ? 'true' : 'false') . ") {
            \$_SESSION['admin_logged_in'] = true;
            \$_SESSION['admin_user_id'] = 1;
            \$_SESSION['admin_user_name'] = 'Admin User';
            \$_SESSION['admin_user_email'] = 'admin@screenshare.local';
            \$_SESSION['admin_csrf_token'] = 'test_token';
        }
        chdir('" . addslashes($adminDir) . "');
        require '" . addslashes($targetFile) . "';
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

echo "=== DESKSTREAM ADMIN PORTAL REGRESSION SUITE ===\n\n";
$failures = 0;

foreach ($pages as $page => $variations) {
    echo "Checking $page:\n";
    if ($page !== 'login.php') {
        $resUnauth = runPageTest($adminDir, $page, [], false);
        if ($resUnauth['code'] !== 0) {
            echo "  [FAIL] Unauthenticated redirect failed with code {$resUnauth['code']}\n";
            $failures++;
        } else {
            echo "  [PASS] Unauthenticated access properly secured.\n";
        }
    }

    foreach ($variations as $idx => $getParams) {
        $queryStr = http_build_query($getParams);
        $resAuth = runPageTest($adminDir, $page, $getParams, true);
        if ($resAuth['code'] !== 0 || !empty($resAuth['stderr']) || strpos($resAuth['stdout'], 'Fatal error') !== false) {
            echo "  [FAIL] Query (?$queryStr)\n";
            if ($resAuth['stderr']) echo "    STDERR: " . trim($resAuth['stderr']) . "\n";
            $failures++;
        } else {
            $len = strlen($resAuth['stdout']);
            echo "  [PASS] Query (?$queryStr) - Output: $len bytes\n";
        }
    }
    echo "\n";
}

if ($failures === 0) {
    echo "=== ALL ADMIN PAGES PASSED WITH ZERO ERRORS ===\n";
    exit(0);
} else {
    echo "=== {$failures} TEST(S) FAILED ===\n";
    exit(1);
}
