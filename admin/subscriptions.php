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

$subscriptionTable = admin_table_exists($pdo, 'subscriptions') ? 'subscriptions' : null;
if (!$subscriptionTable) {
    echo admin_render_page('Subscriptions', 'subscriptions', '<div class="flash error">The subscriptions database table is unavailable.</div>');
    exit;
}

// ─────────────────────────────────────────────────────────────────────────────
// SEARCH, FILTER, SORTING & PAGINATION
// ─────────────────────────────────────────────────────────────────────────────

$search = trim((string) ($_GET['q'] ?? ''));
$status = strtolower(trim((string) ($_GET['status'] ?? 'all')));
$status = in_array($status, ['all', 'active', 'expired', 'cancelled', 'pending'], true) ? $status : 'all';

// Sorting Allowlist
$allowedSorts = [
    'id' => 's.id',
    'starts_at' => 's.starts_at',
    'expires_at' => 's.expires_at',
    'status' => 's.status',
    'price' => 'p.price',
];
$sort = (string) ($_GET['sort'] ?? 'id');
$sortColumn = $allowedSorts[$sort] ?? 's.id';
$dir = strtolower((string) ($_GET['dir'] ?? 'desc')) === 'asc' ? 'ASC' : 'DESC';

$page = max(1, (int) ($_GET['page'] ?? 1));
$perPage = 20;
$offset = ($page - 1) * $perPage;

$where = [];
$params = [];

if ($search !== '') {
    $where[] = '(u.name LIKE ? OR u.email LIKE ? OR u.username LIKE ? OR u.unique_user_id LIKE ? OR p.name LIKE ?)';
    $params[] = '%' . $search . '%';
    $params[] = '%' . $search . '%';
    $params[] = '%' . $search . '%';
    $params[] = '%' . $search . '%';
    $params[] = '%' . $search . '%';
}

if ($status !== 'all') {
    $where[] = 'LOWER(s.status) = ?';
    $params[] = $status;
}

$whereSql = $where ? ' WHERE ' . implode(' AND ', $where) : '';

// 1. KPI Metric Summary Counters
$kpiTotal = (int) $pdo->query('SELECT COUNT(*) FROM `subscriptions`')->fetchColumn();
$kpiActive = (int) $pdo->query('SELECT COUNT(*) FROM `subscriptions` WHERE LOWER(status) = "active"')->fetchColumn();
$kpiExpired = (int) $pdo->query('SELECT COUNT(*) FROM `subscriptions` WHERE expires_at IS NOT NULL AND expires_at < NOW()')->fetchColumn();
$kpiExpiringSoon = (int) $pdo->query('SELECT COUNT(*) FROM `subscriptions` WHERE expires_at IS NOT NULL AND expires_at BETWEEN NOW() AND DATE_ADD(NOW(), INTERVAL 7 DAY)')->fetchColumn();

