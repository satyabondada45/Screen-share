<?php
require_once __DIR__ . '/includes/db.php';
require_once __DIR__ . '/includes/auth.php';
require_once __DIR__ . '/includes/layout.php';

admin_require_auth();
try {
    $pdo = admin_db();
} catch (Throwable $e) {
    error_log('Admin database initialization failed: ' . $e->getMessage());
    echo admin_render_page('System Error', 'error', '<div class="flash error">Unable to connect to the database. Please check system configuration.</div>');
    exit;
}

$deviceTable = admin_table_exists($pdo, 'devices') ? 'devices' : null;
$subscriptionTable = admin_table_exists($pdo, 'subscriptions') ? 'subscriptions' : null;
$paymentTable = admin_table_exists($pdo, 'payments') ? 'payments' : null;
$planTable = admin_table_exists($pdo, 'plans') ? 'plans' : null;

$stats = [
    'total_devices' => 0,
    'active_devices' => 0,
    'active_subscriptions' => 0,
    'expired_subscriptions' => 0,
    'expiring_soon' => 0,
    'total_revenue' => 0,
];

try {
    if ($deviceTable) {
        $deviceCounter = $pdo->query('SELECT COUNT(*) AS total FROM `' . $deviceTable . '`');
        $stats['total_devices'] = (int) ($deviceCounter->fetch()['total'] ?? 0);

        if (admin_column_exists($pdo, $deviceTable, 'is_online')) {
            $activeStmt = $pdo->query('SELECT COUNT(*) AS total FROM `' . $deviceTable . '` WHERE is_online = 1');
            $stats['active_devices'] = (int) ($activeStmt->fetch()['total'] ?? 0);
        } elseif (admin_column_exists($pdo, $deviceTable, 'status')) {
            $activeStmt = $pdo->query('SELECT COUNT(*) AS total FROM `' . $deviceTable . '` WHERE LOWER(status) IN ("active", "online")');
            $stats['active_devices'] = (int) ($activeStmt->fetch()['total'] ?? 0);
        }
    }

    if ($subscriptionTable) {
        if (admin_column_exists($pdo, $subscriptionTable, 'status')) {
            $activeSub = $pdo->query('SELECT COUNT(*) AS total FROM `' . $subscriptionTable . '` WHERE LOWER(status) = "active"');
            $stats['active_subscriptions'] = (int) ($activeSub->fetch()['total'] ?? 0);
        }

        if (admin_column_exists($pdo, $subscriptionTable, 'expiry_date')) {
            $expiredSub = $pdo->query('SELECT COUNT(*) AS total FROM `' . $subscriptionTable . '` WHERE expiry_date IS NOT NULL AND expiry_date < NOW()');
            $stats['expired_subscriptions'] = (int) ($expiredSub->fetch()['total'] ?? 0);
        }

        if (admin_column_exists($pdo, $subscriptionTable, 'expiry_date')) {
            $expiringSoon = $pdo->query('SELECT COUNT(*) AS total FROM `' . $subscriptionTable . '` WHERE expiry_date IS NOT NULL AND expiry_date BETWEEN NOW() AND DATE_ADD(NOW(), INTERVAL 7 DAY)');
            $stats['expiring_soon'] = (int) ($expiringSoon->fetch()['total'] ?? 0);
        }
    }

    if ($paymentTable) {
        $revenueSql = 'SELECT COALESCE(SUM(amount), 0) AS total FROM `' . $paymentTable . '`';
        if (admin_column_exists($pdo, $paymentTable, 'status')) {
            $revenueSql .= ' WHERE LOWER(status) IN ("paid", "successful", "completed")';
        }
        $revenueRow = $pdo->query($revenueSql);
        $stats['total_revenue'] = (float) ($revenueRow->fetch()['total'] ?? 0);
    }
} catch (Throwable $e) {
    error_log('Dashboard metrics error: ' . $e->getMessage());
}

