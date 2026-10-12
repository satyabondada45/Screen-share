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
$planTable = admin_table_exists($pdo, 'plans') ? 'plans' : null;
$historyTable = admin_table_exists($pdo, 'subscription_history') ? 'subscription_history' : null;
$paymentTable = admin_table_exists($pdo, 'payments') ? 'payments' : null;
$userTable = admin_table_exists($pdo, 'users') ? 'users' : null;
$statusFilter = (string) ($_GET['status'] ?? 'all');
$subscriptionFilter = (string) ($_GET['subscription_status'] ?? 'all');
$statusFilter = in_array($statusFilter, ['all', 'online', 'offline'], true) ? $statusFilter : 'all';
$subscriptionFilter = in_array($subscriptionFilter, ['all', 'active', 'expired', 'expiring_soon', 'none'], true) ? $subscriptionFilter : 'all';
$search = trim((string) ($_GET['q'] ?? ''));
$page = max(1, (int) ($_GET['page'] ?? 1));
$perPage = 20;
$pageError = '';

if ($_SERVER['REQUEST_METHOD'] === 'POST' && isset($_POST['assign_plan'])) {
    if (!admin_verify_csrf_token($_POST['csrf_token'] ?? null)) {
        admin_set_flash('error', 'Invalid request token. Please refresh and try again.');
    } else {
        $assignDeviceId = (string) ($_POST['device_id'] ?? '');
        $planId = (int) ($_POST['plan_id'] ?? 0);

        if ($assignDeviceId === '' || $planId <= 0) {
            admin_set_flash('error', 'Invalid device or plan selection.');
        } else {
            try {
                // Get plan details
                $planStmt = $pdo->prepare('SELECT * FROM `plans` WHERE `id` = ? AND `status` = "active" LIMIT 1');
                $planStmt->execute([$planId]);
                $plan = $planStmt->fetch(PDO::FETCH_ASSOC);

                if (!$plan) {
                    throw new RuntimeException('The selected plan does not exist or is inactive.');
                }

                $duration = (int) ($plan['duration_days'] ?? 30);
                $price = (float) ($plan['price'] ?? 0);
                
                // Get device column name
                $devCol = 'device_id';
                if (admin_column_exists($pdo, 'subscriptions', 'system_id')) $devCol = 'system_id';
                elseif (admin_column_exists($pdo, 'subscriptions', 'device_uid')) $devCol = 'device_uid';

                $startDate = date('Y-m-d H:i:s');
                $expiryDate = date('Y-m-d H:i:s', strtotime("+{$duration} days"));
                
                $columns = [$devCol, 'plan_id', 'plan_name', 'status', 'start_date', 'expiry_date', 'amount'];
                $placeholders = ['?', '?', '?', '?', '?', '?', '?'];
                $values = [$assignDeviceId, $planId, $plan['name'], 'active', $startDate, $expiryDate, $price];

                // Check if user_id should be populated
                if (admin_column_exists($pdo, 'subscriptions', 'user_id')) {
                    $devQCol = 'id';
                    if (admin_column_exists($pdo, 'devices', 'system_id')) $devQCol = 'system_id';
                    elseif (admin_column_exists($pdo, 'devices', 'device_uid')) $devQCol = 'device_uid';
                    elseif (admin_column_exists($pdo, 'devices', 'device_id')) $devQCol = 'device_id';
                    
                    $uStmt = $pdo->prepare('SELECT `user_id` FROM `devices` WHERE `' . $devQCol . '` = ? LIMIT 1');
                    $uStmt->execute([$assignDeviceId]);
                    $devUser = $uStmt->fetchColumn();
                    if ($devUser) {
                        $columns[] = 'user_id';
                        $placeholders[] = '?';
                        $values[] = $devUser;
                    }
                }

                // Attempt to insert
                $sql = 'INSERT INTO `subscriptions` (' . implode(', ', array_map(fn($c) => "`$c`", $columns)) . ') VALUES (' . implode(', ', $placeholders) . ')';
                $insert = $pdo->prepare($sql);
                $insert->execute($values);

                admin_set_flash('success', 'Plan assigned successfully.');
                header('Location: ' . admin_url('devices.php', ['view' => $assignDeviceId]));
                exit;
            } catch (Throwable $e) {
                error_log('Plan assignment error: ' . $e->getMessage());
                admin_set_flash('error', 'Failed to assign plan: ' . $e->getMessage());
            }
        }
    }
}

