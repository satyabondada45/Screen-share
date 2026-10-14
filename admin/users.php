<?php
require_once __DIR__ . '/includes/db.php';
require_once __DIR__ . '/includes/auth.php';
require_once __DIR__ . '/includes/layout.php';

admin_require_auth();

try {
    $pdo = admin_db();
} catch (Throwable $e) {
    error_log('Admin database initialization failed: ' . $e->getMessage());
    echo admin_render_page('System Error', 'error', '<div class="flash error">Unable to connect to the database.</div>');
    exit;
}

$userTable = admin_table_exists($pdo, 'users') ? 'users' : null;
if (!$userTable) {
    echo admin_render_page('Users', 'users', '<div class="flash error">The users database table is unavailable.</div>');
    exit;
}

// ─────────────────────────────────────────────────────────────────────────────
// DETAIL VIEW (view = USER_ID)
// ─────────────────────────────────────────────────────────────────────────────
if (isset($_GET['view'])) {
    $viewId = (int) $_GET['view'];
    $stmt = $pdo->prepare('SELECT * FROM `users` WHERE `id` = ? LIMIT 1');
    $stmt->execute([$viewId]);
    $user = $stmt->fetch(PDO::FETCH_ASSOC);

    if (!$user) {
        admin_set_flash('error', 'The requested user could not be found.');
        header('Location: ' . admin_url('users.php'));
        exit;
    }

    $name = htmlspecialchars((string) ($user['name'] ?? ($user['username'] ?? 'User #' . $user['id'])), ENT_QUOTES, 'UTF-8');
    $email = htmlspecialchars((string) ($user['email'] ?? 'N/A'), ENT_QUOTES, 'UTF-8');
    $username = htmlspecialchars((string) ($user['username'] ?? '—'), ENT_QUOTES, 'UTF-8');
    $uniqueId = htmlspecialchars((string) ($user['unique_user_id'] ?? '—'), ENT_QUOTES, 'UTF-8');
    $role = strtoupper((string) ($user['role'] ?? 'USER'));
    $status = strtoupper((string) ($user['status'] ?? 'ACTIVE'));
    $statusClass = strtolower($status);
    $created = !empty($user['created_at']) ? htmlspecialchars((string) $user['created_at'], ENT_QUOTES, 'UTF-8') : 'N/A';

    // 1. User Devices
    $userDevices = [];
    if (admin_table_exists($pdo, 'devices')) {
        $devStmt = $pdo->prepare('SELECT * FROM `devices` WHERE `user_id` = ? ORDER BY id DESC');
        $devStmt->execute([$viewId]);
        $userDevices = $devStmt->fetchAll(PDO::FETCH_ASSOC);
    }

    $deviceHtml = '';
    if (!empty($userDevices)) {
        $deviceHtml = '<table><thead><tr><th>System ID</th><th>Device Name</th><th>Status</th><th>OS</th><th>IP Address</th><th>Last Seen</th><th>Actions</th></tr></thead><tbody>';
        foreach ($userDevices as $dev) {
            $devId = (int) $dev['id'];
            $sysId = htmlspecialchars((string) ($dev['system_id'] ?? ($dev['device_uid'] ?? 'ID#' . $devId)), ENT_QUOTES, 'UTF-8');
            $devName = htmlspecialchars((string) ($dev['name'] ?? 'Unnamed'), ENT_QUOTES, 'UTF-8');
            $isOnline = !empty($dev['is_online']);
            $devStatus = $isOnline ? '<span class="status-badge status-active">Online</span>' : '<span class="status-badge status-inactive">Offline</span>';
            $os = htmlspecialchars((string) ($dev['os_type'] ?? '—'), ENT_QUOTES, 'UTF-8');
            $ip = htmlspecialchars((string) ($dev['ip_address'] ?? '—'), ENT_QUOTES, 'UTF-8');
            $lastSeen = admin_time_ago($dev['last_seen_at'] ?? null);
            $viewDevUrl = htmlspecialchars(admin_url('devices.php', ['view' => $devId]), ENT_QUOTES, 'UTF-8');

            $deviceHtml .= "<tr>
                <td><code>{$sysId}</code></td>
                <td><strong>{$devName}</strong></td>
                <td>{$devStatus}</td>
                <td>{$os}</td>
                <td>{$ip}</td>
                <td><small>{$lastSeen}</small></td>
                <td><a class=\"outline-button\" href=\"{$viewDevUrl}\">View Device</a></td>
            </tr>";
        }
        $deviceHtml .= '</tbody></table>';
    } else {
        $deviceHtml = '<div class="empty-state">No remote devices are registered to this user account.</div>';
    }

    // 2. User Subscriptions
    $userSubs = [];
    if (admin_table_exists($pdo, 'subscriptions')) {
        $subStmt = $pdo->prepare("
            SELECT s.*, p.name AS plan_name, p.price, p.duration_unit
            FROM `subscriptions` s
            LEFT JOIN `plans` p ON s.plan_id = p.id
            WHERE s.user_id = ?
            ORDER BY s.id DESC
        ");
        $subStmt->execute([$viewId]);
        $userSubs = $subStmt->fetchAll(PDO::FETCH_ASSOC);
    }

    $subHtml = '';
    if (!empty($userSubs)) {
        $subHtml = '<table><thead><tr><th>Sub ID</th><th>Plan</th><th>Price</th><th>Starts At</th><th>Expires At</th><th>Status</th></tr></thead><tbody>';
        foreach ($userSubs as $sub) {
            $subId = (int) $sub['id'];
            $planName = htmlspecialchars((string) ($sub['plan_name'] ?? 'Plan #' . $sub['plan_id']), ENT_QUOTES, 'UTF-8');
            $price = '₹' . number_format((float) ($sub['price'] ?? 0), 2);
            $startsAt = !empty($sub['starts_at']) ? htmlspecialchars((string) $sub['starts_at'], ENT_QUOTES, 'UTF-8') : 'N/A';
            $expVal = $sub['expires_at'] ?? null;
            if (empty($expVal) || ($sub['duration_unit'] ?? '') === 'unlimited') {
                $expiresAt = '<span style="color:#16a34a; font-weight:600;">Unlimited</span>';
            } else {
                $expiresAt = htmlspecialchars((string) $expVal, ENT_QUOTES, 'UTF-8');
            }
            $subStat = strtoupper((string) ($sub['status'] ?? 'ACTIVE'));
            $subClass = strtolower($subStat);

            $subHtml .= "<tr>
                <td>#{$subId}</td>
                <td><strong>{$planName}</strong></td>
                <td>{$price}</td>
                <td>{$startsAt}</td>
                <td>{$expiresAt}</td>
                <td><span class=\"status-badge status-{$subClass}\">{$subStat}</span></td>
            </tr>";
        }
        $subHtml .= '</tbody></table>';
    } else {
        $subHtml = '<div class="empty-state">No subscriptions have been assigned to this user yet.</div>';
    }

    // 3. User Remote Sessions
    $userSessions = [];
    if (admin_table_exists($pdo, 'sessions') && !empty($userDevices)) {
        $deviceIds = array_column($userDevices, 'id');
        if (!empty($deviceIds)) {
            $placeholders = implode(',', array_fill(0, count($deviceIds), '?'));
            $sessStmt = $pdo->prepare("
                SELECT s.*, d.name AS device_name
                FROM `sessions` s
                LEFT JOIN `devices` d ON s.host_device_id = d.id
                WHERE s.host_device_id IN ({$placeholders})
                ORDER BY s.id DESC
                LIMIT 20
            ");
            $sessStmt->execute($deviceIds);
            $userSessions = $sessStmt->fetchAll(PDO::FETCH_ASSOC);
        }
    }

    $sessHtml = '';
    if (!empty($userSessions)) {
        $sessHtml = '<table><thead><tr><th>Session Key</th><th>Device</th><th>Status</th><th>Bandwidth</th><th>Duration</th><th>Started</th></tr></thead><tbody>';
        foreach ($userSessions as $s) {
            $key = htmlspecialchars((string) ($s['session_key'] ?? 'N/A'), ENT_QUOTES, 'UTF-8');
            $dName = htmlspecialchars((string) ($s['device_name'] ?? 'Device #' . $s['host_device_id']), ENT_QUOTES, 'UTF-8');
            $st = strtoupper((string) ($s['status'] ?? 'COMPLETED'));
            $stClass = strtolower($st);
            $bw = admin_format_bytes((float) ($s['bandwidth_used_mb'] ?? 0) * 1024 * 1024);
            $dur = admin_format_duration($s['started_at'] ?? null, $s['ended_at'] ?? null);
            $start = admin_time_ago($s['started_at'] ?? null);

            $sessHtml .= "<tr>
                <td><code>{$key}</code></td>
                <td>{$dName}</td>
                <td><span class=\"status-badge status-{$stClass}\">{$st}</span></td>
                <td>{$bw}</td>
                <td>{$dur}</td>
                <td><small>{$start}</small></td>
            </tr>";
        }
        $sessHtml .= '</tbody></table>';
    } else {
        $sessHtml = '<div class="empty-state">No remote sessions recorded for this user\'s devices.</div>';
    }

    // 4. User Payments
    $userPayments = [];
    if (admin_table_exists($pdo, 'payments')) {
        $payStmt = $pdo->prepare('SELECT * FROM `payments` WHERE `user_id` = ? ORDER BY id DESC');
        $payStmt->execute([$viewId]);
        $userPayments = $payStmt->fetchAll(PDO::FETCH_ASSOC);
    }

    $payHtml = '';
    if (!empty($userPayments)) {
        $payHtml = '<table><thead><tr><th>Payment ID</th><th>Transaction ID</th><th>Amount</th><th>Method</th><th>Status</th><th>Date</th></tr></thead><tbody>';
        foreach ($userPayments as $p) {
            $pId = (int) $p['id'];
            $txn = htmlspecialchars((string) ($p['transaction_id'] ?? '—'), ENT_QUOTES, 'UTF-8');
            $amt = '₹' . number_format((float) ($p['amount'] ?? 0), 2);
            $meth = htmlspecialchars((string) ($p['payment_method'] ?? 'Online'), ENT_QUOTES, 'UTF-8');
            $pStat = strtoupper((string) ($p['status'] ?? 'COMPLETED'));
            $pClass = strtolower($pStat);
            $pDate = !empty($p['created_at']) ? htmlspecialchars((string) $p['created_at'], ENT_QUOTES, 'UTF-8') : 'N/A';

            $payHtml .= "<tr>
                <td>#{$pId}</td>
                <td><code>{$txn}</code></td>
                <td>{$amt}</td>
                <td>{$meth}</td>
                <td><span class=\"status-badge status-{$pClass}\">{$pStat}</span></td>
                <td>{$pDate}</td>
            </tr>";
        }
        $payHtml .= '</tbody></table>';
    } else {
        $payHtml = '<div class="empty-state">No payment records found for this user.</div>';
    }

    $usersBackUrl = htmlspecialchars(admin_url('users.php'), ENT_QUOTES, 'UTF-8');

    $body = <<<HTML
<div class="section">
    <div class="section-header">
        <h3>User Profile: {$name}</h3>
        <div class="mini-actions"><a class="outline-button" href="{$usersBackUrl}">&larr; Back to Users</a></div>
    </div>
    <div class="detail-panels">
        <div class="detail-panel">
            <h4>Account Information</h4>
            <div class="detail-row"><span>User ID</span><strong>#{$viewId}</strong></div>
            <div class="detail-row"><span>Unique Identifier</span><strong><code>{$uniqueId}</code></strong></div>
            <div class="detail-row"><span>Full Name</span><strong>{$name}</strong></div>
            <div class="detail-row"><span>Username</span><strong>{$username}</strong></div>
            <div class="detail-row"><span>Email</span><strong>{$email}</strong></div>
            <div class="detail-row"><span>Role</span><strong><span class="status-badge">{$role}</span></strong></div>
            <div class="detail-row"><span>Status</span><strong><span class="status-badge status-{$statusClass}">{$status}</span></strong></div>
            <div class="detail-row"><span>Registered Date</span><strong>{$created}</strong></div>
        </div>
        <div class="detail-panel">
            <h4>Activity Summary</h4>
            <div class="detail-row"><span>Registered Devices</span><strong>count: </strong><span>(count: count)</span></div>
            <div class="detail-row"><span>Total Devices</span><strong>count: </strong><span>(</span></div>
            <div class="detail-row"><span>Devices Total</span><strong>count: </strong><span>count</span></div>
            <div class="detail-row"><span>Devices Registered</span><strong>count: </strong><span>count</span></div>
            <div class="detail-row"><span>Registered Devices</span><strong>count: </strong><span>count</span></div>
        </div>
    </div>
</div>
HTML;

    // Build cleaner details
    $devCount = count($userDevices);
    $subCount = count($userSubs);
    $sessCount = count($userSessions);
    $payCount = count($userPayments);

    $body = <<<HTML
<div class="section">
    <div class="section-header">
        <h3>User Profile: {$name}</h3>
        <div class="mini-actions"><a class="outline-button" href="{$usersBackUrl}">&larr; Back to Users</a></div>
    </div>
    <div class="detail-panels">
        <div class="detail-panel">
            <h4>Account Information</h4>
            <div class="detail-row"><span>User ID</span><strong>#{$viewId}</strong></div>
            <div class="detail-row"><span>Unique User ID</span><strong><code>{$uniqueId}</code></strong></div>
            <div class="detail-row"><span>Full Name</span><strong>{$name}</strong></div>
            <div class="detail-row"><span>Username</span><strong>{$username}</strong></div>
            <div class="detail-row"><span>Email Address</span><strong>{$email}</strong></div>
            <div class="detail-row"><span>Role</span><strong><span class="status-badge">{$role}</span></strong></div>
            <div class="detail-row"><span>Account Status</span><strong><span class="status-badge status-{$statusClass}">{$status}</span></strong></div>
            <div class="detail-row"><span>Registered On</span><strong>{$created}</strong></div>
        </div>
        <div class="detail-panel">
            <h4>Associated Assets & Activity</h4>
            <div class="detail-row"><span>Registered Devices</span><strong>{$devCount} devices</strong></div>
            <div class="detail-row"><span>Assigned Subscriptions</span><strong>{$subCount} plans</strong></div>
            <div class="detail-row"><span>Recorded Sessions</span><strong>{$sessCount} sessions</strong></div>
            <div class="detail-row"><span>Payment Transactions</span><strong>{$payCount} records</strong></div>
        </div>
    </div>
</div>

<div class="section">
    <div class="section-header">
        <h3>Registered Devices ({$devCount})</h3>
    </div>
    <div style="overflow:auto;">
        {$deviceHtml}
    </div>
</div>

<div class="section">
    <div class="section-header">
        <h3>Assigned Subscriptions ({$subCount})</h3>
    </div>
    <div style="overflow:auto;">
        {$subHtml}
    </div>
</div>

<div class="section">
    <div class="section-header">
        <h3>Recent Remote Sessions ({$sessCount})</h3>
    </div>
    <div style="overflow:auto;">
        {$sessHtml}
    </div>
</div>

<div class="section">
    <div class="section-header">
        <h3>Payments & Transactions ({$payCount})</h3>
    </div>
    <div style="overflow:auto;">
        {$payHtml}
    </div>
</div>
HTML;

    echo admin_render_page('User Details', 'users', $body);
    exit;
}

// ─────────────────────────────────────────────────────────────────────────────
// LIST VIEW WITH SERVER-SIDE PAGINATION, SEARCH, SORTING & METRICS
// ─────────────────────────────────────────────────────────────────────────────

// Filters & Query Parameters
$search = trim((string) ($_GET['q'] ?? ''));
$statusFilter = strtolower(trim((string) ($_GET['status'] ?? 'all')));
$roleFilter = strtolower(trim((string) ($_GET['role'] ?? 'all')));
$statusFilter = in_array($statusFilter, ['all', 'active', 'inactive', 'suspended'], true) ? $statusFilter : 'all';
$roleFilter = in_array($roleFilter, ['all', 'user', 'admin'], true) ? $roleFilter : 'all';

// Sorting Allowlist
$allowedSorts = [
    'id' => 'u.id',
    'name' => 'u.name',
    'email' => 'u.email',
    'role' => 'u.role',
    'status' => 'u.status',
    'created_at' => 'u.created_at',
    'device_count' => 'device_count',
];
$sort = (string) ($_GET['sort'] ?? 'id');
$sortColumn = $allowedSorts[$sort] ?? 'u.id';
$dir = strtolower((string) ($_GET['dir'] ?? 'desc')) === 'asc' ? 'ASC' : 'DESC';

// Pagination
$page = max(1, (int) ($_GET['page'] ?? 1));
$perPage = 15;
$offset = ($page - 1) * $perPage;

// Build WHERE Clause
$where = [];
$params = [];

if ($search !== '') {
    $where[] = '(u.name LIKE ? OR u.email LIKE ? OR u.username LIKE ? OR u.unique_user_id LIKE ?)';
    $params[] = '%' . $search . '%';
    $params[] = '%' . $search . '%';
    $params[] = '%' . $search . '%';
    $params[] = '%' . $search . '%';
}

if ($statusFilter !== 'all') {
    $where[] = 'LOWER(u.status) = ?';
    $params[] = $statusFilter;
}

if ($roleFilter !== 'all') {
    $where[] = 'LOWER(u.role) = ?';
    $params[] = $roleFilter;
}

$whereSql = $where ? ' WHERE ' . implode(' AND ', $where) : '';

// 1. Total Count for Pagination
$countStmt = $pdo->prepare('SELECT COUNT(*) FROM `users` u' . $whereSql);
$countStmt->execute($params);
$totalUsersCount = (int) $countStmt->fetchColumn();

// 2. Fetch Aggregated User Records
$sql = "
    SELECT u.*,
           (SELECT COUNT(*) FROM `devices` d WHERE d.user_id = u.id) AS device_count,
           (SELECT p.name 
            FROM `subscriptions` s 
            JOIN `plans` p ON s.plan_id = p.id 
            WHERE s.user_id = u.id AND s.status = 'active' 
            ORDER BY s.id DESC LIMIT 1) AS active_plan_name
    FROM `users` u
    {$whereSql}
    ORDER BY {$sortColumn} {$dir}
    LIMIT {$perPage} OFFSET {$offset}
";

$stmt = $pdo->prepare($sql);
$stmt->execute($params);
$userRows = $stmt->fetchAll(PDO::FETCH_ASSOC);

// Top KPI Metric Summary Counters
$kpiTotal = (int) $pdo->query('SELECT COUNT(*) FROM `users`')->fetchColumn();
$kpiActive = (int) $pdo->query('SELECT COUNT(*) FROM `users` WHERE LOWER(status) = "active"')->fetchColumn();
$kpiWithDevices = (int) $pdo->query('SELECT COUNT(DISTINCT user_id) FROM `devices` WHERE user_id IS NOT NULL')->fetchColumn();
$kpiWithActiveSub = (int) $pdo->query('SELECT COUNT(DISTINCT user_id) FROM `subscriptions` WHERE status = "active"')->fetchColumn();

// Build Table Rows
$rowHtml = '';
if (!empty($userRows)) {
    foreach ($userRows as $u) {
        $uid = (int) $u['id'];
        $uniqueId = htmlspecialchars((string) ($u['unique_user_id'] ?? '—'), ENT_QUOTES, 'UTF-8');
        $uName = htmlspecialchars((string) ($u['name'] ?? ($u['username'] ?? 'User #' . $uid)), ENT_QUOTES, 'UTF-8');
        $uEmail = htmlspecialchars((string) ($u['email'] ?? 'N/A'), ENT_QUOTES, 'UTF-8');
        $devCount = (int) ($u['device_count'] ?? 0);
        $activePlan = htmlspecialchars((string) ($u['active_plan_name'] ?? 'None'), ENT_QUOTES, 'UTF-8');
        $uRole = strtoupper((string) ($u['role'] ?? 'USER'));
        $uStatus = strtoupper((string) ($u['status'] ?? 'ACTIVE'));
        $uStatusClass = strtolower($uStatus);
        $created = admin_utc_date($u['created_at'] ?? null);
        $viewUrl = htmlspecialchars(admin_url('users.php', ['view' => $uid]), ENT_QUOTES, 'UTF-8');

        $rowHtml .= "<tr>
            <td>#{$uid}</td>
            <td><strong>{$uName}</strong></td>
            <td>{$uEmail}</td>
            <td><span class=\"status-badge\">{$uRole}</span></td>
            <td><span class=\"kpi-pill\">{$devCount}</span></td>
            <td>{$activePlan}</td>
            <td><span class=\"status-badge status-{$uStatusClass}\">{$uStatus}</span></td>
            <td>{$created}</td>
            <td><a class=\"outline-button\" href=\"{$viewUrl}\">View</a></td>
        </tr>";
    }
} else {
    $rowHtml = '<tr><td colspan="9" class="empty-state">No matching users found for this query.</td></tr>';
}

// Build Filter Form & Pagination Controls
$usersUrl = htmlspecialchars(admin_url('users.php'), ENT_QUOTES, 'UTF-8');
$queryParams = array_filter([
    'q' => $search !== '' ? $search : null,
    'status' => $statusFilter !== 'all' ? $statusFilter : null,
    'role' => $roleFilter !== 'all' ? $roleFilter : null,
    'sort' => $sort !== 'id' ? $sort : null,
    'dir' => $dir !== 'DESC' ? strtolower($dir) : null,
]);

$paginationHtml = admin_pagination($totalUsersCount, $perPage, $page, 'users.php', $queryParams);

$statusOptions = '';
foreach (['all' => 'All Statuses', 'active' => 'Active', 'inactive' => 'Inactive', 'suspended' => 'Suspended'] as $v => $l) {
    $statusOptions .= '<option value="' . $v . '"' . ($statusFilter === $v ? ' selected' : '') . '>' . $l . '</option>';
}

$roleOptions = '';
foreach (['all' => 'All Roles', 'user' => 'User', 'admin' => 'Admin'] as $v => $l) {
    $roleOptions .= '<option value="' . $v . '"' . ($roleFilter === $v ? ' selected' : '') . '>' . $l . '</option>';
}

$searchSafe = htmlspecialchars($search, ENT_QUOTES, 'UTF-8');

$thId = admin_sort_header('id', 'ID', $sort, $dir, 'users.php', $queryParams);
$thName = admin_sort_header('name', 'Name', $sort, $dir, 'users.php', $queryParams);
$thEmail = admin_sort_header('email', 'Email', $sort, $dir, 'users.php', $queryParams);
$thRole = admin_sort_header('role', 'Role', $sort, $dir, 'users.php', $queryParams);
$thDevices = admin_sort_header('device_count', 'Devices', $sort, $dir, 'users.php', $queryParams);
$thStatus = admin_sort_header('status', 'Status', $sort, $dir, 'users.php', $queryParams);
$thCreated = admin_sort_header('created_at', 'Registered', $sort, $dir, 'users.php', $queryParams);

$body = <<<HTML
<!-- User Metrics Cards -->
<div class="card-grid">
    <div class="card">
        <span class="label">Total Registered Users</span>
        <p class="value">{$kpiTotal}</p>
        <div class="trend">All-time customer accounts</div>
    </div>
    <div class="card">
        <span class="label">Active Users</span>
        <p class="value">{$kpiActive}</p>
        <div class="trend">Accounts in active standing</div>
    </div>
    <div class="card">
        <span class="label">Users with Devices</span>
        <p class="value">{$kpiWithDevices}</p>
        <div class="trend">Have at least 1 registered device</div>
    </div>
    <div class="card">
        <span class="label">Active Subscriptions</span>
        <p class="value">{$kpiWithActiveSub}</p>
        <div class="trend">Subscribed to a paid/active plan</div>
    </div>
</div>

<!-- Search & Filter Bar -->
<div class="filter-bar">
    <form method="GET" action="{$usersUrl}" style="display:flex; gap:10px; flex-wrap:wrap; align-items:center; width:100%;">
        <div class="filter-group">
            <input type="text" name="q" value="{$searchSafe}" placeholder="Search name, email, username..." class="filter-input">
        </div>
        <div class="filter-group">
            <select name="status" class="filter-select">
                {$statusOptions}
            </select>
        </div>
        <div class="filter-group">
            <select name="role" class="filter-select">
                {$roleOptions}
            </select>
        </div>
        <button type="submit" class="filter-btn">Apply Filters</button>
        <a href="{$usersUrl}" class="filter-reset">Reset</a>
    </form>
</div>

<!-- Users Table -->
<div class="section">
    <div class="section-header">
        <h3>User Accounts</h3>
        <span class="kpi-pill">{$totalUsersCount} Total Results</span>
    </div>
    <div style="overflow:auto;">
        <table>
            <thead>
                <tr>
                    <th>{$thId}</th>
                    <th>{$thName}</th>
                    <th>{$thEmail}</th>
                    <th>{$thRole}</th>
                    <th>{$thDevices}</th>
                    <th>Active Plan</th>
                    <th>{$thStatus}</th>
                    <th>{$thCreated}</th>
                    <th>Actions</th>
                </tr>
            </thead>
            <tbody>
                {$rowHtml}
            </tbody>
        </table>
    </div>
    {$paginationHtml}
</div>
HTML;

echo admin_render_page('Users', 'users', $body);