$recentDevices = [];
if ($deviceTable) {
    try {
        $userTable = admin_table_exists($pdo, 'users') ? 'users' : null;
        $canJoinUser = $deviceTable && $userTable
            && admin_column_exists($pdo, $deviceTable, 'user_id')
            && admin_column_exists($pdo, $userTable, 'id');
            
        $userSelect = '';
        $deviceFrom = ' FROM `' . $deviceTable . '` d';
        if ($canJoinUser) {
            $userCols = [];
            if (admin_column_exists($pdo, $userTable, 'email')) $userCols[] = 'u.`email` AS `owner_email`';
            if (admin_column_exists($pdo, $userTable, 'username')) $userCols[] = 'u.`username` AS `owner_username`';
            if ($userCols) {
                $userSelect = ', ' . implode(', ', $userCols);
                $deviceFrom .= ' LEFT JOIN `' . $userTable . '` u ON u.`id` = d.`user_id`';
            }
        }
        
        $deviceSql = 'SELECT d.*' . $userSelect . $deviceFrom;
        if (admin_column_exists($pdo, $deviceTable, 'last_seen')) {
            $deviceSql .= ' ORDER BY d.last_seen DESC LIMIT 5';
        } elseif (admin_column_exists($pdo, $deviceTable, 'last_seen_at')) {
            $deviceSql .= ' ORDER BY d.last_seen_at DESC LIMIT 5';
        } elseif (admin_column_exists($pdo, $deviceTable, 'created_at')) {
            $deviceSql .= ' ORDER BY d.created_at DESC LIMIT 5';
        } else {
            $deviceSql .= ' LIMIT 5';
        }
        $recentDevicesStmt = $pdo->query($deviceSql);
        $recentDevices = $recentDevicesStmt ? $recentDevicesStmt->fetchAll(PDO::FETCH_ASSOC) : [];
    } catch (Throwable $e) {
        error_log('Recent devices query error: ' . $e->getMessage());
    }
}

$recentSubscriptions = [];
if ($subscriptionTable) {
    try {
        $subOrder = admin_column_exists($pdo, $subscriptionTable, 'created_at')
            ? '`created_at`'
            : (admin_column_exists($pdo, $subscriptionTable, 'id') ? '`id`' : null);
        $subSql = 'SELECT * FROM `subscriptions`' . ($subOrder ? ' ORDER BY ' . $subOrder . ' DESC' : '') . ' LIMIT 5';
        $subStmt = $pdo->query($subSql);
        $recentSubscriptions = $subStmt ? $subStmt->fetchAll(PDO::FETCH_ASSOC) : [];
    } catch (Throwable $e) {
        error_log('Recent subscriptions query error: ' . $e->getMessage());
    }
}

$expiringSoonRows = [];
if ($subscriptionTable) {
    try {
        if (admin_column_exists($pdo, $subscriptionTable, 'expiry_date')) {
            $expiringSql = 'SELECT * FROM `subscriptions` WHERE `expiry_date` IS NOT NULL AND `expiry_date` BETWEEN NOW() AND DATE_ADD(NOW(), INTERVAL 7 DAY) ORDER BY `expiry_date` ASC LIMIT 5';
            $expiringStmt = $pdo->query($expiringSql);
            $expiringSoonRows = $expiringStmt ? $expiringStmt->fetchAll(PDO::FETCH_ASSOC) : [];
        }
    } catch (Throwable $e) {
        error_log('Expiring soon query error: ' . $e->getMessage());
    }
}

$cardMarkup = function ($label, $value, $trend) {
    return '<div class="card"><span class="label">' . htmlspecialchars($label) . '</span><p class="value">' . htmlspecialchars((string) $value) . '</p><div class="trend">' . htmlspecialchars($trend) . '</div></div>';
};

$recentDevicesMarkup = '';
if (!empty($recentDevices)) {
    foreach ($recentDevices as $device) {
        $deviceId = $device['device_id'] ?? ($device['device_uid'] ?? ($device['system_id'] ?? $device['id']));
        $deviceName = $device['device_name'] ?? ($device['name'] ?? 'Unnamed device');
        $email = $device['owner_email'] ?? ($device['email'] ?? ($device['user_email'] ?? 'N/A'));
        $status = strtoupper((string) ($device['status'] ?? (!empty($device['is_online']) ? 'Online' : 'Offline')));
        $expiry = $device['expiry_date'] ?? ($device['expiry'] ?? 'N/A');
        $lastSeen = $device['last_seen'] ?? ($device['last_seen_at'] ?? 'N/A');
        $recentDevicesMarkup .= '<tr>'
            . '<td>' . htmlspecialchars((string) $deviceId) . '</td>'
            . '<td>' . htmlspecialchars((string) $deviceName) . '</td>'
            . '<td>' . htmlspecialchars((string) $email) . '</td>'
            . '<td><span class="status-badge status-' . strtolower(str_replace(' ', '-', $status)) . '">' . htmlspecialchars($status) . '</span></td>'
            . '<td>' . htmlspecialchars((string) $expiry) . '</td>'
            . '<td>' . htmlspecialchars((string) $lastSeen) . '</td>'
            . '<td><a class="outline-button" href="' . htmlspecialchars(admin_url('devices.php', ['view' => (string) ($device['id'] ?? $deviceId)]), ENT_QUOTES, 'UTF-8') . '">View</a></td>'
            . '</tr>';
    }
} else {
    $recentDevicesMarkup = '<tr><td colspan="7" class="empty-state">No remote devices registered yet.</td></tr>';
}