$canJoinUser = $deviceTable && $userTable
    && admin_column_exists($pdo, $deviceTable, 'user_id')
    && admin_column_exists($pdo, $userTable, 'id');
$userSelect = [];
if ($canJoinUser && admin_column_exists($pdo, $userTable, 'email')) {
    $userSelect[] = 'u.`email` AS `owner_email`';
}
if ($canJoinUser && admin_column_exists($pdo, $userTable, 'username')) {
    $userSelect[] = 'u.`username` AS `owner_username`';
}
$deviceFrom = $deviceTable ? ' FROM `devices` d' . ($canJoinUser ? ' LEFT JOIN `users` u ON u.`id` = d.`user_id`' : '') : '';
$deviceSelect = $deviceTable ? 'SELECT d.*' . ($userSelect ? ', ' . implode(', ', $userSelect) : '') . $deviceFrom : '';

$where = [];
$parameters = [];
if ($search !== '' && $deviceTable) {
    $searchFields = [];
    foreach (['id', 'device_uid', 'system_id', 'name', 'device_name', 'machine_identifier'] as $column) {
        if (admin_column_exists($pdo, $deviceTable, $column)) {
            $searchFields[] = 'd.`' . $column . '` LIKE ?';
        }
    }
    if ($canJoinUser && admin_column_exists($pdo, $userTable, 'email')) {
        $searchFields[] = 'u.`email` LIKE ?';
    }
    if ($canJoinUser && admin_column_exists($pdo, $userTable, 'username')) {
        $searchFields[] = 'u.`username` LIKE ?';
    }
    $where[] = $searchFields ? '(' . implode(' OR ', $searchFields) . ')' : '1 = 0';
    foreach ($searchFields as $_) {
        $parameters[] = '%' . $search . '%';
    }
}

if ($deviceTable && $statusFilter !== 'all') {
    if (admin_column_exists($pdo, $deviceTable, 'is_online')) {
        $where[] = 'd.`is_online` = ?';
        $parameters[] = $statusFilter === 'online' ? 1 : 0;
    } elseif (admin_column_exists($pdo, $deviceTable, 'status')) {
        $where[] = 'LOWER(d.`status`) = ?';
        $parameters[] = $statusFilter;
    }
}

$subscriptionCorrelation = [];
if ($deviceTable && $subscriptionTable) {
    foreach (['device_id' => ['id', 'device_id', 'device_uid', 'system_id'], 'device_uid' => ['device_uid', 'system_id'], 'system_id' => ['system_id', 'device_uid']] as $subColumn => $deviceFields) {
        if (!admin_column_exists($pdo, $subscriptionTable, $subColumn)) {
            continue;
        }
        foreach ($deviceFields as $deviceField) {
            if (admin_column_exists($pdo, $deviceTable, $deviceField)) {
                $subscriptionCorrelation[] = 's.`' . $subColumn . '` = d.`' . $deviceField . '`';
            }
        }
    }
}

if ($subscriptionFilter !== 'all' && $subscriptionCorrelation) {
    $exists = 'EXISTS (SELECT 1 FROM `subscriptions` s WHERE (' . implode(' OR ', $subscriptionCorrelation) . ')';
    if ($subscriptionFilter === 'active' && admin_column_exists($pdo, $subscriptionTable, 'status')) {
        $where[] = $exists . ' AND LOWER(s.`status`) = "active")';
    } elseif ($subscriptionFilter === 'expired' && admin_column_exists($pdo, $subscriptionTable, 'expiry_date')) {
        $where[] = $exists . ' AND s.`expiry_date` < NOW())';
    } elseif ($subscriptionFilter === 'expiring_soon' && admin_column_exists($pdo, $subscriptionTable, 'expiry_date')) {
        $where[] = $exists . ' AND s.`expiry_date` BETWEEN NOW() AND DATE_ADD(NOW(), INTERVAL 7 DAY))';
    } elseif ($subscriptionFilter === 'none') {
        $where[] = 'NOT ' . $exists . ')';
    }
}

