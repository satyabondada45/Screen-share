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

$auditTable = admin_table_exists($pdo, 'audit_logs') ? 'audit_logs' : null;
if (!$auditTable) {
    echo admin_render_page('Audit Logs', 'audit-logs', '<div class="flash error">The audit logs database table is unavailable.</div>');
    exit;
}

// ─────────────────────────────────────────────────────────────────────────────
// SEARCH, SORTING & PAGINATION
// ─────────────────────────────────────────────────────────────────────────────

$search = trim((string) ($_GET['q'] ?? ''));
$actionFilter = trim((string) ($_GET['action'] ?? 'all'));

// Sorting Allowlist
$allowedSorts = [
    'id' => 'a.id',
    'created_at' => 'a.created_at',
    'action' => 'a.action',
];
$sort = (string) ($_GET['sort'] ?? 'id');
$sortColumn = $allowedSorts[$sort] ?? 'a.id';
$dir = strtolower((string) ($_GET['dir'] ?? 'desc')) === 'asc' ? 'ASC' : 'DESC';

$page = max(1, (int) ($_GET['page'] ?? 1));
$perPage = 25;
$offset = ($page - 1) * $perPage;

$where = [];
$params = [];

if ($search !== '') {
    $where[] = '(a.action LIKE ? OR a.details LIKE ? OR a.ip_address LIKE ? OR u.name LIKE ? OR u.email LIKE ? OR u.username LIKE ?)';
    $params[] = '%' . $search . '%';
    $params[] = '%' . $search . '%';
    $params[] = '%' . $search . '%';
    $params[] = '%' . $search . '%';
    $params[] = '%' . $search . '%';
    $params[] = '%' . $search . '%';
}

if ($actionFilter !== 'all' && $actionFilter !== '') {
    $where[] = 'a.action = ?';
    $params[] = $actionFilter;
}

$whereSql = $where ? ' WHERE ' . implode(' AND ', $where) : '';

// 1. KPI Metric Summary Counters
$kpiTotal = (int) $pdo->query('SELECT COUNT(*) FROM `audit_logs`')->fetchColumn();
$kpiToday = (int) $pdo->query('SELECT COUNT(*) FROM `audit_logs` WHERE created_at >= CURDATE()')->fetchColumn();
$kpiUniqueActions = (int) $pdo->query('SELECT COUNT(DISTINCT action) FROM `audit_logs`')->fetchColumn();
$kpiAdmins = (int) $pdo->query('SELECT COUNT(DISTINCT user_id) FROM `audit_logs` WHERE user_id IS NOT NULL')->fetchColumn();

// 2. Count Matching Records
$countStmt = $pdo->prepare("
    SELECT COUNT(*) 
    FROM `audit_logs` a
    LEFT JOIN `users` u ON a.user_id = u.id
    {$whereSql}
");
$countStmt->execute($params);
$totalMatching = (int) $countStmt->fetchColumn();

// 3. Fetch Paginated Records
$sql = "
    SELECT a.*, 
           u.id AS user_pk,
           u.name AS user_name, 
           u.email AS user_email
    FROM `audit_logs` a
    LEFT JOIN `users` u ON a.user_id = u.id
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
        $logId = (int) $row['id'];
        $act = htmlspecialchars((string) ($row['action'] ?? 'system_event'), ENT_QUOTES, 'UTF-8');
        $details = htmlspecialchars((string) ($row['details'] ?? '—'), ENT_QUOTES, 'UTF-8');
        $ip = htmlspecialchars((string) ($row['ip_address'] ?? '127.0.0.1'), ENT_QUOTES, 'UTF-8');
        
        $userId = (int) ($row['user_pk'] ?? $row['user_id']);
        $userName = htmlspecialchars((string) ($row['user_name'] ?? 'Admin #' . $userId), ENT_QUOTES, 'UTF-8');
        $userEmail = htmlspecialchars((string) ($row['user_email'] ?? ''), ENT_QUOTES, 'UTF-8');
        $userDisplay = ($userId > 0)
            ? '<a href="' . htmlspecialchars(admin_url('users.php', ['view' => $userId]), ENT_QUOTES, 'UTF-8') . '" style="color:var(--text-primary); font-weight:600;">' . $userName . '</a>'
            : '<span style="color:var(--text-muted);">System</span>';

        $created = !empty($row['created_at']) ? htmlspecialchars((string) $row['created_at'], ENT_QUOTES, 'UTF-8') : 'N/A';
        $timeAgo = admin_time_ago($row['created_at'] ?? null);

        $rowHtml .= "<tr>
            <td>#{$logId}</td>
            <td><small title=\"{$created}\">{$timeAgo}</small></td>
            <td>{$userDisplay}</td>
            <td><span class=\"kpi-pill\" style=\"font-family:monospace;\">{$act}</span></td>
            <td>{$details}</td>
            <td><small><code>{$ip}</code></small></td>
        </tr>";
    }
} else {
    $rowHtml = '<tr><td colspan="6" class="empty-state">No audit log records found for this query.</td></tr>';
}