// 2. Count Matching Records
$countStmt = $pdo->prepare("
    SELECT COUNT(*) 
    FROM `subscriptions` s
    LEFT JOIN `users` u ON s.user_id = u.id
    LEFT JOIN `plans` p ON s.plan_id = p.id
    {$whereSql}
");
$countStmt->execute($params);
$totalMatching = (int) $countStmt->fetchColumn();

// 3. Fetch Paginated Records
$sql = "
    SELECT s.*, 
           u.id AS user_pk,
           u.name AS user_name, 
           u.email AS user_email,
           u.username AS user_username,
           p.name AS plan_name, 
           p.price AS plan_price,
           p.duration_unit
    FROM `subscriptions` s
    LEFT JOIN `users` u ON s.user_id = u.id
    LEFT JOIN `plans` p ON s.plan_id = p.id
    {$whereSql}
    ORDER BY {$sortColumn} {$dir}
    LIMIT {$perPage} OFFSET {$offset}
";

$stmt = $pdo->prepare($sql);
$stmt->execute($params);
$rows = $stmt->fetchAll(PDO::FETCH_ASSOC);

// Build Table Rows
$rowHtml = '';
if (!empty($rows)) {
    foreach ($rows as $row) {
        $subId = (int) $row['id'];
        $userId = (int) ($row['user_pk'] ?? $row['user_id']);
        $userName = htmlspecialchars((string) ($row['user_name'] ?? ($row['user_username'] ?? 'User #' . $userId)), ENT_QUOTES, 'UTF-8');
        $userEmail = htmlspecialchars((string) ($row['user_email'] ?? ''), ENT_QUOTES, 'UTF-8');
        $userLink = '<a href="' . htmlspecialchars(admin_url('users.php', ['view' => $userId]), ENT_QUOTES, 'UTF-8') . '" style="color:var(--text-primary); font-weight:600;">' . $userName . '</a><br><small style="color:var(--text-muted);">' . $userEmail . '</small>';

        $planDisplay = htmlspecialchars((string) ($row['plan_name'] ?? 'Plan #' . $row['plan_id']), ENT_QUOTES, 'UTF-8');
        $startsAt = !empty($row['starts_at']) ? admin_utc_date($row['starts_at']) : 'N/A';
        
        $expVal = $row['expires_at'] ?? null;
        if (empty($expVal) || ($row['duration_unit'] ?? '') === 'unlimited') {
            $expiresAt = '<span style="color:#16a34a; font-weight:600;">Unlimited</span>';
        } else {
            $expTs = strtotime($expVal);
            $expDateStr = admin_utc_date($expVal);
            if ($expTs < time()) {
                $expiresAt = "<span style=\"color:#dc2626; font-weight:600;\">{$expDateStr}</span><br><small style=\"color:#dc2626;\">Expired</small>";
            } else {
                $daysLeft = ceil(($expTs - time()) / 86400);
                $expiresAt = "<strong>{$expDateStr}</strong><br><small style=\"color:var(--text-muted);\">{$daysLeft} days remaining</small>";
            }
        }

        $statusVal = strtoupper((string) ($row['status'] ?? 'ACTIVE'));
        $statusClass = strtolower($statusVal);
        $amount = (float) ($row['plan_price'] ?? 0);

        $rowHtml .= "<tr>
            <td>#{$subId}</td>
            <td>{$userLink}</td>
            <td><strong>{$planDisplay}</strong></td>
            <td>{$startsAt}</td>
            <td>{$expiresAt}</td>
            <td>₹" . number_format($amount, 2) . "</td>
            <td><span class=\"status-badge status-{$statusClass}\">{$statusVal}</span></td>
        </tr>";
    }
} else {
    $rowHtml = '<tr><td colspan="7" class="empty-state">No matching subscriptions found.</td></tr>';
}

$subscriptionsUrl = htmlspecialchars(admin_url('subscriptions.php'), ENT_QUOTES, 'UTF-8');
$queryParams = array_filter([
    'q' => $search !== '' ? $search : null,
    'status' => $status !== 'all' ? $status : null,
    'sort' => $sort !== 'id' ? $sort : null,
    'dir' => $dir !== 'DESC' ? strtolower($dir) : null,
]);

$paginationHtml = admin_pagination($totalMatching, $perPage, $page, 'subscriptions.php', $queryParams);

$statusOptions = '';
foreach (['all' => 'All Statuses', 'active' => 'Active', 'expired' => 'Expired', 'pending' => 'Pending', 'cancelled' => 'Cancelled'] as $value => $label) {
    $statusOptions .= '<option value="' . $value . '"' . ($status === $value ? ' selected' : '') . '>' . $label . '</option>';
}

$searchValue = htmlspecialchars($search, ENT_QUOTES, 'UTF-8');

$thId = admin_sort_header('id', 'ID', $sort, $dir, 'subscriptions.php', $queryParams);
$thStarted = admin_sort_header('starts_at', 'Started At', $sort, $dir, 'subscriptions.php', $queryParams);
$thExpires = admin_sort_header('expires_at', 'Expires At', $sort, $dir, 'subscriptions.php', $queryParams);
$thPrice = admin_sort_header('price', 'Plan Price', $sort, $dir, 'subscriptions.php', $queryParams);
$thStatus = admin_sort_header('status', 'Status', $sort, $dir, 'subscriptions.php', $queryParams);

$body = <<<HTML
<!-- Subscriptions KPI Cards -->
<div class="card-grid">
    <div class="card">
        <span class="label">Total Subscriptions</span>
        <p class="value">{$kpiTotal}</p>
        <div class="trend">All assigned subscription records</div>
    </div>
    <div class="card">
        <span class="label">Active Subscriptions</span>
        <p class="value" style="color:#16a34a;">{$kpiActive}</p>
        <div class="trend">Currently valid & running</div>
    </div>
    <div class="card">
        <span class="label">Past Due / Expired</span>
        <p class="value" style="color:#6b7280;">{$kpiExpired}</p>
        <div class="trend">Lapsed subscription plans</div>
    </div>
    <div class="card">
        <span class="label">Expiring Soon</span>
        <p class="value" style="color:#b45309;">{$kpiExpiringSoon}</p>
        <div class="trend">Next 7 days renewal window</div>
    </div>
</div>

<!-- Search & Filter Bar -->
<div class="filter-bar">
    <form method="GET" action="{$subscriptionsUrl}" style="display:flex; gap:10px; flex-wrap:wrap; align-items:center; width:100%;">
        <div class="filter-group">
            <input type="text" name="q" value="{$searchValue}" placeholder="Search customer, email, plan..." class="filter-input" style="min-width:260px;">
        </div>
        <div class="filter-group">
            <select name="status" class="filter-select">
                {$statusOptions}
            </select>
        </div>
        <button type="submit" class="filter-btn">Filter</button>
        <a href="{$subscriptionsUrl}" class="filter-reset">Reset</a>
    </form>
</div>

<!-- Subscriptions Table -->
<div class="section">
    <div class="section-header">
        <h3>Customer Subscriptions</h3>
        <span class="kpi-pill">{$totalMatching} Subscriptions Found</span>
    </div>
    <div style="overflow:auto;">
        <table>
            <thead>
                <tr>
                    <th>{$thId}</th>
                    <th>Customer Account</th>
                    <th>Assigned Plan</th>
                    <th>{$thStarted}</th>
                    <th>{$thExpires}</th>
                    <th>{$thPrice}</th>
                    <th>{$thStatus}</th>
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

echo admin_render_page('Subscriptions', 'subscriptions', $body);