$selectedDevice = null;
$selectedKey = trim((string) ($_GET['view'] ?? $_GET['subscription'] ?? ''));
$deviceRows = [];
$totalRows = 0;
$totalPages = 1;
$relatedRows = static function (string $table, array $device, int $limit = 20, array $subIds = []) use ($pdo): array {
    return admin_related_rows($pdo, $table, $device, $limit, $subIds);
};

if ($deviceTable) {
    try {
        $whereSql = $where ? ' WHERE ' . implode(' AND ', $where) : '';
        $count = $pdo->prepare('SELECT COUNT(DISTINCT d.`id`)' . $deviceFrom . $whereSql);
        $count->execute($parameters);
        $totalRows = (int) $count->fetchColumn();
        $totalPages = max(1, (int) ceil($totalRows / $perPage));
        $page = min($page, $totalPages);
        $orderColumn = admin_column_exists($pdo, $deviceTable, 'last_seen_at')
            ? 'd.`last_seen_at`'
            : (admin_column_exists($pdo, $deviceTable, 'created_at') ? 'd.`created_at`' : 'd.`id`');
        $list = $pdo->prepare($deviceSelect . $whereSql . ' ORDER BY ' . $orderColumn . ' DESC LIMIT ' . (($page - 1) * $perPage) . ', ' . $perPage);
        $list->execute($parameters);
        $deviceRows = $list->fetchAll(PDO::FETCH_ASSOC);

        if ($selectedKey !== '') {
            $identity = [];
            $identityValues = [];
            foreach (['id', 'device_uid', 'system_id'] as $column) {
                if (admin_column_exists($pdo, $deviceTable, $column)) {
                    $identity[] = 'd.`' . $column . '` = ?';
                    $identityValues[] = $selectedKey;
                }
            }
            if ($identity) {
                $selectedStmt = $pdo->prepare($deviceSelect . ' WHERE (' . implode(' OR ', $identity) . ') LIMIT 1');
                $selectedStmt->execute($identityValues);
                $selectedDevice = $selectedStmt->fetch(PDO::FETCH_ASSOC) ?: null;
            }
        }
    } catch (Throwable $e) {
        error_log('Admin device query failed: ' . $e->getMessage());
        $pageError = 'Device records could not be loaded. Check the server error log for the database error.';
    }
}

