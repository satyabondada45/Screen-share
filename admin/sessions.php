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

$sessionTable = admin_table_exists($pdo, 'sessions') ? 'sessions' : null;
if (!$sessionTable) {
    echo admin_render_page('Sessions', 'sessions', '<div class="flash error">The sessions database table is unavailable.</div>');
    exit;
}

// ─────────────────────────────────────────────────────────────────────────────
// FILTERS, SEARCH, SORTING & PAGINATION
// ─────────────────────────────────────────────────────────────────────────────

$search = trim((string) ($_GET['q'] ?? ''));
$statusFilter = strtolower(trim((string) ($_GET['status'] ?? 'all')));
$statusFilter = in_array($statusFilter, ['all', 'active', 'completed', 'rejected'], true) ? $statusFilter : 'all';

// Sorting Allowlist
$allowedSorts = [
    'id' => 's.id',
    'started_at' => 's.started_at',
    'ended_at' => 's.ended_at',
    'status' => 's.status',
    'bandwidth' => 's.bandwidth_used_mb',
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
    $where[] = '(s.session_key LIKE ? OR s.client_ip LIKE ? OR d.name LIKE ? OR d.system_id LIKE ? OR u.name LIKE ? OR u.email LIKE ?)';
    $params[] = '%' . $search . '%';
    $params[] = '%' . $search . '%';
    $params[] = '%' . $search . '%';
    $params[] = '%' . $search . '%';
    $params[] = '%' . $search . '%';
    $params[] = '%' . $search . '%';
}

if ($statusFilter !== 'all') {
    $where[] = 'LOWER(s.status) = ?';
    $params[] = $statusFilter;
}

$whereSql = $where ? ' WHERE ' . implode(' AND ', $where) : '';

// 1. KPI Metric Summary Counters
$kpiTotal = (int) $pdo->query('SELECT COUNT(*) FROM `sessions`')->fetchColumn();
$kpiActive = (int) $pdo->query('SELECT COUNT(*) FROM `sessions` WHERE LOWER(status) = "active"')->fetchColumn();
$kpiCompleted = (int) $pdo->query('SELECT COUNT(*) FROM `sessions` WHERE LOWER(status) = "completed"')->fetchColumn();
$kpiBandwidthMb = (float) $pdo->query('SELECT COALESCE(SUM(bandwidth_used_mb), 0) FROM `sessions`')->fetchColumn();
$kpiBandwidthFmt = admin_format_bytes($kpiBandwidthMb * 1024 * 1024);

