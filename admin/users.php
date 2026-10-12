<?php
require_once __DIR__ . '/includes/db.php';
require_once __DIR__ . '/includes/auth.php';
require_once __DIR__ . '/includes/layout.php';

admin_require_auth();
$pdo = admin_db();
$userTable = admin_table_exists($pdo, 'users') ? 'users' : null;

$rowHtml = '<tr><td colspan="7" class="empty-state">No users found.</td></tr>';
if ($userTable) {
    try {
        $orderColumn = admin_column_exists($pdo, $userTable, 'created_at')
            ? '`created_at`'
            : (admin_column_exists($pdo, $userTable, 'id') ? '`id`' : null);
        $sql = 'SELECT * FROM `users`' . ($orderColumn ? ' ORDER BY ' . $orderColumn . ' DESC' : '') . ' LIMIT 50';
        $stmt = $pdo->query($sql);
        $rows = $stmt->fetchAll(PDO::FETCH_ASSOC);
        if ($rows) {
            $rowHtml = '';
            foreach ($rows as $row) {
                $name = $row['name'] ?? ($row['username'] ?? 'Unknown');
                $email = $row['email'] ?? 'N/A';
                $deviceCount = 0;
                $activeSub = 'N/A';
                $status = strtoupper((string) ($row['status'] ?? 'Active'));
                $created = $row['created_at'] ?? 'N/A';
                $rowHtml .= '<tr><td>' . htmlspecialchars((string) $name) . '</td><td>' . htmlspecialchars((string) $email) . '</td><td>' . (int) $deviceCount . '</td><td>' . htmlspecialchars((string) $activeSub) . '</td><td><span class="status-badge status-' . strtolower(str_replace(' ', '-', $status)) . '">' . htmlspecialchars($status) . '</span></td><td>' . htmlspecialchars((string) $created) . '</td><td><a class="outline-button" href="' . htmlspecialchars(admin_url('users.php', ['view' => (string) ($row['id'] ?? '')]), ENT_QUOTES, 'UTF-8') . '">View</a></td></tr>';
            }
        }
    } catch (Throwable $e) {
        error_log('Users page error: ' . $e->getMessage());
    }
}

$body = '';