$deviceRowsHtml = '';
foreach ($deviceRows as $device) {
    $deviceId = (string) ($device['system_id'] ?? ($device['device_uid'] ?? ($device['device_id'] ?? ($device['id'] ?? 'N/A'))));
    $deviceKey = (string) ($device['id'] ?? $deviceId);
    $deviceName = (string) ($device['name'] ?? ($device['device_name'] ?? 'Unnamed device'));
    $email = (string) ($device['owner_email'] ?? ($device['email'] ?? ($device['user_email'] ?? 'N/A')));
    $isOnline = !empty($device['is_online']);
    $status = (string) ($device['status'] ?? ($isOnline ? 'online' : 'offline'));
    $statusClass = in_array(strtolower($status), ['online', 'active'], true) ? 'active' : (in_array(strtolower($status), ['offline', 'inactive'], true) ? 'inactive' : strtolower($status));
    $registered = (string) ($device['created_at'] ?? 'N/A');
    $lastSeen = (string) ($device['last_seen_at'] ?? ($device['last_seen'] ?? 'N/A'));
    $subscriptions = $subscriptionTable ? $relatedRows($subscriptionTable, $device, 1) : [];
    $currentSubscription = $subscriptions[0] ?? [];
    $planName = (string) ($currentSubscription['plan_name'] ?? ($currentSubscription['plan'] ?? 'No subscription'));
    $subscriptionStatus = (string) ($currentSubscription['status'] ?? '');
    if (isset($currentSubscription['plan_id']) && $planTable && admin_column_exists($pdo, $planTable, 'id') && admin_column_exists($pdo, $planTable, 'name')) {
        $planStmt = $pdo->prepare('SELECT `name` FROM `plans` WHERE `id` = ? LIMIT 1');
        $planStmt->execute([(int) $currentSubscription['plan_id']]);
        $planName = (string) ($planStmt->fetchColumn() ?: $planName);
    }
    $expiry = (string) ($currentSubscription['expiry_date'] ?? ($device['expiry_date'] ?? 'N/A'));
    $recharge = (string) ($currentSubscription['recharge_date'] ?? ($currentSubscription['created_at'] ?? 'N/A'));
    if ($subscriptionStatus !== '') {
        $planName .= ' (' . $subscriptionStatus . ')';
    }
    $deviceRowsHtml .= '<tr>'
        . '<td style="max-width:120px; white-space:nowrap; overflow:hidden; text-overflow:ellipsis;" title="' . htmlspecialchars($deviceId, ENT_QUOTES, 'UTF-8') . '">' . htmlspecialchars($deviceId, ENT_QUOTES, 'UTF-8') . '</td>'
        . '<td style="max-width:150px; white-space:nowrap; overflow:hidden; text-overflow:ellipsis;" title="' . htmlspecialchars($deviceName, ENT_QUOTES, 'UTF-8') . '">' . htmlspecialchars($deviceName, ENT_QUOTES, 'UTF-8') . '</td>'
        . '<td style="max-width:150px; white-space:nowrap; overflow:hidden; text-overflow:ellipsis;" title="' . htmlspecialchars($email, ENT_QUOTES, 'UTF-8') . '">' . htmlspecialchars($email, ENT_QUOTES, 'UTF-8') . '</td>'
        . '<td><span class="status-badge status-' . htmlspecialchars($statusClass, ENT_QUOTES, 'UTF-8') . '">' . htmlspecialchars(strtoupper($status), ENT_QUOTES, 'UTF-8') . '</span></td>'
        . '<td style="max-width:150px; white-space:nowrap; overflow:hidden; text-overflow:ellipsis;" title="' . htmlspecialchars($planName, ENT_QUOTES, 'UTF-8') . '">' . htmlspecialchars($planName, ENT_QUOTES, 'UTF-8') . '</td>'
        . '<td>' . htmlspecialchars($recharge, ENT_QUOTES, 'UTF-8') . '</td>'
        . '<td>' . htmlspecialchars($expiry, ENT_QUOTES, 'UTF-8') . '</td>'
        . '<td style="max-width:100px; white-space:nowrap; overflow:hidden; text-overflow:ellipsis;" title="' . htmlspecialchars($lastSeen, ENT_QUOTES, 'UTF-8') . '">' . htmlspecialchars($lastSeen, ENT_QUOTES, 'UTF-8') . '</td>'
        . '<td><div class="mini-actions"><a class="outline-button" href="' . htmlspecialchars(admin_url('devices.php', ['view' => $deviceKey]), ENT_QUOTES, 'UTF-8') . '">View Details</a> <a class="outline-button" href="' . htmlspecialchars(admin_url('devices.php', ['subscription' => $deviceKey]), ENT_QUOTES, 'UTF-8') . '">Subscription</a></div></td>'
        . '</tr>';
}
if ($deviceRowsHtml === '') {
    $deviceRowsHtml = '<tr><td colspan="9" class="empty-state">' . ($deviceTable ? 'No remote devices found.' : 'The existing devices table is unavailable.') . '</td></tr>';
}