$auditUrl = htmlspecialchars(admin_url('audit-logs.php'), ENT_QUOTES, 'UTF-8');
$queryParams = array_filter([
    'q' => $search !== '' ? $search : null,
    'action' => $actionFilter !== 'all' ? $actionFilter : null,
    'sort' => $sort !== 'id' ? $sort : null,
    'dir' => $dir !== 'DESC' ? strtolower($dir) : null,
]);

$paginationHtml = admin_pagination($totalMatching, $perPage, $page, 'audit-logs.php', $queryParams);

// Action options
$actionOptions = '<option value="all">All Actions</option>';
try {
    $distinctActions = $pdo->query('SELECT DISTINCT action FROM `audit_logs` ORDER BY action ASC')->fetchAll(PDO::FETCH_COLUMN);
    foreach ($distinctActions as $a) {
        $aSafe = htmlspecialchars((string) $a, ENT_QUOTES, 'UTF-8');
        $actionOptions .= '<option value="' . $aSafe . '"' . ($actionFilter === $a ? ' selected' : '') . '>' . $aSafe . '</option>';
    }
} catch (Throwable $e) {}

$searchValue = htmlspecialchars($search, ENT_QUOTES, 'UTF-8');

$thId = admin_sort_header('id', 'ID', $sort, $dir, 'audit-logs.php', $queryParams);
$thCreated = admin_sort_header('created_at', 'Timestamp', $sort, $dir, 'audit-logs.php', $queryParams);
$thAction = admin_sort_header('action', 'Action', $sort, $dir, 'audit-logs.php', $queryParams);

$body = <<<HTML
<!-- Audit KPI Summary Cards -->
<div class="card-grid">
    <div class="card">
        <span class="label">Total Recorded Events</span>
        <p class="value">{$kpiTotal}</p>
        <div class="trend">System & administrative audits</div>
    </div>
    <div class="card">
        <span class="label">Logged Today</span>
        <p class="value" style="color:#16a34a;">{$kpiToday}</p>
        <div class="trend">Recorded in past 24 hours</div>
    </div>
    <div class="card">
        <span class="label">Unique Action Types</span>
        <p class="value">{$kpiUniqueActions}</p>
        <div class="trend">Distinct operation categories</div>
    </div>
    <div class="card">
        <span class="label">Audited Actors</span>
        <p class="value">{$kpiAdmins}</p>
        <div class="trend">Administrators / actors</div>
    </div>
</div>

<!-- Search & Interactive Filter Bar -->
<div class="filter-bar">
    <form method="GET" action="{$auditUrl}" style="display:flex; gap:10px; flex-wrap:wrap; align-items:center; width:100%;">
        <div class="filter-group">
            <input type="text" name="q" value="{$searchValue}" placeholder="Search details, actor, IP..." class="filter-input" style="min-width:280px;">
        </div>
        <div class="filter-group">
            <select name="action" class="filter-select">
                {$actionOptions}
            </select>
        </div>
        <button type="submit" class="filter-btn">Filter</button>
        <a href="{$auditUrl}" class="filter-reset">Reset</a>
    </form>
</div>

<!-- Audit Logs Table -->
<div class="section">
    <div class="section-header">
        <h3>System Audit Trail</h3>
        <span class="kpi-pill">{$totalMatching} Audit Entries</span>
    </div>
    <div style="overflow:auto;">
        <table>
            <thead>
                <tr>
                    <th>{$thId}</th>
                    <th>{$thCreated}</th>
                    <th>User / Actor</th>
                    <th>{$thAction}</th>
                    <th>Details</th>
                    <th>IP Address</th>
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

echo admin_render_page('Audit Logs', 'audit-logs', $body);
