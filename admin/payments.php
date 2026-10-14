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

$paymentsTable = admin_table_exists($pdo, 'payments') ? 'payments' : null;
if (!$paymentsTable) {
    echo admin_render_page('Payments', 'payments', '<div class="flash error">The payments database table is unavailable.</div>');
    exit;
}

// ─────────────────────────────────────────────────────────────────────────────
// SEARCH, FILTER, SORTING & PAGINATION
// ─────────────────────────────────────────────────────────────────────────────

$search = trim((string) ($_GET['q'] ?? ''));
$status = strtolower(trim((string) ($_GET['status'] ?? 'all')));
$validStatuses = ['all', 'completed', 'pending', 'failed', 'refunded'];
$status = in_array($status, $validStatuses, true) ? $status : 'all';

// Sorting Allowlist
$allowedSorts = [
    'id' => 'pay.id',
    'created_at' => 'pay.created_at',
    'amount' => 'pay.amount',
    'status' => 'pay.status',
];
$sort = (string) ($_GET['sort'] ?? 'id');
$sortColumn = $allowedSorts[$sort] ?? 'pay.id';
$dir = strtolower((string) ($_GET['dir'] ?? 'desc')) === 'asc' ? 'ASC' : 'DESC';

$page = max(1, (int) ($_GET['page'] ?? 1));
$perPage = 20;
$offset = ($page - 1) * $perPage;

$where = [];
$params = [];

if ($search !== '') {
    $where[] = '(pay.transaction_id LIKE ? OR pay.payment_method LIKE ? OR u.name LIKE ? OR u.email LIKE ? OR u.username LIKE ? OR p.name LIKE ?)';
    $params[] = '%' . $search . '%';
    $params[] = '%' . $search . '%';
    $params[] = '%' . $search . '%';
    $params[] = '%' . $search . '%';
    $params[] = '%' . $search . '%';
    $params[] = '%' . $search . '%';
}

if ($status !== 'all') {
    $where[] = 'LOWER(pay.status) = ?';
    $params[] = $status;
}

$whereSql = $where ? ' WHERE ' . implode(' AND ', $where) : '';

// 1. KPI Metric Summary Counters
$kpiTotalRevenue = (float) $pdo->query('SELECT COALESCE(SUM(amount), 0) FROM `payments` WHERE LOWER(status) IN ("completed", "paid", "successful")')->fetchColumn();
$kpiCompletedCount = (int) $pdo->query('SELECT COUNT(*) FROM `payments` WHERE LOWER(status) IN ("completed", "paid", "successful")')->fetchColumn();
$kpiPendingCount = (int) $pdo->query('SELECT COUNT(*) FROM `payments` WHERE LOWER(status) = "pending"')->fetchColumn();
$kpiFailedCount = (int) $pdo->query('SELECT COUNT(*) FROM `payments` WHERE LOWER(status) IN ("failed", "refunded")')->fetchColumn();

// 2. Count Matching Records
$countStmt = $pdo->prepare("
    SELECT COUNT(*) 
    FROM `payments` pay
    LEFT JOIN `users` u ON pay.user_id = u.id
    LEFT JOIN `subscriptions` sub ON pay.subscription_id = sub.id
    LEFT JOIN `plans` p ON sub.plan_id = p.id
    {$whereSql}
");
$countStmt->execute($params);
$totalMatching = (int) $countStmt->fetchColumn();

// 3. Fetch Paginated Records
$sql = "
    SELECT pay.*, 
           u.id AS user_pk,
           u.name AS user_name, 
           u.email AS user_email,
           p.name AS plan_name
    FROM `payments` pay
    LEFT JOIN `users` u ON pay.user_id = u.id
    LEFT JOIN `subscriptions` sub ON pay.subscription_id = sub.id
    LEFT JOIN `plans` p ON sub.plan_id = p.id
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
        $payId = (int) $row['id'];
        $txnId = htmlspecialchars((string) ($row['transaction_id'] ?? '—'), ENT_QUOTES, 'UTF-8');
        
        $userId = (int) ($row['user_pk'] ?? $row['user_id']);
        $userName = htmlspecialchars((string) ($row['user_name'] ?? 'User #' . $userId), ENT_QUOTES, 'UTF-8');
        $userEmail = htmlspecialchars((string) ($row['user_email'] ?? ''), ENT_QUOTES, 'UTF-8');
        $userDisplay = ($userId > 0)
            ? '<a href="' . htmlspecialchars(admin_url('users.php', ['view' => $userId]), ENT_QUOTES, 'UTF-8') . '" style="color:var(--text-primary); font-weight:600;">' . $userName . '</a><br><small style="color:var(--text-muted);">' . $userEmail . '</small>'
            : '<span style="color:var(--text-muted);">Guest / Direct</span>';

        $planDisplay = !empty($row['plan_name']) 
            ? htmlspecialchars((string) $row['plan_name'], ENT_QUOTES, 'UTF-8')
            : (!empty($row['subscription_id']) ? ('Sub #' . (int) $row['subscription_id']) : 'Direct Payment');

        $amount = (float) ($row['amount'] ?? 0);
        $method = htmlspecialchars((string) ($row['payment_method'] ?? 'Online'), ENT_QUOTES, 'UTF-8');
        $statusVal = strtoupper((string) ($row['status'] ?? 'COMPLETED'));
        $statusClass = strtolower($statusVal);
        $date = !empty($row['created_at']) ? admin_utc_date($row['created_at']) : 'N/A';
        $timeAgo = admin_time_ago($row['created_at'] ?? null);

        $rowHtml .= "<tr>
            <td>#{$payId}</td>
            <td><code>{$txnId}</code></td>
            <td>{$userDisplay}</td>
            <td><strong>{$planDisplay}</strong></td>
            <td><strong style=\"color:#16a34a;\">₹" . number_format($amount, 2) . "</strong></td>
            <td><span class=\"kpi-pill\">{$method}</span></td>
            <td><span class=\"status-badge status-{$statusClass}\">{$statusVal}</span></td>
            <td><span title=\"{$date}\">{$timeAgo}</span></td>
        </tr>";
    }
} else {
    $rowHtml = '<tr><td colspan="8" class="empty-state">No payment transactions match the selected filters.</td></tr>';
}