$detailsHtml = '';
if ($selectedDevice) {
    $deviceId = (string) ($selectedDevice['system_id'] ?? ($selectedDevice['device_uid'] ?? ($selectedDevice['id'] ?? 'N/A')));
    $deviceName = (string) ($selectedDevice['name'] ?? ($selectedDevice['device_name'] ?? 'Unnamed device'));
    $email = (string) ($selectedDevice['owner_email'] ?? ($selectedDevice['email'] ?? 'N/A'));
    $created = (string) ($selectedDevice['created_at'] ?? 'N/A');
    $lastSeen = (string) ($selectedDevice['last_seen_at'] ?? ($selectedDevice['last_seen'] ?? 'N/A'));
    $deviceStatus = (string) ($selectedDevice['status'] ?? (!empty($selectedDevice['is_online']) ? 'online' : 'offline'));
    $subscriptions = $subscriptionTable ? $relatedRows($subscriptionTable, $selectedDevice) : [];
    $subscriptionIds = array_values(array_filter(array_map(static fn($row) => $row['id'] ?? null, $subscriptions), static fn($id) => $id !== null));
    $currentSubscription = $subscriptions[0] ?? [];
    $planName = (string) ($currentSubscription['plan_name'] ?? ($currentSubscription['plan'] ?? 'N/A'));
    if (isset($currentSubscription['plan_id']) && $planTable && admin_column_exists($pdo, $planTable, 'id') && admin_column_exists($pdo, $planTable, 'name')) {
        $planStmt = $pdo->prepare('SELECT `name` FROM `plans` WHERE `id` = ? LIMIT 1');
        $planStmt->execute([(int) $currentSubscription['plan_id']]);
        $planName = (string) ($planStmt->fetchColumn() ?: $planName);
    }
    $history = $historyTable ? $relatedRows($historyTable, $selectedDevice, 20, $subscriptionIds) : [];
    $payments = $paymentTable ? $relatedRows($paymentTable, $selectedDevice, 20, $subscriptionIds) : [];
    $expiry = (string) ($currentSubscription['expiry_date'] ?? 'N/A');
    $daysRemaining = 'N/A';
    if ($expiry !== 'N/A' && ($expiryTimestamp = strtotime($expiry)) !== false) {
        $daysRemaining = (string) (int) ceil(($expiryTimestamp - strtotime('today')) / 86400);
    }
    $subscriptionStatus = (string) ($currentSubscription['status'] ?? 'No subscription');
    $historyHtml = '';
    foreach ($history as $item) {
        $historyHtml .= '<tr><td>' . htmlspecialchars((string) ($item['action'] ?? ($item['status'] ?? 'N/A')), ENT_QUOTES, 'UTF-8') . '</td><td>' . htmlspecialchars((string) ($item['start_date'] ?? ($item['created_at'] ?? 'N/A')), ENT_QUOTES, 'UTF-8') . '</td><td>' . htmlspecialchars((string) ($item['expiry_date'] ?? 'N/A'), ENT_QUOTES, 'UTF-8') . '</td><td>' . htmlspecialchars((string) ($item['payment_reference'] ?? 'N/A'), ENT_QUOTES, 'UTF-8') . '</td></tr>';
    }
    if ($historyHtml === '') {
        $historyHtml = '<tr><td colspan="4" class="empty-state">No subscription history found.</td></tr>';
    }
    $paymentHtml = '';
    foreach ($payments as $payment) {
        $paymentHtml .= '<tr><td>' . htmlspecialchars((string) ($payment['created_at'] ?? 'N/A'), ENT_QUOTES, 'UTF-8') . '</td><td>' . htmlspecialchars((string) ($payment['amount'] ?? 'N/A'), ENT_QUOTES, 'UTF-8') . '</td><td>' . htmlspecialchars((string) ($payment['status'] ?? 'N/A'), ENT_QUOTES, 'UTF-8') . '</td><td>' . htmlspecialchars((string) ($payment['payment_reference'] ?? 'N/A'), ENT_QUOTES, 'UTF-8') . '</td></tr>';
    }
    if ($paymentHtml === '') {
        $paymentHtml = '<tr><td colspan="4" class="empty-state">No payment records found.</td></tr>';
    }

    $formatDate = static function(string $d): string {
        if ($d === 'N/A' || $d === '') return 'N/A';
        $time = strtotime($d);
        return $time ? date('M j, Y g:i A', $time) : 'N/A';
    };

    $detailsHtml = '<div style="margin-bottom: 24px;"><a href="' . htmlspecialchars(admin_url('devices.php'), ENT_QUOTES, 'UTF-8') . '" class="outline-button">← Back to Devices</a></div>'
        . '<div class="section"><div class="section-header" style="display:flex; justify-content:space-between; align-items:center; flex-wrap:wrap; gap:16px;">'
        . '<div><h2 style="margin:0 0 4px; font-size:1.4rem;">' . htmlspecialchars($deviceName, ENT_QUOTES, 'UTF-8') . '</h2>'
        . '<div style="color:var(--muted); font-size:0.9rem;">' . htmlspecialchars($email, ENT_QUOTES, 'UTF-8') . '</div></div>'
        . '<div style="text-align:right;"><span class="status-badge status-' . htmlspecialchars($deviceStatus === 'online' ? 'active' : 'inactive', ENT_QUOTES, 'UTF-8') . '" style="margin-bottom:6px;">' . htmlspecialchars(strtoupper($deviceStatus), ENT_QUOTES, 'UTF-8') . '</span>'
        . '<div style="color:var(--muted); font-size:0.85rem;">Device ID: <strong>' . htmlspecialchars($deviceId, ENT_QUOTES, 'UTF-8') . '</strong></div></div>'
        . '</div>'
        . '<div class="detail-panels">'
        . '<div class="detail-panel"><h4>DEVICE INFORMATION</h4>'
        . '<div class="detail-row"><span>Device ID</span><strong>' . htmlspecialchars($deviceId, ENT_QUOTES, 'UTF-8') . '</strong></div>'
        . '<div class="detail-row"><span>Device Name</span><strong>' . htmlspecialchars($deviceName, ENT_QUOTES, 'UTF-8') . '</strong></div>'
        . '<div class="detail-row"><span>Email</span><strong>' . htmlspecialchars($email, ENT_QUOTES, 'UTF-8') . '</strong></div>'
        . '<div class="detail-row"><span>Status</span><strong>' . htmlspecialchars(strtoupper($deviceStatus), ENT_QUOTES, 'UTF-8') . '</strong></div>'
        . '<div class="detail-row"><span>Registered</span><strong>' . htmlspecialchars($formatDate($created), ENT_QUOTES, 'UTF-8') . '</strong></div>'
        . '<div class="detail-row"><span>Last Seen</span><strong>' . htmlspecialchars($formatDate($lastSeen), ENT_QUOTES, 'UTF-8') . '</strong></div>'
        . '</div>'
        . '<div class="detail-panel"><h4>SUBSCRIPTION</h4>'
        . '<div class="detail-row"><span>Plan</span><strong>' . htmlspecialchars($planName, ENT_QUOTES, 'UTF-8') . '</strong></div>'
        . '<div class="detail-row"><span>Start Date</span><strong>' . htmlspecialchars($formatDate((string) ($currentSubscription['created_at'] ?? 'N/A')), ENT_QUOTES, 'UTF-8') . '</strong></div>'
        . '<div class="detail-row"><span>Expiry Date</span><strong>' . htmlspecialchars($formatDate($expiry), ENT_QUOTES, 'UTF-8') . '</strong></div>'
        . '<div class="detail-row"><span>Status</span><strong>' . htmlspecialchars($subscriptionStatus, ENT_QUOTES, 'UTF-8') . '</strong></div>'
        . '<div class="detail-row"><span>Days Remaining</span><strong>' . htmlspecialchars($daysRemaining, ENT_QUOTES, 'UTF-8') . '</strong></div>'
        . '</div>'
        . '</div>'
        . '<div class="section-header"><h3>ACTIONS</h3></div><div style="padding:16px 20px;">';

    $csrfToken = htmlspecialchars(admin_csrf_token(), ENT_QUOTES, 'UTF-8');
    $planOptions = '<option value="">-- Select a Plan --</option>';
    if ($planTable) {
        $pStmt = $pdo->query('SELECT * FROM `plans` WHERE `status` = "active"');
        foreach ($pStmt->fetchAll(PDO::FETCH_ASSOC) as $pRow) {
            $planOptions .= '<option value="' . (int)$pRow['id'] . '">' . htmlspecialchars($pRow['name'], ENT_QUOTES, 'UTF-8') . ' (₹' . number_format((float)$pRow['price'], 2) . ' for ' . (int)$pRow['duration_days'] . ' days)</option>';
        }
    }

    $detailsHtml .= '<form method="POST" style="display:flex; align-items:flex-end; gap:12px; flex-wrap:wrap;">'
        . '<input type="hidden" name="csrf_token" value="' . $csrfToken . '">'
        . '<input type="hidden" name="assign_plan" value="1">'
        . '<input type="hidden" name="device_id" value="' . htmlspecialchars($deviceId, ENT_QUOTES, 'UTF-8') . '">'
        . '<div class="field"><label>Assign New Plan</label><select name="plan_id" required>' . $planOptions . '</select></div>'
        . '<button type="submit" class="button">Assign Plan</button>'
        . '</form></div></div>';
} elseif ($selectedKey !== '') {
    $detailsHtml = '<div style="margin-bottom: 24px;"><a href="' . htmlspecialchars(admin_url('devices.php'), ENT_QUOTES, 'UTF-8') . '" class="outline-button">← Back to Devices</a></div><div class="section"><div class="empty-state" style="padding: 60px 20px;"><h3>Device Not Found</h3><p style="color:var(--muted); margin-top:8px;">The requested device does not exist or has been removed.</p></div></div>';
}