$recentSubscriptionsMarkup = '';
if (!empty($recentSubscriptions)) {
    foreach ($recentSubscriptions as $sub) {
        $deviceRef = $sub['device_id'] ?? ($sub['device_name'] ?? 'Device');
        $plan = $sub['plan_name'] ?? ($sub['plan_id'] ?? 'Plan');
        $start = $sub['start_date'] ?? 'N/A';
        $expiry = $sub['expiry_date'] ?? 'N/A';
        $status = strtoupper((string) ($sub['status'] ?? 'Active'));
        $recentSubscriptionsMarkup .= '<tr>'
            . '<td>' . htmlspecialchars((string) $deviceRef) . '</td>'
            . '<td>' . htmlspecialchars((string) $plan) . '</td>'
            . '<td>' . htmlspecialchars((string) $start) . '</td>'
            . '<td>' . htmlspecialchars((string) $expiry) . '</td>'
            . '<td><span class="status-badge status-' . strtolower(str_replace(' ', '-', $status)) . '">' . htmlspecialchars($status) . '</span></td>'
            . '</tr>';
    }
} else {
    $recentSubscriptionsMarkup = '<tr><td colspan="5" class="empty-state">No subscriptions found.</td></tr>';
}

$expiringMarkup = '';
if (!empty($expiringSoonRows)) {
    foreach ($expiringSoonRows as $row) {
        $expiry = $row['expiry_date'] ?? 'N/A';
        $status = strtoupper((string) ($row['status'] ?? 'Expiring Soon'));
        $expiringMarkup .= '<li>' . htmlspecialchars((string) ($row['device_name'] ?? ($row['device_id'] ?? 'Device'))) . ' — ' . htmlspecialchars((string) $expiry) . ' — ' . htmlspecialchars($status) . '</li>';
    }
} else {
    $expiringMarkup = '<li class="muted">No subscriptions expiring soon.</li>';
}

$devicesUrl = htmlspecialchars(admin_url('devices.php'), ENT_QUOTES, 'UTF-8');
$subscriptionsUrl = htmlspecialchars(admin_url('subscriptions.php'), ENT_QUOTES, 'UTF-8');
$body = <<<HTML
<div class="card-grid">
    {$cardMarkup('Total Remote Devices', $stats['total_devices'], 'Across all registered devices')}
    {$cardMarkup('Active Devices', $stats['active_devices'], 'Currently online/active')}
    {$cardMarkup('Active Subscriptions', $stats['active_subscriptions'], 'Current active plans')}
    {$cardMarkup('Expired Subscriptions', $stats['expired_subscriptions'], 'Past due subscriptions')}
    {$cardMarkup('Subscriptions Expiring Soon', $stats['expiring_soon'], 'Next 7 days')}
    {$cardMarkup('Total Revenue', '₹' . number_format((float) $stats['total_revenue'], 2), 'Paid/Successful payments')}
</div>

<div class="section">
    <div class="section-header">
        <h3>Recent Devices</h3>
        <div class="mini-actions"><a class="button" href="{$devicesUrl}">View all devices</a></div>
    </div>
    <div style="overflow:auto;">
        <table>
            <thead>
                <tr>
                    <th>Device ID</th>
                    <th>Device Name</th>
                    <th>Email</th>
                    <th>Status</th>
                    <th>Expiry</th>
                    <th>Last Seen</th>
                    <th>Actions</th>
                </tr>
            </thead>
            <tbody>
                {$recentDevicesMarkup}
            </tbody>
        </table>
    </div>
</div>

<div class="section">
    <div class="section-header">
        <h3>Recent Subscriptions</h3>
        <div class="mini-actions"><a class="button" href="{$subscriptionsUrl}">View all subscriptions</a></div>
    </div>
    <div style="overflow:auto;">
        <table>
            <thead>
                <tr>
                    <th>Device</th>
                    <th>Plan</th>
                    <th>Start Date</th>
                    <th>Expiry</th>
                    <th>Status</th>
                </tr>
            </thead>
            <tbody>
                {$recentSubscriptionsMarkup}
            </tbody>
        </table>
    </div>
</div>

<div class="section">
    <div class="section-header">
        <h3>Expiring Soon</h3>
    </div>
    <div class="detail-panels">
        <div class="detail-panel">
            <ul style="margin:0; padding-left:18px; line-height:2;">{$expiringMarkup}</ul>
        </div>
    </div>
</div>
HTML;

echo admin_render_page('Dashboard', 'dashboard', $body);
