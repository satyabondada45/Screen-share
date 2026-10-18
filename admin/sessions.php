<?php
require_once __DIR__ . '/includes/db.php';
require_once __DIR__ . '/includes/auth.php';
require_once __DIR__ . '/includes/layout.php';

admin_require_auth();

try {
    $pdo = admin_db();
} catch (Throwable $e) {
    admin_log_error($e, 'admin/sessions.php db connect');
    echo admin_render_page('System Error', 'error', admin_safe_error_html('Database Connection Error', 'Unable to connect to the database.', $e));
    exit;
}

$sessionTable = admin_table_exists($pdo, 'sessions') ? 'sessions' : null;
if (!$sessionTable) {
    echo admin_render_page('Sessions', 'sessions', admin_safe_error_html('Table Unavailable', 'The sessions database table is unavailable.'));
    exit;
}

try {
    $sessCols = admin_table_columns($pdo, 'sessions');
    $hasStartedAt = in_array('started_at', $sessCols, true);
    $hasEndedAt = in_array('ended_at', $sessCols, true);
    $hasBandwidth = in_array('bandwidth_used_mb', $sessCols, true);
    $hasStatus = in_array('status', $sessCols, true);
    $hasSessionKey = in_array('session_key', $sessCols, true);
    $hasClientIp = in_array('client_ip', $sessCols, true);

    // ─────────────────────────────────────────────────────────────────────────────
    // HANDLE SESSION TERMINATION
    // ─────────────────────────────────────────────────────────────────────────────
    if (isset($_GET['action']) && $_GET['action'] === 'terminate' && isset($_GET['id'])) {
        $sessionId = (int)$_GET['id'];
        $token = $_GET['token'] ?? '';
        if (!admin_verify_csrf_token($token)) {
            admin_set_flash('error', 'Invalid security token. Please try again.');
            header('Location: ' . admin_url('sessions.php'));
            exit;
        }

        try {
            $sStmt = $pdo->prepare('SELECT id, session_key, status, host_device_id FROM `sessions` WHERE id = ? LIMIT 1');
            $sStmt->execute([$sessionId]);
            $targetSession = $sStmt->fetch(PDO::FETCH_ASSOC);

            if (!$targetSession) {
                admin_set_flash('error', 'Session not found.');
            } elseif (strtolower($targetSession['status']) !== 'active') {
                admin_set_flash('error', 'This session is already ' . strtolower($targetSession['status']) . '.');
            } else {
                $endParams = ['completed'];
                $endCols = ['`status` = ?'];
                if ($hasEndedAt) {
                    $endCols[] = '`ended_at` = NOW()';
                }
                $endSql = 'UPDATE `sessions` SET ' . implode(', ', $endCols) . ' WHERE `id` = ? AND LOWER(`status`) = "active"';
                $endParams[] = $sessionId;

                $upd = $pdo->prepare($endSql);
                $upd->execute($endParams);

                admin_log_action($pdo, 'terminate_session', "Terminated remote session #{$sessionId} (key: {$targetSession['session_key']})");
                admin_set_flash('success', "Session #{$sessionId} was terminated successfully.");
            }
        } catch (Throwable $e) {
            admin_log_error($e, 'Session termination error');
            admin_set_flash('error', 'Unable to terminate session.');
        }

        header('Location: ' . admin_url('sessions.php'));
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
        'started_at' => $hasStartedAt ? 's.started_at' : 's.id',
        'ended_at' => $hasEndedAt ? 's.ended_at' : 's.id',
        'status' => $hasStatus ? 's.status' : 's.id',
        'bandwidth' => $hasBandwidth ? 's.bandwidth_used_mb' : 's.id',
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
        $searchFields = [];
        if ($hasSessionKey) $searchFields[] = 's.session_key';
        if ($hasClientIp) $searchFields[] = 's.client_ip';
        if (admin_table_exists($pdo, 'devices')) {
            $searchFields[] = 'd.name';
            $searchFields[] = 'd.system_id';
        }
        if (admin_table_exists($pdo, 'users')) {
            $searchFields[] = 'u.name';
            $searchFields[] = 'u.email';
        }

        if ($searchFields) {
            $where[] = '(' . implode(' OR ', array_map(fn($f) => "$f LIKE ?", $searchFields)) . ')';
            foreach ($searchFields as $unused) {
                $params[] = '%' . $search . '%';
            }
        }
    }

    if ($hasStatus && $statusFilter !== 'all') {
        $where[] = 'LOWER(s.status) = ?';
        $params[] = $statusFilter;
    }

    $whereSql = $where ? ' WHERE ' . implode(' AND ', $where) : '';

    // 1. KPI Metric Summary Counters
    $kpiTotal = (int) $pdo->query('SELECT COUNT(*) FROM `sessions`')->fetchColumn();
    $kpiActive = $hasStatus ? (int) $pdo->query('SELECT COUNT(*) FROM `sessions` WHERE LOWER(status) = "active"')->fetchColumn() : 0;
    $kpiCompleted = $hasStatus ? (int) $pdo->query('SELECT COUNT(*) FROM `sessions` WHERE LOWER(status) = "completed"')->fetchColumn() : 0;
    $kpiBandwidthMb = $hasBandwidth ? (float) $pdo->query('SELECT COALESCE(SUM(bandwidth_used_mb), 0) FROM `sessions`')->fetchColumn() : 0.0;
    $kpiBandwidthFmt = admin_format_bytes($kpiBandwidthMb * 1024 * 1024);

    // 2. Count Matching Records
    $deviceJoin = admin_table_exists($pdo, 'devices') ? 'LEFT JOIN `devices` d ON s.host_device_id = d.id' : '';
    $userJoin = (admin_table_exists($pdo, 'devices') && admin_table_exists($pdo, 'users')) ? 'LEFT JOIN `users` u ON d.user_id = u.id' : '';

    $countStmt = $pdo->prepare("
        SELECT COUNT(DISTINCT s.id) 
        FROM `sessions` s
        {$deviceJoin}
        {$userJoin}
        {$whereSql}
    ");
    $countStmt->execute($params);
    $totalMatchingSessions = (int) $countStmt->fetchColumn();

    // 3. Fetch Paginated Records
    $deviceSelect = admin_table_exists($pdo, 'devices') ? 'd.name AS host_device_name, d.system_id AS host_system_id,' : '"" AS host_device_name, "" AS host_system_id,';
    $userSelect = (admin_table_exists($pdo, 'devices') && admin_table_exists($pdo, 'users')) ? 'u.name AS host_user_name, u.email AS host_user_email,' : '"" AS host_user_name, "" AS host_user_email,';

    $sql = "
        SELECT s.*, 
               {$deviceSelect}
               {$userSelect}
               s.id AS session_pk
        FROM `sessions` s
        {$deviceJoin}
        {$userJoin}
        {$whereSql}
        GROUP BY s.id
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
            $sessionKey = htmlspecialchars((string) ($row['session_key'] ?? ('Session #' . $id)), ENT_QUOTES, 'UTF-8');
            $devName = htmlspecialchars((string) ($row['host_device_name'] ?? ('Device #' . ($row['host_device_id'] ?? ''))), ENT_QUOTES, 'UTF-8');
            $devSysId = htmlspecialchars((string) ($row['host_system_id'] ?? ''), ENT_QUOTES, 'UTF-8');
            $devLink = !empty($row['host_device_id']) 
                ? '<a href="' . htmlspecialchars(admin_url('devices.php', ['view' => (int) $row['host_device_id']]), ENT_QUOTES, 'UTF-8') . '" style="color:var(--text-primary); font-weight:600;">' . $devName . '</a>' . ($devSysId !== '' ? '<br><small style="color:var(--text-muted); font-family:monospace;">' . $devSysId . '</small>' : '')
                : $devName;

            $userName = htmlspecialchars((string) ($row['host_user_name'] ?? '—'), ENT_QUOTES, 'UTF-8');
            $userEmail = htmlspecialchars((string) ($row['host_user_email'] ?? ''), ENT_QUOTES, 'UTF-8');
            $userDisplay = ($userName !== '—') 
                ? '<strong>' . $userName . '</strong><br><small style="color:var(--text-muted);">' . $userEmail . '</small>' 
                : '<span style="color:var(--text-muted);">Unassigned</span>';

            $st = strtoupper((string) ($row['status'] ?? 'COMPLETED'));
            $stClass = strtolower($st);
            $bwMb = (float) ($row['bandwidth_used_mb'] ?? 0);
            $bwFormatted = admin_format_bytes($bwMb * 1024 * 1024);
            $durationFormatted = admin_format_duration($row['started_at'] ?? null, $row['ended_at'] ?? null);
            $startedAgo = admin_time_ago($row['started_at'] ?? null);
            $startedExact = !empty($row['started_at']) ? admin_utc_date((string)$row['started_at']) : 'N/A';

            $connMode = htmlspecialchars(ucfirst(strtolower((string)($row['connection_mode'] ?? 'desktop'))), ENT_QUOTES, 'UTF-8');
            $actionBtn = '';
            if (strtolower($st) === 'active') {
                $csrfToken = admin_csrf_token();
                $termUrl = htmlspecialchars(admin_url('sessions.php', ['action' => 'terminate', 'id' => $id, 'token' => $csrfToken]), ENT_QUOTES, 'UTF-8');
                $actionBtn = '<a class="outline-button" href="' . $termUrl . '" onclick="return confirm(\'Terminate this active streaming session now?\');" style="color:#dc2626; border-color:#fecaca; font-size:0.75rem; padding:4px 10px;">Terminate</a>';
            } else {
                $actionBtn = '<span style="color:var(--text-muted); font-size:0.8rem;">—</span>';
            }

            $rowsHtml .= "<tr>
                <td><code>{$sessionKey}</code></td>
                <td>{$devLink}</td>
                <td>{$userDisplay}</td>
                <td><span class=\"status-badge\" style=\"background:#f1f5f9; color:#334155;\">{$connMode}</span></td>
                <td><span class=\"status-badge status-{$stClass}\">{$st}</span></td>
                <td>{$durationFormatted}</td>
                <td>{$bwFormatted}</td>
                <td><span title=\"{$startedExact}\">{$startedAgo}</span></td>
                <td>{$actionBtn}</td>
            </tr>";
        }
    } else {
        $rowsHtml = '<tr><td colspan="9" class="empty-state">No remote sessions match the selected query.</td></tr>';
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
    foreach (['all' => 'All Statuses', 'active' => 'Active', 'completed' => 'Completed', 'rejected' => 'Rejected'] as $val => $lbl) {
        $statusOptions .= '<option value="' . $val . '"' . ($statusFilter === $val ? ' selected' : '') . '>' . $lbl . '</option>';
    }

    $searchSafe = htmlspecialchars($search, ENT_QUOTES, 'UTF-8');

    $thStarted = admin_sort_header('started_at', 'Duration', $sort, $dir, 'sessions.php', $queryParams);
    $thBandwidth = admin_sort_header('bandwidth', 'Bandwidth', $sort, $dir, 'sessions.php', $queryParams);
    $thStatus = admin_sort_header('status', 'Status', $sort, $dir, 'sessions.php', $queryParams);

    $body = <<<HTML
<!-- Remote Sessions KPI Summary Cards -->
<div class="card-grid">
    <div class="card">
        <span class="label">Total Sessions</span>
        <p class="value">{$kpiTotal}</p>
        <div class="trend">All recorded streaming sessions</div>
    </div>
    <div class="card">
        <span class="label">Active Sessions</span>
        <p class="value" style="color:#16a34a;">{$kpiActive}</p>
        <div class="trend">Currently streaming live</div>
    </div>
    <div class="card">
        <span class="label">Completed Sessions</span>
        <p class="value" style="color:#1d4ed8;">{$kpiCompleted}</p>
        <div class="trend">Successfully ended sessions</div>
    </div>
    <div class="card">
        <span class="label">Total Bandwidth Streamed</span>
        <p class="value" style="color:#b45309;">{$kpiBandwidthFmt}</p>
        <div class="trend">Data transferred through relay</div>
    </div>
</div>

<!-- Search & Filter Bar -->
<div class="filter-bar">
    <form method="GET" action="{$sessionsUrl}" style="display:flex; gap:10px; flex-wrap:wrap; align-items:center; width:100%;">
        <div class="filter-group">
            <input type="text" name="q" value="{$searchSafe}" placeholder="Search session key, device, user..." class="filter-input" style="min-width:280px;">
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
        <h3>Streaming Remote Sessions</h3>
        <span class="kpi-pill">{$totalMatchingSessions} Sessions Found</span>
    </div>
    <div style="overflow:auto;">
        <table>
            <thead>
                <tr>
                    <th>Session Key</th>
                    <th>Host Device</th>
                    <th>Device Owner</th>
                    <th>Mode</th>
                    <th>{$thStatus}</th>
                    <th>{$thStarted}</th>
                    <th>{$thBandwidth}</th>
                    <th>Started</th>
                    <th>Action</th>
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

    echo admin_render_page('Remote Sessions', 'sessions', $body);

} catch (Throwable $e) {
    admin_log_error($e, 'sessions.php execution');
    echo admin_render_page('Remote Sessions', 'sessions', admin_safe_error_html('Unexpected Error', 'An error occurred while loading the Sessions page.', $e));
}