$pageErrorHtml = $pageError !== '' ? '<div class="flash error">' . htmlspecialchars($pageError, ENT_QUOTES, 'UTF-8') . '</div>' : '';
$devicesUrl = htmlspecialchars(admin_url('devices.php'), ENT_QUOTES, 'UTF-8');
$searchValue = htmlspecialchars($search, ENT_QUOTES, 'UTF-8');
$statusOptions = '';
foreach (['all' => 'All statuses', 'online' => 'Online', 'offline' => 'Offline'] as $value => $label) {
    $statusOptions .= '<option value="' . $value . '"' . ($statusFilter === $value ? ' selected' : '') . '>' . $label . '</option>';
}
$subscriptionOptions = '';
foreach (['all' => 'All subscriptions', 'active' => 'Active', 'expired' => 'Expired', 'expiring_soon' => 'Expiring soon', 'none' => 'No subscription'] as $value => $label) {
    $subscriptionOptions .= '<option value="' . $value . '"' . ($subscriptionFilter === $value ? ' selected' : '') . '>' . $label . '</option>';
}
$paginationHtml = '';
for ($i = 1; $i <= $totalPages; $i++) {
    $paginationHtml .= '<a class="page-btn ' . ($i === $page ? 'current' : '') . '" href="' . htmlspecialchars(admin_url('devices.php', ['page' => $i, 'status' => $statusFilter, 'subscription_status' => $subscriptionFilter, 'q' => $search]), ENT_QUOTES, 'UTF-8') . '">' . $i . '</a>';
}

