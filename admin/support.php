<?php
require_once __DIR__ . '/includes/db.php';
require_once __DIR__ . '/includes/auth.php';
require_once __DIR__ . '/includes/layout.php';

admin_require_auth();

try {
    $pdo = admin_db();

    $supportTable = admin_table_exists($pdo, 'support_tickets') ? 'support_tickets' : null;
    if (!$supportTable) {
        echo admin_render_page('Support', 'support', '<div class="flash error">The support tickets database table is unavailable.</div>');
        exit;
    }

    $hasUserName = admin_column_exists($pdo, 'users', 'name');
    $hasUserEmail = admin_column_exists($pdo, 'users', 'email');
    $hasUserUsername = admin_column_exists($pdo, 'users', 'username');
    $hasTicketSubject = admin_column_exists($pdo, 'support_tickets', 'subject');
    $hasTicketMessage = admin_column_exists($pdo, 'support_tickets', 'message');
    $hasTicketStatus = admin_column_exists($pdo, 'support_tickets', 'status');
    $hasTicketPriority = admin_column_exists($pdo, 'support_tickets', 'priority');
    $hasTicketUserId = admin_column_exists($pdo, 'support_tickets', 'user_id');

    // ─────────────────────────────────────────────────────────────────────────────
    // HANDLE TICKET STATUS UPDATE (POST)
    // ─────────────────────────────────────────────────────────────────────────────
    if (($_SERVER['REQUEST_METHOD'] ?? '') === 'POST' && isset($_POST['update_ticket'])) {
        if (!admin_verify_csrf_token($_POST['csrf_token'] ?? null)) {
            admin_set_flash('error', 'Invalid security token. Please refresh.');
        } else {
            $ticketId = (int) ($_POST['ticket_id'] ?? 0);
            $newStatus = strtolower(trim((string) ($_POST['status'] ?? '')));
            $newPriority = strtolower(trim((string) ($_POST['priority'] ?? '')));

            if (!in_array($newStatus, ['open', 'in_progress', 'resolved', 'closed'], true)) {
                $newStatus = 'open';
            }
            if (!in_array($newPriority, ['low', 'medium', 'high'], true)) {
                $newPriority = 'medium';
            }

            try {
                $updFields = ['`status` = ?', '`priority` = ?'];
                $updParams = [$newStatus, $newPriority];
                if (admin_column_exists($pdo, 'support_tickets', 'updated_at')) {
                    $updFields[] = '`updated_at` = NOW()';
                }
                $updParams[] = $ticketId;

                $upd = $pdo->prepare('UPDATE `support_tickets` SET ' . implode(', ', $updFields) . ' WHERE `id` = ?');
                $upd->execute($updParams);
                admin_log_action($pdo, 'update_ticket', "Updated support ticket #{$ticketId} status to '{$newStatus}', priority to '{$newPriority}'");
                admin_set_flash('success', "Ticket #{$ticketId} updated successfully.");
                header('Location: ' . admin_url('support.php', ['ticket' => $ticketId]));
                exit;
            } catch (Throwable $e) {
                error_log('Support ticket update error: ' . $e->getMessage());
                admin_set_flash('error', 'Failed to update ticket: ' . $e->getMessage());
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // TICKET DETAIL VIEW (?ticket = ID)
    // ─────────────────────────────────────────────────────────────────────────────
    $selectedTicketId = (int) ($_GET['ticket'] ?? 0);
    if ($selectedTicketId > 0) {
        $userSelectCols = [];
        if ($hasUserName) $userSelectCols[] = 'u.name AS user_name';
        if ($hasUserEmail) $userSelectCols[] = 'u.email AS user_email';
        if ($hasUserUsername) $userSelectCols[] = 'u.username AS user_username';
        $userSelectSql = !empty($userSelectCols) ? ', ' . implode(', ', $userSelectCols) : '';

        $joinUserSql = $hasTicketUserId ? 'LEFT JOIN `users` u ON st.user_id = u.id' : '';

        $tStmt = $pdo->prepare("
            SELECT st.*
                   {$userSelectSql}
            FROM `support_tickets` st
            {$joinUserSql}
            WHERE st.id = ?
            LIMIT 1
        ");
        $tStmt->execute([$selectedTicketId]);
        $ticket = $tStmt->fetch(PDO::FETCH_ASSOC);

        if (!$ticket) {
            admin_set_flash('error', 'The requested support ticket could not be found.');
            header('Location: ' . admin_url('support.php'));
            exit;
        }

        $tId = (int) $ticket['id'];
        $subject = htmlspecialchars((string) ($ticket['subject'] ?? 'No Subject'), ENT_QUOTES, 'UTF-8');
        $message = nl2br(htmlspecialchars((string) ($ticket['message'] ?? '—'), ENT_QUOTES, 'UTF-8'));
        $ticketUserId = isset($ticket['user_id']) ? (int) $ticket['user_id'] : 0;
        $userName = htmlspecialchars((string) ($ticket['user_name'] ?? ($ticketUserId > 0 ? ('User #' . $ticketUserId) : 'Guest')), ENT_QUOTES, 'UTF-8');
        $userEmail = htmlspecialchars((string) ($ticket['user_email'] ?? '—'), ENT_QUOTES, 'UTF-8');
        $userLink = ($ticketUserId > 0)
            ? '<a href="' . htmlspecialchars(admin_url('users.php', ['view' => $ticketUserId]), ENT_QUOTES, 'UTF-8') . '" style="color:var(--accent); font-weight:600;">' . $userName . '</a>'
            : $userName;

        $statusVal = strtoupper((string) ($ticket['status'] ?? 'OPEN'));
        $statusClass = strtolower($statusVal);
        $priorityVal = strtoupper((string) ($ticket['priority'] ?? 'MEDIUM'));
        $priorityClass = strtolower($priorityVal);

        $created = admin_time_ago($ticket['created_at'] ?? null);
        $updated = !empty($ticket['updated_at']) ? admin_time_ago($ticket['updated_at']) : 'Never';

        $csrfToken = htmlspecialchars(admin_csrf_token(), ENT_QUOTES, 'UTF-8');
        $supportBackUrl = htmlspecialchars(admin_url('support.php'), ENT_QUOTES, 'UTF-8');

        $statusOptions = '';
        foreach (['open' => 'Open', 'in_progress' => 'In Progress', 'resolved' => 'Resolved', 'closed' => 'Closed'] as $v => $l) {
            $statusOptions .= '<option value="' . $v . '"' . (strtolower($statusVal) === $v ? ' selected' : '') . '>' . $l . '</option>';
        }

        $priorityOptions = '';
        foreach (['low' => 'Low Priority', 'medium' => 'Medium Priority', 'high' => 'High Priority'] as $v => $l) {
            $priorityOptions .= '<option value="' . $v . '"' . (strtolower($priorityVal) === $v ? ' selected' : '') . '>' . $l . '</option>';
        }

        $body = <<<HTML
    <div class="section">
        <div class="section-header">
            <h3>Ticket #{$tId}: {$subject}</h3>
            <div class="mini-actions"><a class="outline-button" href="{$supportBackUrl}">&larr; Back to Tickets</a></div>
        </div>
        <div class="detail-panels">
            <div class="detail-panel">
                <h4>Ticket Overview</h4>
                <div class="detail-row"><span>Ticket ID</span><strong>#{$tId}</strong></div>
                <div class="detail-row"><span>Submitted By</span><strong>{$userLink}</strong></div>
                <div class="detail-row"><span>User Email</span><strong>{$userEmail}</strong></div>
                <div class="detail-row"><span>Current Status</span><strong><span class="status-badge status-{$statusClass}">{$statusVal}</span></strong></div>
                <div class="detail-row"><span>Priority</span><strong><span class="kpi-pill">{$priorityVal}</span></strong></div>
                <div class="detail-row"><span>Submitted</span><strong>{$created}</strong></div>
                <div class="detail-row"><span>Last Updated</span><strong>{$updated}</strong></div>
            </div>
            <div class="detail-panel">
                <h4>Administrative Actions</h4>
                <form method="POST" action="{$supportBackUrl}" style="display:flex; flex-direction:column; gap:12px; margin-top:8px;">
                    <input type="hidden" name="csrf_token" value="{$csrfToken}">
                    <input type="hidden" name="update_ticket" value="1">
                    <input type="hidden" name="ticket_id" value="{$tId}">
                    <div class="field">
                        <label style="display:block; margin-bottom:4px; font-weight:600; font-size:0.8rem;">Change Status</label>
                        <select name="status" class="filter-select" style="width:100%;">
                            {$statusOptions}
                        </select>
                    </div>
                    <div class="field">
                        <label style="display:block; margin-bottom:4px; font-weight:600; font-size:0.8rem;">Change Priority</label>
                        <select name="priority" class="filter-select" style="width:100%;">
                            {$priorityOptions}
                        </select>
                    </div>
                    <button type="submit" class="button" style="width:100%;">Update Ticket Status</button>
                </form>
            </div>
        </div>
    </div>

    <div class="section">
        <div class="section-header">
            <h3>Customer Message</h3>
        </div>
        <div style="padding:20px; background:var(--bg); border-radius:var(--radius-md); font-size:0.95rem; line-height:1.7; border:1px solid var(--border);">
            {$message}
        </div>
    </div>
HTML;

        echo admin_render_page('Support Ticket #' . $tId, 'support', $body);
        exit;
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // LIST VIEW WITH SEARCH, STATUS/PRIORITY FILTER, PAGINATION & METRICS
    // ─────────────────────────────────────────────────────────────────────────────

    $search = trim((string) ($_GET['q'] ?? ''));
    $status = strtolower(trim((string) ($_GET['status'] ?? 'all')));
    $priority = strtolower(trim((string) ($_GET['priority'] ?? 'all')));
    $status = in_array($status, ['all', 'open', 'in_progress', 'resolved', 'closed'], true) ? $status : 'all';
    $priority = in_array($priority, ['all', 'low', 'medium', 'high'], true) ? $priority : 'all';

    // Sorting Allowlist
    $allowedSorts = [
        'id' => 'st.id',
        'created_at' => 'st.created_at',
        'updated_at' => 'st.updated_at',
        'priority' => 'st.priority',
        'status' => 'st.status',
    ];
    $sort = (string) ($_GET['sort'] ?? 'id');
    $sortColumn = $allowedSorts[$sort] ?? 'st.id';
    $dir = strtolower((string) ($_GET['dir'] ?? 'desc')) === 'asc' ? 'ASC' : 'DESC';

    $page = max(1, (int) ($_GET['page'] ?? 1));
    $perPage = 20;
    $offset = ($page - 1) * $perPage;

    $where = [];
    $params = [];

    if ($search !== '') {
        $searchFields = [];
        if ($hasTicketSubject) $searchFields[] = 'st.subject LIKE ?';
        if ($hasTicketMessage) $searchFields[] = 'st.message LIKE ?';
        if ($hasTicketUserId) {
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

    if ($status !== 'all' && $hasTicketStatus) {
        $where[] = 'LOWER(st.status) = ?';
        $params[] = $status;
    }

    if ($priority !== 'all' && $hasTicketPriority) {
        $where[] = 'LOWER(st.priority) = ?';
        $params[] = $priority;
    }

    $whereSql = $where ? ' WHERE ' . implode(' AND ', $where) : '';

    // 1. KPI Metric Summary Counters
    $kpiTotal = (int) $pdo->query('SELECT COUNT(*) FROM `support_tickets`')->fetchColumn();
    $kpiOpen = $hasTicketStatus ? (int) $pdo->query('SELECT COUNT(*) FROM `support_tickets` WHERE LOWER(status) = "open"')->fetchColumn() : 0;
    $kpiInProgress = $hasTicketStatus ? (int) $pdo->query('SELECT COUNT(*) FROM `support_tickets` WHERE LOWER(status) = "in_progress"')->fetchColumn() : 0;
    $kpiResolved = $hasTicketStatus ? (int) $pdo->query('SELECT COUNT(*) FROM `support_tickets` WHERE LOWER(status) IN ("resolved", "closed")')->fetchColumn() : 0;

    $joinUserSql = $hasTicketUserId ? 'LEFT JOIN `users` u ON st.user_id = u.id' : '';
    $userCols = '';
    if ($hasTicketUserId) {
        $userCols = ', u.id AS user_pk';
        if ($hasUserName) $userCols .= ', u.name AS user_name';
        if ($hasUserEmail) $userCols .= ', u.email AS user_email';
    }

    // 2. Count Matching Records
    $countStmt = $pdo->prepare("
        SELECT COUNT(*) 
        FROM `support_tickets` st
        {$joinUserSql}
        {$whereSql}
    ");
    $countStmt->execute($params);
    $totalMatching = (int) $countStmt->fetchColumn();

    // 3. Fetch Paginated Records
    $sql = "
        SELECT st.*
               {$userCols}
        FROM `support_tickets` st
        {$joinUserSql}
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
            $tId = (int) $row['id'];
            $subject = htmlspecialchars((string) ($row['subject'] ?? 'General inquiry'), ENT_QUOTES, 'UTF-8');
            
            $userId = (int) ($row['user_pk'] ?? ($row['user_id'] ?? 0));
            $userName = htmlspecialchars((string) ($row['user_name'] ?? ($userId > 0 ? ('User #' . $userId) : 'Guest')), ENT_QUOTES, 'UTF-8');
            $userEmail = htmlspecialchars((string) ($row['user_email'] ?? ''), ENT_QUOTES, 'UTF-8');
            $userDisplay = ($userId > 0)
                ? '<strong>' . $userName . '</strong><br><small style="color:var(--text-muted);">' . $userEmail . '</small>'
                : 'Guest';

            $statusVal = strtoupper((string) ($row['status'] ?? 'OPEN'));
            $statusClass = strtolower($statusVal);
            $priorityVal = strtoupper((string) ($row['priority'] ?? 'MEDIUM'));
            $priorityClass = strtolower($priorityVal);

            $created = admin_time_ago($row['created_at'] ?? null);
            $updated = !empty($row['updated_at']) ? admin_time_ago($row['updated_at']) : 'Never';
            $viewUrl = htmlspecialchars(admin_url('support.php', ['ticket' => $tId]), ENT_QUOTES, 'UTF-8');

            $rowHtml .= "<tr>
                <td>#{$tId}</td>
                <td><strong>{$subject}</strong></td>
                <td>{$userDisplay}</td>
                <td><span class=\"kpi-pill\">{$priorityVal}</span></td>
                <td><span class=\"status-badge status-{$statusClass}\">{$statusVal}</span></td>
                <td><small>{$created}</small></td>
                <td><small>{$updated}</small></td>
                <td><a class=\"outline-button\" href=\"{$viewUrl}\">View & Manage</a></td>
            </tr>";
        }
    } else {
        $rowHtml = '<tr><td colspan="8" class="empty-state">No support tickets match the selected filters.</td></tr>';
    }

    $supportUrl = htmlspecialchars(admin_url('support.php'), ENT_QUOTES, 'UTF-8');
    $queryParams = array_filter([
        'q' => $search !== '' ? $search : null,
        'status' => $status !== 'all' ? $status : null,
        'priority' => $priority !== 'all' ? $priority : null,
        'sort' => $sort !== 'id' ? $sort : null,
        'dir' => $dir !== 'DESC' ? strtolower($dir) : null,
    ]);

    $paginationHtml = admin_pagination($totalMatching, $perPage, $page, 'support.php', $queryParams);

    $statusOptions = '';
    foreach (['all' => 'All Statuses', 'open' => 'Open', 'in_progress' => 'In Progress', 'resolved' => 'Resolved', 'closed' => 'Closed'] as $v => $l) {
        $statusOptions .= '<option value="' . $v . '"' . ($status === $v ? ' selected' : '') . '>' . $l . '</option>';
    }

    $priorityOptions = '';
    foreach (['all' => 'All Priorities', 'low' => 'Low Priority', 'medium' => 'Medium Priority', 'high' => 'High Priority'] as $v => $l) {
        $priorityOptions .= '<option value="' . $v . '"' . ($priority === $v ? ' selected' : '') . '>' . $l . '</option>';
    }

    $searchSafe = htmlspecialchars($search, ENT_QUOTES, 'UTF-8');

    $thId = admin_sort_header('id', 'ID', $sort, $dir, 'support.php', $queryParams);
    $thPriority = admin_sort_header('priority', 'Priority', $sort, $dir, 'support.php', $queryParams);
    $thStatus = admin_sort_header('status', 'Status', $sort, $dir, 'support.php', $queryParams);
    $thCreated = admin_sort_header('created_at', 'Created', $sort, $dir, 'support.php', $queryParams);
    $thUpdated = admin_sort_header('updated_at', 'Updated', $sort, $dir, 'support.php', $queryParams);

    $body = <<<HTML
    <!-- Support KPI Summary Cards -->
    <div class="card-grid">
        <div class="card">
            <span class="label">Total Support Tickets</span>
            <p class="value">{$kpiTotal}</p>
            <div class="trend">All customer inquiries</div>
        </div>
        <div class="card">
            <span class="label">Open Tickets</span>
            <p class="value" style="color:#e53935;">{$kpiOpen}</p>
            <div class="trend">Require administrative attention</div>
        </div>
        <div class="card">
            <span class="label">In Progress</span>
            <p class="value" style="color:#b45309;">{$kpiInProgress}</p>
            <div class="trend">Currently being investigated</div>
        </div>
        <div class="card">
            <span class="label">Resolved / Closed</span>
            <p class="value" style="color:#16a34a;">{$kpiResolved}</p>
            <div class="trend">Successfully answered tickets</div>
        </div>
    </div>

    <!-- Search & Interactive Filter Bar -->
    <div class="filter-bar">
        <form method="GET" action="{$supportUrl}" style="display:flex; gap:10px; flex-wrap:wrap; align-items:center; width:100%;">
            <div class="filter-group">
                <input type="text" name="q" value="{$searchSafe}" placeholder="Search ticket subject, message, customer..." class="filter-input" style="min-width:260px;">
            </div>
            <div class="filter-group">
                <select name="status" class="filter-select">
                    {$statusOptions}
                </select>
            </div>
            <div class="filter-group">
                <select name="priority" class="filter-select">
                    {$priorityOptions}
                </select>
            </div>
            <button type="submit" class="filter-btn">Filter</button>
            <a href="{$supportUrl}" class="filter-reset">Reset</a>
        </form>
    </div>

    <!-- Support Tickets Table -->
    <div class="section">
        <div class="section-header">
            <h3>Support Inquiries</h3>
            <span class="kpi-pill">{$totalMatching} Tickets Found</span>
        </div>
        <div style="overflow:auto;">
            <table>
                <thead>
                    <tr>
                        <th>{$thId}</th>
                        <th>Subject</th>
                        <th>Customer</th>
                        <th>{$thPriority}</th>
                        <th>{$thStatus}</th>
                        <th>{$thCreated}</th>
                        <th>{$thUpdated}</th>
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

    echo admin_render_page('Support', 'support', $body);
} catch (Throwable $e) {
    admin_log_error($e, 'support.php execution');
    echo admin_render_page('Support', 'support', admin_safe_error_html('Unexpected Error', 'An error occurred while loading the Support page.', $e));
}
