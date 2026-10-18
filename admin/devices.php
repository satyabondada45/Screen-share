<?php
require_once __DIR__ . '/includes/db.php';
require_once __DIR__ . '/includes/auth.php';
require_once __DIR__ . '/includes/layout.php';

admin_require_auth();

try {
    $pdo = admin_db();
} catch (Throwable $e) {
    admin_log_error($e, 'admin/devices.php db connect');
    echo admin_render_page('System Error', 'error', admin_safe_error_html('Database Connection Error', 'Unable to connect to the database. Please check system configuration.', $e));
    exit;
}

$deviceTable = admin_table_exists($pdo, 'devices') ? 'devices' : null;
$subscriptionTable = admin_table_exists($pdo, 'subscriptions') ? 'subscriptions' : null;
$planTable = admin_table_exists($pdo, 'plans') ? 'plans' : null;
$userTable = admin_table_exists($pdo, 'users') ? 'users' : null;

if (!$deviceTable) {
    echo admin_render_page('Remote Devices', 'devices', admin_safe_error_html('Table Unavailable', 'The devices database table is currently unavailable.'));
    exit;
}

try {
    $devCols = admin_table_columns($pdo, 'devices');
    $subCols = $subscriptionTable ? admin_table_columns($pdo, 'subscriptions') : [];
    $hasSubUserId = in_array('user_id', $subCols, true);
    $hasSubDeviceId = in_array('device_id', $subCols, true);
    $subExpCol = in_array('expires_at', $subCols, true) ? 'expires_at' : (in_array('expiry_date', $subCols, true) ? 'expiry_date' : null);
    $subStartCol = in_array('starts_at', $subCols, true) ? 'starts_at' : (in_array('start_date', $subCols, true) ? 'start_date' : null);

    $planCols = $planTable ? admin_table_columns($pdo, 'plans') : [];
    $hasPlanUnit = in_array('duration_unit', $planCols, true);

    $payCols = admin_table_exists($pdo, 'payments') ? admin_table_columns($pdo, 'payments') : [];
    $hasPayUserId = in_array('user_id', $payCols, true);
    $hasPayDeviceId = in_array('device_id', $payCols, true);

    // Handle Plan Assignment (POST)
    if (($_SERVER['REQUEST_METHOD'] ?? '') === 'POST' && isset($_POST['assign_plan'])) {
        if (!admin_verify_csrf_token($_POST['csrf_token'] ?? null)) {
            admin_set_flash('error', 'Invalid request token. Please refresh and try again.');
        } else {
            $assignDeviceId = (string) ($_POST['device_id'] ?? '');
            $planId = (int) ($_POST['plan_id'] ?? 0);

            if ($assignDeviceId === '' || $planId <= 0) {
                admin_set_flash('error', 'Invalid device or plan selection.');
            } else {
                try {
                    // Find device and its owner
                    $dStmt = $pdo->prepare('SELECT id, user_id, name, system_id, device_uid FROM `devices` WHERE `id` = ? OR `system_id` = ? OR `device_uid` = ? LIMIT 1');
                    $dStmt->execute([$assignDeviceId, $assignDeviceId, $assignDeviceId]);
                    $dev = $dStmt->fetch(PDO::FETCH_ASSOC);

                    if (!$dev) {
                        throw new RuntimeException('Device not found.');
                    }

                    $userId = !empty($dev['user_id']) ? (int) $dev['user_id'] : null;

                    // Verify plan
                    $planStmt = $pdo->prepare('SELECT * FROM `plans` WHERE `id` = ? AND `status` = "active" LIMIT 1');
                    $planStmt->execute([$planId]);
                    $plan = $planStmt->fetch(PDO::FETCH_ASSOC);

                    if (!$plan) {
                        throw new RuntimeException('The selected plan is not active or does not exist.');
                    }

                    $duration = (int) ($plan['duration_days'] ?? 30);
                    $durationUnit = (string) ($plan['duration_unit'] ?? 'days');
                    $price = (float) ($plan['price'] ?? 0);
                    $startDate = date('Y-m-d H:i:s');
                    $expiryDate = ($durationUnit === 'unlimited' || $duration <= 0) ? null : date('Y-m-d H:i:s', strtotime("+{$duration} days"));
                    $planSnapshot = json_encode($plan);
                    $adminId = (int)($_SESSION['admin_user_id'] ?? 1);

                    $targetDeviceKey = !empty($dev['system_id']) ? $dev['system_id'] : (!empty($dev['device_uid']) ? $dev['device_uid'] : (string)$dev['id']);

                    $pdo->beginTransaction();

                    // Deactivate currently active subscriptions for this user or device
                    if ($hasSubUserId && $userId) {
                        $deactStmt = $pdo->prepare('UPDATE `subscriptions` SET `status` = "expired" WHERE `user_id` = ? AND LOWER(`status`) = "active"');
                        $deactStmt->execute([$userId]);
                    } elseif ($hasSubDeviceId) {
                        $deactStmt = $pdo->prepare('UPDATE `subscriptions` SET `status` = "expired" WHERE `device_id` = ? AND LOWER(`status`) = "active"');
                        $deactStmt->execute([$targetDeviceKey]);
                    }

                    // Insert into subscriptions using available columns
                    $subData = [
                        'plan_id' => $planId,
                        'status' => 'active',
                    ];
                    if ($hasSubUserId && $userId) $subData['user_id'] = $userId;
                    if ($hasSubDeviceId) $subData['device_id'] = $targetDeviceKey;
                    if (in_array('plan_name', $subCols, true)) $subData['plan_name'] = $plan['name'];
                    if (in_array('amount', $subCols, true)) $subData['amount'] = $price;
                    if (in_array('payment_status', $subCols, true)) $subData['payment_status'] = 'completed';
                    if (in_array('starts_at', $subCols, true)) $subData['starts_at'] = $startDate;
                    if (in_array('start_date', $subCols, true)) $subData['start_date'] = date('Y-m-d');
                    if (in_array('expires_at', $subCols, true)) $subData['expires_at'] = $expiryDate;
                    if (in_array('expiry_date', $subCols, true)) $subData['expiry_date'] = $expiryDate ? date('Y-m-d', strtotime($expiryDate)) : null;
                    if (in_array('plan_snapshot', $subCols, true)) $subData['plan_snapshot'] = $planSnapshot;
                    if (in_array('activation_source', $subCols, true)) $subData['activation_source'] = 'admin_portal';
                    if (in_array('activated_by', $subCols, true)) $subData['activated_by'] = $adminId;
                    if (in_array('activation_reason', $subCols, true)) $subData['activation_reason'] = "Assigned to device {$targetDeviceKey}";
                    if (in_array('created_at', $subCols, true)) $subData['created_at'] = $startDate;

                    $colList = implode(', ', array_map(fn($k) => "`$k`", array_keys($subData)));
                    $valList = implode(', ', array_fill(0, count($subData), '?'));
                    $subStmt = $pdo->prepare("INSERT INTO `subscriptions` ({$colList}) VALUES ({$valList})");
                    $subStmt->execute(array_values($subData));
                    $newSubId = (int) $pdo->lastInsertId();

                    // If payments table exists, record payment transaction adapting to schema
                    if (!empty($payCols)) {
                        $txnId = 'ADMIN-' . strtoupper(bin2hex(random_bytes(4)));
                        $payData = [
                            'subscription_id' => $newSubId,
                            'amount' => $price,
                            'status' => 'completed',
                        ];
                        if ($hasPayUserId && $userId) $payData['user_id'] = $userId;
                        if ($hasPayDeviceId) $payData['device_id'] = $targetDeviceKey;
                        if (in_array('transaction_id', $payCols, true)) $payData['transaction_id'] = $txnId;
                        if (in_array('payment_reference', $payCols, true)) $payData['payment_reference'] = $txnId;
                        if (in_array('payment_method', $payCols, true)) $payData['payment_method'] = 'admin_assigned';
                        if (in_array('created_at', $payCols, true)) $payData['created_at'] = $startDate;
                        if (in_array('paid_at', $payCols, true)) $payData['paid_at'] = $startDate;
                        if (in_array('plan_id', $payCols, true)) $payData['plan_id'] = $planId;
                        if (in_array('plan_snapshot', $payCols, true)) $payData['plan_snapshot'] = $planSnapshot;

                        $payColsList = implode(', ', array_map(fn($k) => "`$k`", array_keys($payData)));
                        $payValsList = implode(', ', array_fill(0, count($payData), '?'));
                        $payStmt = $pdo->prepare("INSERT INTO `payments` ({$payColsList}) VALUES ({$payValsList})");
                        $payStmt->execute(array_values($payData));
                    }

                    $pdo->commit();

                    admin_log_action($pdo, 'assign_plan', "Assigned plan '{$plan['name']}' to device {$targetDeviceKey}" . ($userId ? " (User #{$userId})" : ''));
                    admin_set_flash('success', "Plan '{$plan['name']}' assigned successfully.");
                    header('Location: ' . admin_url('devices.php', ['view' => (string) $dev['id']]));
                    exit;
                } catch (Throwable $e) {
                    if ($pdo->inTransaction()) {
                        $pdo->rollBack();
                    }
                    admin_log_error($e, 'Plan assignment error');
                    admin_set_flash('error', 'Failed to assign plan: ' . $e->getMessage());
                }
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // DETAIL VIEW (view = DEVICE_ID or SYSTEM_ID)
    // ─────────────────────────────────────────────────────────────────────────────
    $selectedKey = trim((string) ($_GET['view'] ?? ''));
    if ($selectedKey !== '') {
        $devSql = "
            SELECT d.*, 
                   u.name AS owner_name, 
                   u.email AS owner_email,
                   u.username AS owner_username,
                   u.unique_user_id AS owner_unique_id
            FROM `devices` d
            LEFT JOIN `users` u ON d.user_id = u.id
            WHERE d.id = ? OR d.system_id = ? OR d.device_uid = ?
            LIMIT 1
        ";
        $selStmt = $pdo->prepare($devSql);
        $selStmt->execute([$selectedKey, $selectedKey, $selectedKey]);
        $device = $selStmt->fetch(PDO::FETCH_ASSOC);

        if (!$device) {
            $body = '<div class="section"><div class="section-header"><h3>Device Not Found</h3><div class="mini-actions"><a class="outline-button" href="' . htmlspecialchars(admin_url('devices.php'), ENT_QUOTES, 'UTF-8') . '">&larr; Back to Devices</a></div></div><div class="empty-state">The requested device could not be found.</div></div>';
            echo admin_render_page('Device Details', 'devices', $body);
            exit;
        }

        $devId = (int) $device['id'];
        $sysId = htmlspecialchars((string) ($device['system_id'] ?? ($device['device_uid'] ?? 'ID#' . $devId)), ENT_QUOTES, 'UTF-8');
        $devName = htmlspecialchars((string) ($device['name'] ?? 'Unnamed device'), ENT_QUOTES, 'UTF-8');
        $machineId = htmlspecialchars((string) ($device['machine_identifier'] ?? '—'), ENT_QUOTES, 'UTF-8');
        $os = htmlspecialchars((string) ($device['os_type'] ?? 'Windows'), ENT_QUOTES, 'UTF-8');
        $ip = htmlspecialchars((string) ($device['ip_address'] ?? '—'), ENT_QUOTES, 'UTF-8');
        $isOnline = !empty($device['is_online']);
        $devStatus = $isOnline ? '<span class="status-badge status-active">Online</span>' : '<span class="status-badge status-inactive">Offline</span>';
        $lastSeen = admin_time_ago($device['last_seen_at'] ?? null);
        $created = admin_utc_date($device['created_at'] ?? null);

        // Owner details
        $hasOwner = !empty($device['user_id']);
        $ownerName = htmlspecialchars((string) ($device['owner_name'] ?? 'Unassigned'), ENT_QUOTES, 'UTF-8');
        $ownerEmail = htmlspecialchars((string) ($device['owner_email'] ?? '—'), ENT_QUOTES, 'UTF-8');
        $ownerLink = $hasOwner ? '<a href="' . htmlspecialchars(admin_url('users.php', ['view' => (int) $device['user_id']]), ENT_QUOTES, 'UTF-8') . '" style="color:var(--accent); font-weight:600;">' . $ownerName . ' (#' . (int) $device['user_id'] . ')</a>' : 'Unassigned';

        // Subscriptions associated with this device or owner
        $ownerSubs = [];
        if (!empty($subCols)) {
            $unitSelect = $hasPlanUnit ? ', p.duration_unit' : ', "days" AS duration_unit';
            $priceSelect = in_array('price', $planCols, true) ? ', p.price' : '';

            if ($hasSubUserId && $hasOwner) {
                $sStmt = $pdo->prepare("
                    SELECT s.*, p.name AS plan_name {$priceSelect} {$unitSelect}
                    FROM `subscriptions` s
                    LEFT JOIN `plans` p ON s.plan_id = p.id
                    WHERE s.user_id = ?
                    ORDER BY s.id DESC
                ");
                $sStmt->execute([(int) $device['user_id']]);
                $ownerSubs = $sStmt->fetchAll(PDO::FETCH_ASSOC);
            } elseif ($hasSubDeviceId) {
                $devKeys = array_values(array_unique(array_filter([
                    (string)$device['id'],
                    (string)($device['system_id'] ?? ''),
                    (string)($device['device_uid'] ?? '')
                ])));
                $placeholders = implode(',', array_fill(0, count($devKeys), '?'));
                $sStmt = $pdo->prepare("
                    SELECT s.*, p.name AS plan_name {$priceSelect} {$unitSelect}
                    FROM `subscriptions` s
                    LEFT JOIN `plans` p ON s.plan_id = p.id
                    WHERE s.device_id IN ({$placeholders})
                    ORDER BY s.id DESC
                ");
                $sStmt->execute($devKeys);
                $ownerSubs = $sStmt->fetchAll(PDO::FETCH_ASSOC);
            }
        }

        $subsMarkup = '';
        if (!empty($ownerSubs)) {
            $subsMarkup = '<table><thead><tr><th>Sub ID</th><th>Plan</th><th>Price</th><th>Started</th><th>Expires</th><th>Status</th></tr></thead><tbody>';
            foreach ($ownerSubs as $s) {
                $subId = (int) $s['id'];
                $pName = htmlspecialchars((string) ($s['plan_name'] ?? 'Plan #' . ($s['plan_id'] ?? '')), ENT_QUOTES, 'UTF-8');
                $price = '₹' . number_format((float) ($s['amount'] ?? $s['price'] ?? 0), 2);
                $startVal = $s[$subStartCol] ?? ($s['starts_at'] ?? ($s['start_date'] ?? null));
                $starts = !empty($startVal) ? htmlspecialchars(admin_utc_date((string)$startVal), ENT_QUOTES, 'UTF-8') : 'N/A';
                
                $expVal = $s[$subExpCol] ?? ($s['expires_at'] ?? ($s['expiry_date'] ?? null));
                if (empty($expVal) || ($s['duration_unit'] ?? '') === 'unlimited') {
                    $expires = '<span style="color:#16a34a; font-weight:600;">Unlimited</span>';
                } else {
                    $expires = htmlspecialchars(admin_utc_date((string)$expVal), ENT_QUOTES, 'UTF-8');
                }
                $stat = strtoupper((string) ($s['status'] ?? 'ACTIVE'));
                $statClass = strtolower($stat);
                $subsMarkup .= "<tr><td>#{$subId}</td><td><strong>{$pName}</strong></td><td>{$price}</td><td>{$starts}</td><td>{$expires}</td><td><span class=\"status-badge status-{$statClass}\">{$stat}</span></td></tr>";
            }
            $subsMarkup .= '</tbody></table>';
        } else {
            $subsMarkup = '<div class="empty-state">No subscription plan assigned to this device or owner.</div>';
        }

        // Device Remote Sessions (host_device_id = device.id)
        $deviceSessions = [];
        if (admin_table_exists($pdo, 'sessions')) {
            $sessStmt = $pdo->prepare("SELECT * FROM `sessions` WHERE `host_device_id` = ? ORDER BY id DESC LIMIT 15");
            $sessStmt->execute([$devId]);
            $deviceSessions = $sessStmt->fetchAll(PDO::FETCH_ASSOC);
        }

        $sessionsMarkup = '';
        if (!empty($deviceSessions)) {
            $sessionsMarkup = '<table><thead><tr><th>Session Key</th><th>Status</th><th>Bandwidth</th><th>Duration</th><th>Started At</th></tr></thead><tbody>';
            foreach ($deviceSessions as $sess) {
                $sKey = htmlspecialchars((string) ($sess['session_key'] ?? 'N/A'), ENT_QUOTES, 'UTF-8');
                $st = strtoupper((string) ($sess['status'] ?? 'COMPLETED'));
                $stClass = strtolower($st);
                $bw = admin_format_bytes((float) ($sess['bandwidth_used_mb'] ?? 0) * 1024 * 1024);
                $dur = admin_format_duration($sess['started_at'] ?? null, $sess['ended_at'] ?? null);
                $started = admin_time_ago($sess['started_at'] ?? null);
                $sessionsMarkup .= "<tr><td><code>{$sKey}</code></td><td><span class=\"status-badge status-{$stClass}\">{$st}</span></td><td>{$bw}</td><td>{$dur}</td><td><small>{$started}</small></td></tr>";
            }
            $sessionsMarkup .= '</tbody></table>';
        } else {
            $sessionsMarkup = '<div class="empty-state">No remote sessions recorded on this device yet.</div>';
        }

        // Connection Requests involving this device (requester or target)
        $connRequests = [];
        if (admin_table_exists($pdo, 'connection_requests')) {
            $crStmt = $pdo->prepare("SELECT * FROM `connection_requests` WHERE `requester_system_id` = ? OR `target_system_id` = ? ORDER BY id DESC LIMIT 15");
            $crStmt->execute([$sysId, $sysId]);
            $connRequests = $crStmt->fetchAll(PDO::FETCH_ASSOC);
        }

        $connReqMarkup = '';
        if (!empty($connRequests)) {
            $connReqMarkup = '<table><thead><tr><th>Token</th><th>Role</th><th>Peer System ID</th><th>Status</th><th>Created</th><th>Expires</th></tr></thead><tbody>';
            foreach ($connRequests as $cr) {
                $token = htmlspecialchars((string) ($cr['request_token'] ?? '—'), ENT_QUOTES, 'UTF-8');
                $isRequester = ($cr['requester_system_id'] === $sysId);
                $role = $isRequester ? '<span style="color:var(--accent); font-weight:600;">Outgoing (Requester)</span>' : '<span style="color:#1d4ed8; font-weight:600;">Incoming (Target)</span>';
                $peer = htmlspecialchars((string) ($isRequester ? $cr['target_system_id'] : $cr['requester_system_id']), ENT_QUOTES, 'UTF-8');
                $st = strtoupper((string) ($cr['status'] ?? 'PENDING'));
                $stClass = strtolower($st);
                $crCreated = admin_time_ago($cr['created_at'] ?? null);
                $crExp = !empty($cr['expires_at']) ? htmlspecialchars((string) $cr['expires_at'], ENT_QUOTES, 'UTF-8') : '—';
                $connReqMarkup .= "<tr><td><code>{$token}</code></td><td>{$role}</td><td><code>{$peer}</code></td><td><span class=\"status-badge status-{$stClass}\">{$st}</span></td><td><small>{$crCreated}</small></td><td><small>{$crExp}</small></td></tr>";
            }
            $connReqMarkup .= '</tbody></table>';
        } else {
            $connReqMarkup = '<div class="empty-state">No connection requests logged for this device.</div>';
        }

        // Plan options for assignment
        $planOptions = '<option value="">-- Choose Subscription Plan --</option>';
        if ($planTable) {
            $plansList = $pdo->query('SELECT * FROM `plans` WHERE `status` = "active" ORDER BY price ASC')->fetchAll(PDO::FETCH_ASSOC);
            foreach ($plansList as $pl) {
                $pPrice = '₹' . number_format((float) ($pl['price'] ?? 0), 2);
                $unit = $pl['duration_unit'] ?? 'days';
                $pDur = ($unit === 'unlimited') ? 'Unlimited' : (($pl['duration_days'] ?? 30) . ' days');
                $planOptions .= '<option value="' . (int) $pl['id'] . '">' . htmlspecialchars($pl['name'], ENT_QUOTES, 'UTF-8') . " ({$pPrice} &bull; {$pDur})</option>";
            }
        }

        $csrfToken = htmlspecialchars(admin_csrf_token(), ENT_QUOTES, 'UTF-8');
        $devicesBackUrl = htmlspecialchars(admin_url('devices.php'), ENT_QUOTES, 'UTF-8');

        $body = <<<HTML
<div class="section">
    <div class="section-header">
        <h3>Device: {$devName}</h3>
        <div class="mini-actions"><a class="outline-button" href="{$devicesBackUrl}">&larr; Back to Devices</a></div>
    </div>
    <div class="detail-panels">
        <div class="detail-panel">
            <h4>Hardware & Network Details</h4>
            <div class="detail-row"><span>System ID</span><strong><code>{$sysId}</code></strong></div>
            <div class="detail-row"><span>Device Name</span><strong>{$devName}</strong></div>
            <div class="detail-row"><span>Operating System</span><strong>{$os}</strong></div>
            <div class="detail-row"><span>IP Address</span><strong><code>{$ip}</code></strong></div>
            <div class="detail-row"><span>Machine Identifier</span><strong><small><code>{$machineId}</code></small></strong></div>
            <div class="detail-row"><span>Live Status</span><strong>{$devStatus}</strong></div>
            <div class="detail-row"><span>Last Seen</span><strong>{$lastSeen}</strong></div>
            <div class="detail-row"><span>Registered Date</span><strong>{$created}</strong></div>
        </div>
        <div class="detail-panel">
            <h4>Ownership & Plan Assignment</h4>
            <div class="detail-row"><span>Device Owner</span><strong>{$ownerLink}</strong></div>
            <div class="detail-row"><span>Owner Email</span><strong>{$ownerEmail}</strong></div>
            <div style="margin-top:20px; padding-top:16px; border-top:1px solid var(--border);">
                <h5 style="margin-bottom:10px; font-size:0.85rem; font-weight:700;">Assign or Renew Plan</h5>
                <form method="POST" action="{$devicesBackUrl}?view={$devId}" style="display:flex; flex-direction:column; gap:10px;">
                    <input type="hidden" name="csrf_token" value="{$csrfToken}">
                    <input type="hidden" name="assign_plan" value="1">
                    <input type="hidden" name="device_id" value="{$devId}">
                    <select name="plan_id" class="filter-select" required style="width:100%;">
                        {$planOptions}
                    </select>
                    <button type="submit" class="button" style="width:100%;">Assign Plan to Device</button>
                </form>
            </div>
        </div>
    </div>
</div>

<div class="section">
    <div class="section-header">
        <h3>Subscriptions</h3>
    </div>
    <div style="overflow:auto;">
        {$subsMarkup}
    </div>
</div>

<div class="section">
    <div class="section-header">
        <h3>Recent Remote Sessions ({$devName})</h3>
    </div>
    <div style="overflow:auto;">
        {$sessionsMarkup}
    </div>
</div>

<div class="section">
    <div class="section-header">
        <h3>Connection Requests ({$devName})</h3>
    </div>
    <div style="overflow:auto;">
        {$connReqMarkup}
    </div>
</div>
HTML;

        echo admin_render_page('Device Details', 'devices', $body);
        exit;
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // LIST VIEW WITH SERVER-SIDE PAGINATION, SEARCH, STATUS FILTERS & KPI CARDS
    // ─────────────────────────────────────────────────────────────────────────────

    $search = trim((string) ($_GET['q'] ?? ''));
    $statusFilter = strtolower(trim((string) ($_GET['status'] ?? 'all')));
    $statusFilter = in_array($statusFilter, ['all', 'online', 'offline'], true) ? $statusFilter : 'all';

    $page = max(1, (int) ($_GET['page'] ?? 1));
    $perPage = 15;
    $offset = ($page - 1) * $perPage;

    $where = [];
    $params = [];

    if ($search !== '') {
        $where[] = '(d.name LIKE ? OR d.system_id LIKE ? OR d.device_uid LIKE ? OR d.ip_address LIKE ? OR u.name LIKE ? OR u.email LIKE ?)';
        $params[] = '%' . $search . '%';
        $params[] = '%' . $search . '%';
        $params[] = '%' . $search . '%';
        $params[] = '%' . $search . '%';
        $params[] = '%' . $search . '%';
        $params[] = '%' . $search . '%';
    }

    if ($statusFilter !== 'all') {
        $where[] = 'd.is_online = ?';
        $params[] = ($statusFilter === 'online') ? 1 : 0;
    }

    $whereSql = $where ? ' WHERE ' . implode(' AND ', $where) : '';

    // KPI Fleet Counters
    $kpiTotal = (int) $pdo->query('SELECT COUNT(*) FROM `devices`')->fetchColumn();
    $kpiOnline = (int) $pdo->query('SELECT COUNT(*) FROM `devices` WHERE `is_online` = 1')->fetchColumn();
    $kpiOffline = max(0, $kpiTotal - $kpiOnline);
    $kpiAssigned = (int) $pdo->query('SELECT COUNT(*) FROM `devices` WHERE `user_id` IS NOT NULL')->fetchColumn();

    // Total count matching current filters
    $countStmt = $pdo->prepare("
        SELECT COUNT(DISTINCT d.id) 
        FROM `devices` d
        LEFT JOIN `users` u ON d.user_id = u.id
        {$whereSql}
    ");
    $countStmt->execute($params);
    $totalMatchingDevices = (int) $countStmt->fetchColumn();

    // Build active plan subquery dynamically based on schema
    $activePlanSubquery = "'No Subscription' AS active_plan_name";
    if ($subscriptionTable && $planTable) {
        if ($hasSubUserId) {
            $activePlanSubquery = "(SELECT p.name FROM `subscriptions` s JOIN `plans` p ON s.plan_id = p.id WHERE s.user_id = d.user_id AND LOWER(s.status) = 'active' ORDER BY s.id DESC LIMIT 1) AS active_plan_name";
        } elseif ($hasSubDeviceId) {
            $activePlanSubquery = "(SELECT p.name FROM `subscriptions` s JOIN `plans` p ON s.plan_id = p.id WHERE (s.device_id = d.system_id OR s.device_id = d.device_uid OR s.device_id = CAST(d.id AS CHAR)) AND LOWER(s.status) = 'active' ORDER BY s.id DESC LIMIT 1) AS active_plan_name";
        }
    }

    // Fetch paginated devices
    $sql = "
        SELECT d.*, 
               u.name AS owner_name, 
               u.email AS owner_email,
               {$activePlanSubquery}
        FROM `devices` d
        LEFT JOIN `users` u ON d.user_id = u.id
        {$whereSql}
        ORDER BY d.id DESC
        LIMIT {$perPage} OFFSET {$offset}
    ";

    $stmt = $pdo->prepare($sql);
    $stmt->execute($params);
    $deviceRows = $stmt->fetchAll(PDO::FETCH_ASSOC);

    $deviceRowsHtml = '';
    if (!empty($deviceRows)) {
        foreach ($deviceRows as $dev) {
            $id = (int) $dev['id'];
            $sysId = htmlspecialchars((string) ($dev['system_id'] ?? ($dev['device_uid'] ?? 'ID#' . $id)), ENT_QUOTES, 'UTF-8');
            $name = htmlspecialchars((string) ($dev['name'] ?? 'Unnamed device'), ENT_QUOTES, 'UTF-8');
            $owner = htmlspecialchars((string) ($dev['owner_name'] ?? ($dev['owner_email'] ?? 'Unassigned')), ENT_QUOTES, 'UTF-8');
            $isOnline = !empty($dev['is_online']);
            $statusBadge = $isOnline ? '<span class="status-badge status-active">Online</span>' : '<span class="status-badge status-inactive">Offline</span>';
            $plan = htmlspecialchars((string) ($dev['active_plan_name'] ?? 'No Subscription'), ENT_QUOTES, 'UTF-8');
            $os = htmlspecialchars((string) ($dev['os_type'] ?? 'Windows'), ENT_QUOTES, 'UTF-8');
            $ip = htmlspecialchars((string) ($dev['ip_address'] ?? '—'), ENT_QUOTES, 'UTF-8');
            $lastSeen = admin_time_ago($dev['last_seen_at'] ?? null);
            $viewUrl = htmlspecialchars(admin_url('devices.php', ['view' => $id]), ENT_QUOTES, 'UTF-8');

            $deviceRowsHtml .= "<tr>
                <td><code>{$sysId}</code></td>
                <td><strong>{$name}</strong></td>
                <td>{$owner}</td>
                <td>{$statusBadge}</td>
                <td>{$plan}</td>
                <td>{$os}</td>
                <td><small><code>{$ip}</code></small></td>
                <td><small>{$lastSeen}</small></td>
                <td><a class=\"outline-button\" href=\"{$viewUrl}\">View Details</a></td>
            </tr>";
        }
    } else {
        $deviceRowsHtml = '<tr><td colspan="9" class="empty-state">No matching remote devices found.</td></tr>';
    }

    $devicesUrl = htmlspecialchars(admin_url('devices.php'), ENT_QUOTES, 'UTF-8');
    $queryParams = array_filter([
        'q' => $search !== '' ? $search : null,
        'status' => $statusFilter !== 'all' ? $statusFilter : null,
    ]);

    $paginationHtml = admin_pagination($totalMatchingDevices, $perPage, $page, 'devices.php', $queryParams);

    $statusOptions = '';
    foreach (['all' => 'All Statuses', 'online' => 'Online', 'offline' => 'Offline'] as $v => $l) {
        $statusOptions .= '<option value="' . $v . '"' . ($statusFilter === $v ? ' selected' : '') . '>' . $l . '</option>';
    }

    $searchSafe = htmlspecialchars($search, ENT_QUOTES, 'UTF-8');

    $body = <<<HTML
<!-- Fleet Summary KPI Cards -->
<div class="card-grid">
    <div class="card">
        <span class="label">Total Fleet Devices</span>
        <p class="value">{$kpiTotal}</p>
        <div class="trend">Registered device endpoints</div>
    </div>
    <div class="card">
        <span class="label">Online Now</span>
        <p class="value" style="color:#16a34a;">{$kpiOnline}</p>
        <div class="trend">Actively connected / responsive</div>
    </div>
    <div class="card">
        <span class="label">Offline Devices</span>
        <p class="value" style="color:#6b7280;">{$kpiOffline}</p>
        <div class="trend">Currently disconnected</div>
    </div>
    <div class="card">
        <span class="label">Assigned to Owner</span>
        <p class="value">{$kpiAssigned}</p>
        <div class="trend">Associated with a user account</div>
    </div>
</div>

<!-- Search & Filter Bar -->
<div class="filter-bar">
    <form method="GET" action="{$devicesUrl}" style="display:flex; gap:10px; flex-wrap:wrap; align-items:center; width:100%;">
        <div class="filter-group">
            <input type="text" name="q" value="{$searchSafe}" placeholder="Search name, system ID, IP, or owner..." class="filter-input" style="min-width:280px;">
        </div>
        <div class="filter-group">
            <select name="status" class="filter-select">
                {$statusOptions}
            </select>
        </div>
        <button type="submit" class="filter-btn">Filter</button>
        <a href="{$devicesUrl}" class="filter-reset">Reset</a>
    </form>
</div>

<!-- Devices Table -->
<div class="section">
    <div class="section-header">
        <h3>Remote Devices</h3>
        <span class="kpi-pill">{$totalMatchingDevices} Devices Found</span>
    </div>
    <div style="overflow:auto;">
        <table>
            <thead>
                <tr>
                    <th>System ID</th>
                    <th>Device Name</th>
                    <th>Owner</th>
                    <th>Status</th>
                    <th>Active Plan</th>
                    <th>OS</th>
                    <th>IP Address</th>
                    <th>Last Seen</th>
                    <th>Actions</th>
                </tr>
            </thead>
            <tbody>
                {$deviceRowsHtml}
            </tbody>
        </table>
    </div>
    {$paginationHtml}
</div>
HTML;

    echo admin_render_page('Remote Devices', 'devices', $body);

} catch (Throwable $e) {
    admin_log_error($e, 'devices.php execution');
    echo admin_render_page('Remote Devices', 'devices', admin_safe_error_html('Unexpected Error', 'An error occurred while loading the Devices page.', $e));
}