if (isset($_GET['view']) && $userTable) {
    try {
        $viewId = (string) $_GET['view'];
        $stmt = $pdo->prepare('SELECT * FROM `users` WHERE `id` = ?');
        $stmt->execute([$viewId]);
        $user = $stmt->fetch(PDO::FETCH_ASSOC);

        if (!$user) {
            $body .= '<div class="section"><div class="section-header"><h3>User Not Found</h3></div><p>The requested user does not exist or has been removed.</p></div>';
        } else {
            $name = $user['name'] ?? ($user['username'] ?? 'Unknown');
            $email = $user['email'] ?? 'N/A';
            $status = strtoupper((string) ($user['status'] ?? 'Active'));
            $created = $user['created_at'] ?? 'N/A';

            // Fetch devices
            $deviceHtml = '<p>No devices found.</p>';
            $userDevices = [];
            if (admin_table_exists($pdo, 'devices') && admin_column_exists($pdo, 'devices', 'user_id')) {
                $dstmt = $pdo->prepare('SELECT * FROM `devices` WHERE `user_id` = ?');
                $dstmt->execute([$viewId]);
                $userDevices = $dstmt->fetchAll(PDO::FETCH_ASSOC);
                
                if ($userDevices) {
                    $deviceHtml = '<table><thead><tr><th>Device ID</th><th>Name</th><th>Status</th><th>Subscription</th></tr></thead><tbody>';
                    foreach ($userDevices as $dev) {
                        $devId = $dev['device_id'] ?? ($dev['device_uid'] ?? ($dev['system_id'] ?? ($dev['id'] ?? 'N/A')));
                        $devName = $dev['name'] ?? ($dev['device_name'] ?? 'Unnamed');
                        $devStatus = $dev['status'] ?? (!empty($dev['is_online']) ? 'online' : 'offline');
                        
                        $subName = 'None';
                        if (admin_table_exists($pdo, 'subscriptions')) {
                            $devSubs = admin_related_rows($pdo, 'subscriptions', $dev, 1);
                            if ($devSubs) {
                                $subName = $devSubs[0]['plan_name'] ?? ($devSubs[0]['plan_id'] ?? 'Active');
                                if (isset($devSubs[0]['status'])) {
                                    $subName .= ' (' . $devSubs[0]['status'] . ')';
                                }
                            }
                        }
                        
                        $deviceHtml .= '<tr><td>' . htmlspecialchars((string) $devId) . '</td><td>' . htmlspecialchars((string) $devName) . '</td><td>' . htmlspecialchars((string) $devStatus) . '</td><td>' . htmlspecialchars((string) $subName) . '</td></tr>';
                    }
                    $deviceHtml .= '</tbody></table>';
                }
            } else {
                $deviceHtml = '<p>Device association is unsupported in this schema.</p>';
            }

            // Fetch subscriptions
            $subHtml = '<p>No subscriptions found.</p>';
            if (admin_table_exists($pdo, 'subscriptions')) {
                $allSubs = [];
                foreach ($userDevices as $dev) {
                    $devSubs = admin_related_rows($pdo, 'subscriptions', $dev, 50);
                    $devId = $dev['device_id'] ?? ($dev['device_uid'] ?? ($dev['system_id'] ?? ($dev['id'] ?? 'N/A')));
                    foreach ($devSubs as $ds) {
                        $ds['_device_ref'] = $devId;
                        $allSubs[] = $ds;
                    }
                }
                
                if ($allSubs) {
                    $subHtml = '<table><thead><tr><th>Device ID</th><th>Plan</th><th>Status</th><th>Start Date</th><th>Expiry Date</th><th>Amount</th></tr></thead><tbody>';
                    foreach ($allSubs as $sub) {
                        $devRef = $sub['_device_ref'];
                        $planName = $sub['plan_name'] ?? ($sub['plan_id'] ?? 'N/A');
                        $subStatus = strtoupper((string) ($sub['status'] ?? 'Active'));
                        $start = $sub['start_date'] ?? ($sub['created_at'] ?? 'N/A');
                        $expires = $sub['expiry_date'] ?? ($sub['expires_at'] ?? 'N/A');
                        $amount = $sub['amount'] ?? 0;
                        
                        if (isset($sub['plan_id']) && admin_table_exists($pdo, 'plans') && admin_column_exists($pdo, 'plans', 'id') && admin_column_exists($pdo, 'plans', 'name')) {
                            $planStmt = $pdo->prepare('SELECT `name` FROM `plans` WHERE `id` = ? LIMIT 1');
                            $planStmt->execute([(int) $sub['plan_id']]);
                            $fetchedPlan = $planStmt->fetchColumn();
                            if ($fetchedPlan) $planName = $fetchedPlan;
                        }

                        $subHtml .= '<tr>'
                            . '<td>' . htmlspecialchars((string) $devRef) . '</td>'
                            . '<td>' . htmlspecialchars((string) $planName) . '</td>'
                            . '<td><span class="status-badge status-' . strtolower(str_replace(' ', '-', $subStatus)) . '">' . htmlspecialchars((string) $subStatus) . '</span></td>'
                            . '<td>' . htmlspecialchars((string) $start) . '</td>'
                            . '<td>' . htmlspecialchars((string) $expires) . '</td>'
                            . '<td>₹' . number_format((float) $amount, 2) . '</td>'
                            . '</tr>';
                    }
                    $subHtml .= '</tbody></table>';
                }
            } else {
                $subHtml = '<p>Subscription association is unsupported in this schema.</p>';
            }

            $body .= <<<HTML
<div class="section">
    <div class="section-header">
        <h3>User Details: {$name}</h3>
        <div class="mini-actions"><a class="outline-button" href="users.php">Back to Users</a></div>
    </div>
    <div class="detail-panels">
        <div class="detail-panel">
            <h4>Profile</h4>
            <div class="detail-row"><span>Email</span><strong>{$email}</strong></div>
            <div class="detail-row"><span>Status</span><strong><span class="status-badge status-{$status}">{$status}</span></strong></div>
            <div class="detail-row"><span>Created</span><strong>{$created}</strong></div>
        </div>
    </div>
</div>
<div class="section">
    <div class="section-header"><h3>Registered Devices</h3></div>
    <div style="overflow:auto;">
        {$deviceHtml}
    </div>
</div>
<div class="section">
    <div class="section-header"><h3>Subscriptions</h3></div>
    <div style="overflow:auto;">
        {$subHtml}
    </div>
</div>
HTML;
        }
    } catch (Throwable $e) {
        error_log('Users view error: ' . $e->getMessage());
        $body .= '<div class="section"><div class="section-header"><h3>Error</h3></div><p>Failed to load user details. Check the server error log.</p></div>';
    }
} else {
    $body .= <<<HTML
<div class="section">
    <div class="section-header">
        <h3>Users</h3>
    </div>
    <div style="overflow:auto;">
        <table>
            <thead>
                <tr>
                    <th>Name</th>
                    <th>Email</th>
                    <th>Device Count</th>
                    <th>Active Subscription</th>
                    <th>Status</th>
                    <th>Created Date</th>
                    <th>Actions</th>
                </tr>
            </thead>
            <tbody>
                {$rowHtml}
            </tbody>
        </table>
    </div>
</div>
HTML;
}

echo admin_render_page('Users', 'users', $body);
