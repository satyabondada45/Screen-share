<?php
require_once __DIR__ . '/includes/db.php';
require_once __DIR__ . '/includes/auth.php';
require_once __DIR__ . '/includes/layout.php';

admin_require_auth();

try {
    $pdo = admin_db();

    $notifTable = admin_table_exists($pdo, 'notifications') ? 'notifications' : null;
    if (!$notifTable) {
        echo admin_render_page('Notifications', 'notifications', '<div class="flash error">The notifications database table is unavailable.</div>');
        exit;
    }

    $hasTitle = admin_column_exists($pdo, 'notifications', 'title');
    $hasMessage = admin_column_exists($pdo, 'notifications', 'message');
    $hasIsRead = admin_column_exists($pdo, 'notifications', 'is_read');
    $hasUserId = admin_column_exists($pdo, 'notifications', 'user_id');
    $hasCreatedAt = admin_column_exists($pdo, 'notifications', 'created_at');

    $hasUserName = admin_column_exists($pdo, 'users', 'name');
    $hasUserEmail = admin_column_exists($pdo, 'users', 'email');
    $hasUserUsername = admin_column_exists($pdo, 'users', 'username');

    // ─────────────────────────────────────────────────────────────────────────────
    // SEARCH, FILTER, SORT & PAGINATION
    // ─────────────────────────────────────────────────────────────────────────────

    $search = trim((string) ($_GET['q'] ?? ''));
    $statusFilter = strtolower(trim((string) ($_GET['status'] ?? 'all')));
    $statusFilter = in_array($statusFilter, ['all', 'unread', 'read'], true) ? $statusFilter : 'all';

    // Sorting Allowlist
    $allowedSorts = [
        'id' => 'n.id',
    ];
    if ($hasCreatedAt) $allowedSorts['created_at'] = 'n.created_at';
    if ($hasIsRead) $allowedSorts['is_read'] = 'n.is_read';

    $sort = (string) ($_GET['sort'] ?? 'id');
    $sortColumn = $allowedSorts[$sort] ?? 'n.id';
    $dir = strtolower((string) ($_GET['dir'] ?? 'desc')) === 'asc' ? 'ASC' : 'DESC';

    $page = max(1, (int) ($_GET['page'] ?? 1));
    $perPage = 25;
    $offset = ($page - 1) * $perPage;

    $where = [];
    $params = [];

    if ($search !== '') {
        $searchFields = [];
        if ($hasTitle) $searchFields[] = 'n.title LIKE ?';
        if ($hasMessage) $searchFields[] = 'n.message LIKE ?';
        if ($hasUserId) {
            if ($hasUserName) $searchFields[] = 'u.name LIKE ?';
            if ($hasUserEmail) $searchFields[] = 'u.email LIKE ?';
            if ($hasUserUsername) $searchFields[] = 'u.username LIKE ?';
        }

        if (!empty($searchFields)) {
            $where[] = '(' . implode(' OR ', $searchFields) . ')';
            $searchParam = '%' . $search . '%';
            for ($i = 0; $i < count($searchFields); $i++) {
                $params[] = $searchParam;
            }
        }
    }

    if ($statusFilter === 'unread' && $hasIsRead) {
        $where[] = 'n.is_read = 0';
    } elseif ($statusFilter === 'read' && $hasIsRead) {
        $where[] = 'n.is_read = 1';
    }

    $whereSql = $where ? ' WHERE ' . implode(' AND ', $where) : '';

    // 1. KPI Metric Summary Counters
    $kpiTotal = (int) $pdo->query('SELECT COUNT(*) FROM `notifications`')->fetchColumn();
    $kpiUnread = $hasIsRead ? (int) $pdo->query('SELECT COUNT(*) FROM `notifications` WHERE is_read = 0')->fetchColumn() : 0;
    $kpiRead = $hasIsRead ? (int) $pdo->query('SELECT COUNT(*) FROM `notifications` WHERE is_read = 1')->fetchColumn() : 0;
    $kpiRecipients = $hasUserId ? (int) $pdo->query('SELECT COUNT(DISTINCT user_id) FROM `notifications`')->fetchColumn() : 0;

    $joinClause = $hasUserId ? 'LEFT JOIN `users` u ON n.user_id = u.id' : '';
    $userCols = '';
    if ($hasUserId) {
        $userCols = ', u.id AS user_pk';
        if ($hasUserName) $userCols .= ', u.name AS user_name';
        if ($hasUserEmail) $userCols .= ', u.email AS user_email';
    }

    // 2. Count Matching Records
    $countStmt = $pdo->prepare("
        SELECT COUNT(*) 
        FROM `notifications` n
        {$joinClause}
        {$whereSql}
    ");
    $countStmt->execute($params);
    $totalMatching = (int) $countStmt->fetchColumn();

    // 3. Fetch Paginated Records
    $sql = "
        SELECT n.*
               {$userCols}
        FROM `notifications` n
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
        foreach ($rows as $row) {
            $nId = (int) $row['id'];
            $title = htmlspecialchars((string) ($row['title'] ?? 'Alert'), ENT_QUOTES, 'UTF-8');
            $message = htmlspecialchars((string) ($row['message'] ?? '—'), ENT_QUOTES, 'UTF-8');
            
            $userId = (int) ($row['user_pk'] ?? ($row['user_id'] ?? 0));
            $userName = htmlspecialchars((string) ($row['user_name'] ?? ($userId > 0 ? ('User #' . $userId) : 'Broadcast / System')), ENT_QUOTES, 'UTF-8');
            $userEmail = htmlspecialchars((string) ($row['user_email'] ?? ''), ENT_QUOTES, 'UTF-8');
            $userDisplay = ($userId > 0)
                ? '<a href="' . htmlspecialchars(admin_url('users.php', ['view' => $userId]), ENT_QUOTES, 'UTF-8') . '" style="color:var(--text-primary); font-weight:600;">' . $userName . '</a>'
                : '<span style="color:var(--text-muted);">' . $userName . '</span>';

            $isRead = !empty($row['is_read']);
            $statusBadge = $isRead 
                ? '<span class="status-badge status-active">Read</span>'
                : '<span class="status-badge status-warning">Unread</span>';

            $created = !empty($row['created_at']) ? htmlspecialchars((string) $row['created_at'], ENT_QUOTES, 'UTF-8') : 'N/A';
            $timeAgo = admin_time_ago($row['created_at'] ?? null);

            $rowHtml .= "<tr>
                <td>#{$nId}</td>
                <td>{$userDisplay}</td>
                <td><strong>{$title}</strong></td>
                <td>{$message}</td>
                <td>{$statusBadge}</td>
                <td><small title=\"{$created}\">{$timeAgo}</small></td>
            </tr>";
        }
    } else {
        $rowHtml = '<tr><td colspan="6" class="empty-state">No notifications match the selected filter.</td></tr>';
    }

    $notifUrl = htmlspecialchars(admin_url('notifications.php'), ENT_QUOTES, 'UTF-8');
    $queryParams = array_filter([
        'q' => $search !== '' ? $search : null,
        'status' => $statusFilter !== 'all' ? $statusFilter : null,
        'sort' => $sort !== 'id' ? $sort : null,
        'dir' => $dir !== 'DESC' ? strtolower($dir) : null,
    ]);

    $paginationHtml = admin_pagination($totalMatching, $perPage, $page, 'notifications.php', $queryParams);

    $statusOptions = '';
    foreach (['all' => 'All Notifications', 'unread' => 'Unread Only', 'read' => 'Read Only'] as $v => $l) {
        $statusOptions .= '<option value="' . $v . '"' . ($statusFilter === $v ? ' selected' : '') . '>' . $l . '</option>';
    }

    $searchSafe = htmlspecialchars($search, ENT_QUOTES, 'UTF-8');

    $thId = admin_sort_header('id', 'ID', $sort, $dir, 'notifications.php', $queryParams);
    $thCreated = admin_sort_header('created_at', 'Sent At', $sort, $dir, 'notifications.php', $queryParams);
    $thStatus = admin_sort_header('is_read', 'Status', $sort, $dir, 'notifications.php', $queryParams);

    $body = <<<HTML
    <!-- Notifications KPI Summary Cards -->
    <div class="card-grid">
        <div class="card">
            <span class="label">Total Notifications</span>
            <p class="value">{$kpiTotal}</p>
            <div class="trend">All system alerts dispatched</div>
        </div>
        <div class="card">
            <span class="label">Unread Messages</span>
            <p class="value" style="color:#b45309;">{$kpiUnread}</p>
            <div class="trend">Awaiting recipient viewing</div>
        </div>
        <div class="card">
            <span class="label">Read Notifications</span>
            <p class="value" style="color:#16a34a;">{$kpiRead}</p>
            <div class="trend">Acknowledged by users</div>
        </div>
        <div class="card">
            <span class="label">Distinct Recipients</span>
            <p class="value">{$kpiRecipients}</p>
            <div class="trend">Accounts with notifications</div>
        </div>
    </div>

    <!-- Search & Interactive Filter Bar -->
    <div class="filter-bar">
        <form method="GET" action="{$notifUrl}" style="display:flex; gap:10px; flex-wrap:wrap; align-items:center; width:100%;">
            <div class="filter-group">
                <input type="text" name="q" value="{$searchSafe}" placeholder="Search title, message, user..." class="filter-input" style="min-width:280px;">
            </div>
            <div class="filter-group">
                <select name="status" class="filter-select">
                    {$statusOptions}
                </select>
            </div>
            <button type="submit" class="filter-btn">Filter</button>
            <a href="{$notifUrl}" class="filter-reset">Reset</a>
        </form>
    </div>

    <!-- Notifications Table -->
    <div class="section">
        <div class="section-header">
            <h3>User Notifications</h3>
            <span class="kpi-pill">{$totalMatching} Dispatched Alerts</span>
        </div>
        <div style="overflow:auto;">
            <table>
                <thead>
                    <tr>
                        <th>{$thId}</th>
                        <th>Recipient</th>
                        <th>Title</th>
                        <th>Message</th>
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

    echo admin_render_page('Notifications', 'notifications', $body);
} catch (Throwable $e) {
    admin_log_error($e, 'notifications.php execution');
    echo admin_render_page('Notifications', 'notifications', admin_safe_error_html('Unexpected Error', 'An error occurred while loading the Notifications page.', $e));
}