if ($selectedKey !== '') {
    $body = $pageErrorHtml . $detailsHtml;
} else {
    $body = $pageErrorHtml . '
    <div class="section">
        <div class="section-header">
            <h3>Remote Devices</h3>
            <form method="get" action="' . $devicesUrl . '" class="filters" id="devicesFilterForm">
                <input type="search" name="q" id="searchInput" value="' . $searchValue . '" placeholder="Search ID, name, or email">
                <select name="status" id="statusSelect">' . $statusOptions . '</select>
                <select name="subscription_status" id="subscriptionSelect">' . $subscriptionOptions . '</select>
                <button class="button" type="submit">Filter</button>
                ' . (($search !== '' || $statusFilter !== 'all' || $subscriptionFilter !== 'all') ? '<a href="' . $devicesUrl . '" class="ghost-button" style="color:var(--muted); text-decoration:none; padding:8px; display:inline-flex; align-items:center;">Clear Filters</a>' : '') . '
            </form>
        </div>
        <div style="margin-bottom: 12px; font-size: 0.85rem; color: var(--muted);">Showing ' . count($deviceRows) . ' of ' . $totalRows . ' device' . ($totalRows !== 1 ? 's' : '') . '</div>
        <div style="overflow:auto"><table><thead><tr><th>Device ID</th><th>Device Name</th><th>Email / Owner</th><th>Status</th><th>Subscription</th><th>Recharge Date</th><th>Expiry Date</th><th>Last Seen</th><th>Actions</th></tr></thead><tbody>' . $deviceRowsHtml . '</tbody></table></div>
        <div class="pagination">' . $paginationHtml . '</div>
    </div>
    <script>
        (function() {
            var form = document.getElementById("devicesFilterForm");
            var searchInput = document.getElementById("searchInput");
            var statusSelect = document.getElementById("statusSelect");
            var subscriptionSelect = document.getElementById("subscriptionSelect");
            var timeout = null;

            if (!form) return;

            function submitForm() {
                // If inputs are empty/default, we can just navigate to base URL, but form submit is fine.
                form.submit();
            }

            // Auto-submit on select change
            if (statusSelect) statusSelect.addEventListener("change", submitForm);
            if (subscriptionSelect) subscriptionSelect.addEventListener("change", submitForm);

            // Debounced auto-submit on typing
            if (searchInput) {
                searchInput.addEventListener("input", function() {
                    clearTimeout(timeout);
                    timeout = setTimeout(submitForm, 500);
                });
            }
        })();
    </script>';
}

echo admin_render_page('Remote Devices', 'devices', $body);
