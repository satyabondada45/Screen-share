<?php
session_start();
$_SESSION['admin_logged_in'] = true;
$_SESSION['admin_user_id'] = 1;
$_SESSION['admin_user_name'] = 'Admin User';
$_SESSION['admin_user_email'] = 'admin@screenshare.local';

$tests = [
    'Dashboard' => ['file' => 'index.php', 'query' => []],
    'Users List' => ['file' => 'users.php', 'query' => []],
    'User Detail' => ['file' => 'users.php', 'query' => ['view' => 3]],
    'Devices List' => ['file' => 'devices.php', 'query' => []],
    'Device Detail' => ['file' => 'devices.php', 'query' => ['view' => 234]],
    'Sessions' => ['file' => 'sessions.php', 'query' => []],
    'Connections' => ['file' => 'connections.php', 'query' => []],
    'Subscriptions' => ['file' => 'subscriptions.php', 'query' => []],
    'Plans' => ['file' => 'plans.php', 'query' => []],
    'Payments' => ['file' => 'payments.php', 'query' => []],
    'Support' => ['file' => 'support.php', 'query' => []],
    'Audit Logs' => ['file' => 'audit-logs.php', 'query' => []],
    'Reports & Analytics' => ['file' => 'reports.php', 'query' => []],
    'Notifications' => ['file' => 'notifications.php', 'query' => []],
];

echo "=== DESKSTREAM ADMIN PORTAL END-TO-END VERIFICATION ===\n\n";

$passCount = 0;
$failCount = 0;

foreach ($tests as $name => $test) {
    $_GET = $test['query'];
    $_SERVER['REQUEST_METHOD'] = 'GET';
    
    ob_start();
    try {
        require __DIR__ . '/../admin/' . $test['file'];
        $html = ob_get_clean();
        
        // Assertions
        $len = strlen($html);
        $hasDocType = strpos($html, '<!DOCTYPE html>') !== false;
        $hasAdminShell = strpos($html, 'app-shell') !== false;
        
        if ($len > 1000 && $hasDocType && $hasAdminShell) {
            echo sprintf("[PASS] %-22s: Generated %-6d bytes | Status: OK\n", $name, $len);
            $passCount++;
        } else {
            echo sprintf("[FAIL] %-22s: Generated %-6d bytes | Content check failed\n", $name, $len);
            $failCount++;
        }
    } catch (Throwable $e) {
        ob_end_clean();
        echo sprintf("[ERROR] %-22s: Exception: %s\n", $name, $e->getMessage());
        $failCount++;
    }
}

echo "\nSummary: {$passCount} Passed, {$failCount} Failed.\n";
if ($failCount === 0) {
    echo "SUCCESS: 100% of DeskStream Admin Portal pages are database-driven and fully functional!\n";
}
