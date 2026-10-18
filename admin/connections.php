<?php
require_once __DIR__ . '/includes/db.php';
require_once __DIR__ . '/includes/auth.php';
require_once __DIR__ . '/includes/layout.php';

admin_require_auth();

try {
    $pdo = admin_db();

    $connReqTable = admin_table_exists($pdo, 'connection_requests') ? 'connection_requests' : (admin_table_exists($pdo, 'connections') ? 'connections' : null);
    if (!$connReqTable) {
        echo admin_render_page('Connections', 'connections', '<div class="flash error">The connections database table is unavailable.</div>');
        exit;
    }

    $hasReqUserId = admin_column_exists($pdo, 'connection_requests', 'requester_user_id');
    $hasTargetUserId = admin_column_exists($pdo, 'connection_requests', 'target_user_id');
    $hasReqSysId = admin_column_exists($pdo, 'connection_requests', 'requester_system_id');
    $hasReqName = admin_column_exists($pdo, 'connection_requests', 'requester_name');
    $hasTargetSysId = admin_column_exists($pdo, 'connection_requests', 'target_system_id');
    $hasReqToken = admin_column_exists($pdo, 'connection_requests', 'request_token');
    $hasExpiresAt = admin_column_exists($pdo, 'connection_requests', 'expires_at');
    $hasStatus = admin_column_exists($pdo, 'connection_requests', 'status');

    // ─────────────────────────────────────────────────────────────────────────────
    // SEARCH, FILTER, SORT & PAGINATION
    // ─────────────────────────────────────────────────────────────────────────────

    $search = trim((string) ($_GET['q'] ?? ''));
    $statusFilter = strtolower(trim((string) ($_GET['status'] ?? 'all')));
    $validStatuses = ['all', 'accepted', 'pending', 'rejected', 'expired', 'cancelled', 'connected', 'disconnected'];
    $statusFilter = in_array($statusFilter, $validStatuses, true) ? $statusFilter : 'all';

    // Sorting Allowlist
    $allowedSorts = [
        'id' => 'cr.id',
        'created_at' => 'cr.created_at',
        'updated_at' => 'cr.updated_at',
        'status' => 'cr.status',
    ];
    if ($hasExpiresAt) {
        $allowedSorts['expires_at'] = 'cr.expires_at';
    }
    $sort = (string) ($_GET['sort'] ?? 'id');
    $sortColumn = $allowedSorts[$sort] ?? 'cr.id';
    $dir = strtolower((string) ($_GET['dir'] ?? 'desc')) === 'asc' ? 'ASC' : 'DESC';

    $page = max(1, (int) ($_GET['page'] ?? 1));
    $perPage = 20;
    $offset = ($page - 1) * $perPage;

    $where = [];
    $params = [];

    if ($search !== '') {
        $searchFields = [];
        if ($hasReqToken) $searchFields[] = 'cr.request_token LIKE ?';
        if ($hasReqSysId) $searchFields[] = 'cr.requester_system_id LIKE ?';
        if ($hasReqName) $searchFields[] = 'cr.requester_name LIKE ?';
        if ($hasTargetSysId) $searchFields[] = 'cr.target_system_id LIKE ?';
        if ($hasReqUserId) {
            $searchFields[] = 'ru.name LIKE ?';
            $searchFields[] = 'ru.email LIKE ?';
        }
        if ($hasTargetUserId) {
            $searchFields[] = 'tu.name LIKE ?';
            $searchFields[] = 'tu.email LIKE ?';
        }

        if (!empty($searchFields)) {
            $where[] = '(' . implode(' OR ', $searchFields) . ')';
            $searchParam = '%' . $search . '%';
            for ($i = 0; $i < count($searchFields); $i++) {
                $params[] = $searchParam;
            }
        }
    }

    if ($statusFilter !== 'all' && $hasStatus) {
        $where[] = 'LOWER(cr.status) = ?';
        $params[] = $statusFilter;
    }

    $whereSql = $where ? ' WHERE ' . implode(' AND ', $where) : '';

    // 1. KPI Metric Summary Counters
    $kpiTotal = (int) $pdo->query('SELECT COUNT(*) FROM `connection_requests`')->fetchColumn();
    $kpiAccepted = $hasStatus ? (int) $pdo->query('SELECT COUNT(*) FROM `connection_requests` WHERE LOWER(status) = "accepted"')->fetchColumn() : 0;
    $kpiPending = $hasStatus ? (int) $pdo->query('SELECT COUNT(*) FROM `connection_requests` WHERE LOWER(status) = "pending"')->fetchColumn() : 0;
    $kpiExpired = $hasStatus ? (int) $pdo->query('SELECT COUNT(*) FROM `connection_requests` WHERE LOWER(status) IN ("expired", "rejected", "cancelled")')->fetchColumn() : 0;

    $joinClause = '';
    $userCols = '';
    if ($hasReqUserId) {
        $joinClause .= ' LEFT JOIN `users` ru ON cr.requester_user_id = ru.id';
        $userCols .= ', ru.name AS requester_user_name, ru.email AS requester_user_email';
    } else {
        $userCols .= ', NULL AS requester_user_name, NULL AS requester_user_email';
    }
    if ($hasTargetUserId) {
        $joinClause .= ' LEFT JOIN `users` tu ON cr.target_user_id = tu.id';
        $userCols .= ', tu.name AS target_user_name, tu.email AS target_user_email';
    } else {
        $userCols .= ', NULL AS target_user_name, NULL AS target_user_email';
    }

    // 2. Count Matching Records
    $countStmt = $pdo->prepare("
        SELECT COUNT(*) 
        FROM `connection_requests` cr
        {$joinClause}
        {$whereSql}
    ");
    $countStmt->execute($params);
    $totalMatching = (int) $countStmt->fetchColumn();

    // 3. Fetch Paginated Records
    $sql = "
        SELECT cr.*
               {$userCols}
        FROM `connection_requests` cr
        {$joinClause}
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
        foreach ($rows as $r) {
            $id = (int) $r['id'];
            $token = htmlspecialchars((string) ($r['request_token'] ?? '—'), ENT_QUOTES, 'UTF-8');
            
            $reqName = htmlspecialchars((string) ($r['requester_name'] ?? 'Requester'), ENT_QUOTES, 'UTF-8');
            $reqSysId = htmlspecialchars((string) ($r['requester_system_id'] ?? '—'), ENT_QUOTES, 'UTF-8');
            $reqUser = !empty($r['requester_user_name']) ? htmlspecialchars((string) $r['requester_user_name'], ENT_QUOTES, 'UTF-8') : '';
            $requesterDisplay = "<strong>{$reqName}</strong><br><small style=\"font-family:monospace; color:var(--text-muted);\">{$reqSysId}</small>";
            if ($reqUser !== '') {
                $requesterDisplay .= "<br><small style=\"color:var(--text-secondary);\">Owner: {$reqUser}</small>";
            }

            $targetSysId = htmlspecialchars((string) ($r['target_system_id'] ?? '—'), ENT_QUOTES, 'UTF-8');
            $targetUser = !empty($r['target_user_name']) ? htmlspecialchars((string) $r['target_user_name'], ENT_QUOTES, 'UTF-8') : '';
            $targetDisplay = "<strong style=\"font-family:monospace;\">{$targetSysId}</strong>";
            if ($targetUser !== '') {
                $targetDisplay .= "<br><small style=\"color:var(--text-secondary);\">Target User: {$targetUser}</small>";
            }

            $statusVal = strtoupper((string) ($r['status'] ?? 'PENDING'));
            $statusClass = strtolower($statusVal);
            if ($statusClass === 'accepted' || $statusClass === 'connected') {
                $badgeClass = 'status-active';
            } elseif ($statusClass === 'pending') {
                $badgeClass = 'status-warning';
            } else {
                $badgeClass = 'status-inactive';
            }

            $created = admin_time_ago($r['created_at'] ?? null);
            $expires = !empty($r['expires_at']) ? htmlspecialchars((string) $r['expires_at'], ENT_QUOTES, 'UTF-8') : '—';
            $updated = !empty($r['updated_at']) ? admin_time_ago($r['updated_at']) : '—';

            $rowHtml .= "<tr>
                <td>#{$id}</td>
                <td><code title=\"{$token}\">" . substr($token, 0, 14) . "...</code></td>
                <td>{$requesterDisplay}</td>
                <td>{$targetDisplay}</td>
                <td><span class=\"status-badge {$badgeClass}\">{$statusVal}</span></td>
                <td><small>{$created}</small></td>
                <td><small>{$updated}</small></td>
                <td><small>{$expires}</small></td>
            </tr>";
        }
    } else {
        $rowHtml = '<tr><td colspan="8" class="empty-state">No connection requests match the current filters.</td></tr>';
    }

    $connectionsUrl = htmlspecialchars(admin_url('connections.php'), ENT_QUOTES, 'UTF-8');
    $queryParams = array_filter([
        'q' => $search !== '' ? $search : null,
        'status' => $statusFilter !== 'all' ? $statusFilter : null,
        'sort' => $sort !== 'id' ? $sort : null,
        'dir' => $dir !== 'DESC' ? strtolower($dir) : null,
    ]);

    $paginationHtml = admin_pagination($totalMatching, $perPage, $page, 'connections.php', $queryParams);

    $statusOptions = '';
    foreach (['all' => 'All Statuses', 'accepted' => 'Accepted', 'pending' => 'Pending', 'connected' => 'Connected', 'expired' => 'Expired', 'rejected' => 'Rejected', 'cancelled' => 'Cancelled'] as $v => $l) {
        $statusOptions .= '<option value="' . $v . '"' . ($statusFilter === $v ? ' selected' : '') . '>' . $l . '</option>';
    }

    $searchSafe = htmlspecialchars($search, ENT_QUOTES, 'UTF-8');

    $thId = admin_sort_header('id', 'ID', $sort, $dir, 'connections.php', $queryParams);
    $thCreated = admin_sort_header('created_at', 'Requested', $sort, $dir, 'connections.php', $queryParams);
    $thUpdated = admin_sort_header('updated_at', 'Updated', $sort, $dir, 'connections.php', $queryParams);
    $thStatus = admin_sort_header('status', 'Status', $sort, $dir, 'connections.php', $queryParams);

    $body = <<<HTML
    <!-- Connection KPI Summary Cards -->
    <div class="card-grid">
        <div class="card">
            <span class="label">Total Connection Requests</span>
            <p class="value">{$kpiTotal}</p>
            <div class="trend">All peer connection attempts</div>
        </div>
        <div class="card">
            <span class="label">Accepted Connections</span>
            <p class="value" style="color:#16a34a;">{$kpiAccepted}</p>
            <div class="trend">Successfully verified & paired</div>
        </div>
        <div class="card">
            <span class="label">Pending Verification</span>
            <p class="value" style="color:#b45309;">{$kpiPending}</p>
            <div class="trend">Awaiting host PIN / accept</div>
        </div>
        <div class="card">
            <span class="label">Expired / Rejected</span>
            <p class="value" style="color:#6b7280;">{$kpiExpired}</p>
            <div class="trend">Timed out or rejected tokens</div>
        </div>
    </div>

    <!-- Search & Filter Bar -->
    <div class="filter-bar">
        <form method="GET" action="{$connectionsUrl}" style="display:flex; gap:10px; flex-wrap:wrap; align-items:center; width:100%;">
            <div class="filter-group">
                <input type="text" name="q" value="{$searchSafe}" placeholder="Search token, system ID, device name, user..." class="filter-input" style="min-width:280px;">
            </div>
            <div class="filter-group">
                <select name="status" class="filter-select">
                    {$statusOptions}
                </select>
            </div>
            <button type="submit" class="filter-btn">Filter</button>
            <a href="{$connectionsUrl}" class="filter-reset">Reset</a>
        </form>
    </div>

    <!-- Connections Table -->
    <div class="section">
        <div class="section-header">
            <h3>Connection Requests</h3>
            <span class="kpi-pill">{$totalMatching} Requests Found</span>
        </div>
        <div style="overflow:auto;">
            <table>
                <thead>
                    <tr>
                        <th>{$thId}</th>
                        <th>Request Token</th>
                        <th>Requester Endpoint</th>
                        <th>Target Endpoint</th>
                        <th>{$thStatus}</th>
                        <th>{$thCreated}</th>
                        <th>{$thUpdated}</th>
                        <th>Expires At</th>
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

    echo admin_render_page('Connections', 'connections', $body);
} catch (Throwable $e) {
    admin_log_error($e, 'connections.php execution');
    echo admin_render_page('Connections', 'connections', admin_safe_error_html('Unexpected Error', 'An error occurred while loading the Connections page.', $e));
}