$paymentsUrl = htmlspecialchars(admin_url('payments.php'), ENT_QUOTES, 'UTF-8');
$queryParams = array_filter([
    'q' => $search !== '' ? $search : null,
    'status' => $status !== 'all' ? $status : null,
    'sort' => $sort !== 'id' ? $sort : null,
    'dir' => $dir !== 'DESC' ? strtolower($dir) : null,
]);

$paginationHtml = admin_pagination($totalMatching, $perPage, $page, 'payments.php', $queryParams);

$statusOptions = '';
foreach (['all' => 'All Statuses', 'completed' => 'Completed', 'pending' => 'Pending', 'failed' => 'Failed', 'refunded' => 'Refunded'] as $value => $label) {
    $statusOptions .= '<option value="' . $value . '"' . ($status === $value ? ' selected' : '') . '>' . $label . '</option>';
}

$searchValue = htmlspecialchars($search, ENT_QUOTES, 'UTF-8');

$thId = admin_sort_header('id', 'ID', $sort, $dir, 'payments.php', $queryParams);
$thAmount = admin_sort_header('amount', 'Amount', $sort, $dir, 'payments.php', $queryParams);
$thStatus = admin_sort_header('status', 'Status', $sort, $dir, 'payments.php', $queryParams);
$thCreated = admin_sort_header('created_at', 'Date', $sort, $dir, 'payments.php', $queryParams);

$formattedRevenue = '₹' . number_format($kpiTotalRevenue, 2);

$body = <<<HTML
<!-- Payments KPI Summary Cards -->
<div class="card-grid">
    <div class="card">
        <span class="label">Total Collected Revenue</span>
        <p class="value">{$formattedRevenue}</p>
        <div class="trend">Successful transaction revenue</div>
    </div>
    <div class="card">
        <span class="label">Completed Payments</span>
        <p class="value" style="color:#16a34a;">{$kpiCompletedCount}</p>
        <div class="trend">Settled transactions</div>
    </div>
    <div class="card">
        <span class="label">Pending Payments</span>
        <p class="value" style="color:#b45309;">{$kpiPendingCount}</p>
        <div class="trend">Awaiting gateway confirmation</div>
    </div>
    <div class="card">
        <span class="label">Failed / Refunded</span>
        <p class="value" style="color:#6b7280;">{$kpiFailedCount}</p>
        <div class="trend">Unsuccessful checkouts</div>
    </div>
</div>

<!-- Search & Interactive Filter Bar -->
<div class="filter-bar">
    <form method="GET" action="{$paymentsUrl}" style="display:flex; gap:10px; flex-wrap:wrap; align-items:center; width:100%;">
        <div class="filter-group">
            <input type="text" name="q" value="{$searchValue}" placeholder="Search transaction ID, customer, payment method..." class="filter-input" style="min-width:280px;">
        </div>
        <div class="filter-group">
            <select name="status" class="filter-select">
                {$statusOptions}
            </select>
        </div>
        <button type="submit" class="filter-btn">Filter</button>
        <a href="{$paymentsUrl}" class="filter-reset">Reset</a>
    </form>
</div>

<!-- Payments Table -->
<div class="section">
    <div class="section-header">
        <h3>Transactions & Payments</h3>
        <span class="kpi-pill">{$totalMatching} Transactions Found</span>
    </div>
    <div style="overflow:auto;">
        <table>
            <thead>
                <tr>
                    <th>{$thId}</th>
                    <th>Transaction ID</th>
                    <th>Customer</th>
                    <th>Plan / Reference</th>
                    <th>{$thAmount}</th>
                    <th>Payment Method</th>
                    <th>{$thStatus}</th>
                    <th>{$thCreated}</th>
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

echo admin_render_page('Payments', 'payments', $body);