// 2. Count Matching Records
$countStmt = $pdo->prepare("
    SELECT COUNT(*) 
    FROM `sessions` s
    LEFT JOIN `devices` d ON s.host_device_id = d.id
    LEFT JOIN `users` u ON d.user_id = u.id
    {$whereSql}
");
$countStmt->execute($params);
$totalMatchingSessions = (int) $countStmt->fetchColumn();

// 3. Fetch Paginated Records
$sql = "
    SELECT s.*, 
           d.name AS host_device_name,
           d.system_id AS host_system_id,
           u.name AS host_user_name,
           u.email AS host_user_email
    FROM `sessions` s
    LEFT JOIN `devices` d ON s.host_device_id = d.id
    LEFT JOIN `users` u ON d.user_id = u.id
    {$whereSql}
    ORDER BY {$sortColumn} {$dir}
    LIMIT {$perPage} OFFSET {$offset}
";

$stmt = $pdo->prepare($sql);
$stmt->execute($params);
$sessionRows = $stmt->fetchAll(PDO::FETCH_ASSOC);

// Build Table Rows
$rowsHtml = '';
if (!empty($sessionRows)) {
    foreach ($sessionRows as $row) {
        $id = (int) $row['id'];
        $sessionKey = htmlspecialchars((string) ($row['session_key'] ?? 'N/A'), ENT_QUOTES, 'UTF-8');
        $devName = htmlspecialchars((string) ($row['host_device_name'] ?? ('Device #' . $row['host_device_id'])), ENT_QUOTES, 'UTF-8');
        $devSysId = htmlspecialchars((string) ($row['host_system_id'] ?? ''), ENT_QUOTES, 'UTF-8');
        $devLink = !empty($row['host_device_id']) 
            ? '<a href="' . htmlspecialchars(admin_url('devices.php', ['view' => (int) $row['host_device_id']]), ENT_QUOTES, 'UTF-8') . '" style="color:var(--text-primary); font-weight:600;">' . $devName . '</a><br><small style="color:var(--text-muted); font-family:monospace;">' . $devSysId . '</small>'
            : $devName;

        $userName = htmlspecialchars((string) ($row['host_user_name'] ?? '—'), ENT_QUOTES, 'UTF-8');
        $userEmail = htmlspecialchars((string) ($row['host_user_email'] ?? ''), ENT_QUOTES, 'UTF-8');
        $userDisplay = ($userName !== '—') 
            ? '<strong>' . $userName . '</strong><br><small style="color:var(--text-muted);">' . $userEmail . '</small>' 
            : '<span style="color:var(--text-muted);">Unassigned</span>';

        $ip = htmlspecialchars((string) ($row['client_ip'] ?: '—'), ENT_QUOTES, 'UTF-8');
        $statusVal = strtoupper((string) ($row['status'] ?? 'COMPLETED'));
        $statusClass = strtolower($statusVal);
        $duration = admin_format_duration($row['started_at'] ?? null, $row['ended_at'] ?? null);
        $bandwidth = admin_format_bytes((float) ($row['bandwidth_used_mb'] ?? 0) * 1024 * 1024);
        $started = !empty($row['started_at']) ? htmlspecialchars((string) $row['started_at'], ENT_QUOTES, 'UTF-8') : 'N/A';
        $startedAgo = admin_time_ago($row['started_at'] ?? null);

        $rowsHtml .= "<tr>
            <td>#{$id}</td>
            <td><code>{$sessionKey}</code></td>
            <td>{$devLink}</td>
            <td>{$userDisplay}</td>
            <td><small><code>{$ip}</code></small></td>
            <td>{$duration}</td>
            <td>{$bandwidth}</td>
            <td><span class=\"status-badge status-{$statusClass}\">{$statusVal}</span></td>
            <td><small title=\"{$started}\">{$startedAgo}</small></td>
        </tr>";
    }
} else {
    $rowsHtml = '<tr><td colspan="9" class="empty-state">No remote sessions match the current query.</td></tr>';
}

$sessionsUrl = htmlspecialchars(admin_url('sessions.php'), ENT_QUOTES, 'UTF-8');
$queryParams = array_filter([
    'q' => $search !== '' ? $search : null,
    'status' => $statusFilter !== 'all' ? $statusFilter : null,
    'sort' => $sort !== 'id' ? $sort : null,
    'dir' => $dir !== 'DESC' ? strtolower($dir) : null,
]);

$paginationHtml = admin_pagination($totalMatchingSessions, $perPage, $page, 'sessions.php', $queryParams);

$statusOptions = '';
foreach (['all' => 'All Statuses', 'active' => 'Active', 'completed' => 'Completed', 'rejected' => 'Rejected'] as $v => $l) {
    $statusOptions .= '<option value="' . $v . '"' . ($statusFilter === $v ? ' selected' : '') . '>' . $l . '</option>';
}

$searchSafe = htmlspecialchars($search, ENT_QUOTES, 'UTF-8');

$thId = admin_sort_header('id', 'ID', $sort, $dir, 'sessions.php', $queryParams);
$thStarted = admin_sort_header('started_at', 'Started', $sort, $dir, 'sessions.php', $queryParams);
$thStatus = admin_sort_header('status', 'Status', $sort, $dir, 'sessions.php', $queryParams);
$thBandwidth = admin_sort_header('bandwidth', 'Bandwidth', $sort, $dir, 'sessions.php', $queryParams);

$body = <<<HTML
<!-- Session Summary KPI Cards -->
<div class="card-grid">
    <div class="card">
        <span class="label">Total Recorded Sessions</span>
        <p class="value">{$kpiTotal}</p>
        <div class="trend">All-time remote desktop sessions</div>
    </div>
    <div class="card">
        <span class="label">Currently Active</span>
        <p class="value" style="color:#e53935;">{$kpiActive}</p>
        <div class="trend">Live streaming connections right now</div>
    </div>
    <div class="card">
        <span class="label">Completed Sessions</span>
        <p class="value" style="color:#16a34a;">{$kpiCompleted}</p>
        <div class="trend">Finished remote sessions</div>
    </div>
    <div class="card">
        <span class="label">Total Bandwidth</span>
        <p class="value">{$kpiBandwidthFmt}</p>
        <div class="trend">Transferred session traffic</div>
    </div>
</div>

<!-- Search & Filter Bar -->
<div class="filter-bar">
    <form method="GET" action="{$sessionsUrl}" style="display:flex; gap:10px; flex-wrap:wrap; align-items:center; width:100%;">
        <div class="filter-group">
            <input type="text" name="q" value="{$searchSafe}" placeholder="Search session key, device, IP, host user..." class="filter-input" style="min-width:280px;">
        </div>
        <div class="filter-group">
            <select name="status" class="filter-select">
                {$statusOptions}
            </select>
        </div>
        <button type="submit" class="filter-btn">Filter</button>
        <a href="{$sessionsUrl}" class="filter-reset">Reset</a>
    </form>
</div>

<!-- Sessions Table -->
<div class="section">
    <div class="section-header">
        <h3>Remote Sessions</h3>
        <span class="kpi-pill">{$totalMatchingSessions} Sessions Found</span>
    </div>
    <div style="overflow:auto;">
        <table>
            <thead>
                <tr>
                    <th>{$thId}</th>
                    <th>Session Key</th>
                    <th>Host Device</th>
                    <th>Host User</th>
                    <th>Client IP</th>
                    <th>Duration</th>
                    <th>{$thBandwidth}</th>
                    <th>{$thStatus}</th>
                    <th>{$thStarted}</th>
                </tr>
            </thead>
            <tbody>
                {$rowsHtml}
            </tbody>
        </table>
    </div>
    {$paginationHtml}
</div>
HTML;

echo admin_render_page('Sessions', 'sessions', $body);
