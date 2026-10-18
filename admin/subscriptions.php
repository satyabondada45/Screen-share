<?php
require_once __DIR__ . '/includes/db.php';
require_once __DIR__ . '/includes/auth.php';
require_once __DIR__ . '/includes/layout.php';

$subServiceFile = admin_service_file('SubscriptionService.php');
if ($subServiceFile && is_file($subServiceFile)) {
    require_once $subServiceFile;
}

admin_require_auth();

if (isset($_GET['action']) && $_GET['action'] === 'search_customers') {
    require __DIR__ . '/api_customers.php';
    exit;
}

try {
    $pdo = admin_db();
} catch (Throwable $e) {
    admin_log_error($e, 'admin/subscriptions.php db connect');
    echo admin_render_page('System Error', 'error', admin_safe_error_html('Database Connection Error', 'Unable to connect to the database.', $e));
    exit;
}

$subscriptionTable = admin_table_exists($pdo, 'subscriptions') ? 'subscriptions' : null;
if (!$subscriptionTable) {
    echo admin_render_page('Subscriptions', 'subscriptions', admin_safe_error_html('Table Unavailable', 'The subscriptions database table is unavailable.'));
    exit;
}

try {
    $subCols = admin_table_columns($pdo, 'subscriptions');
    $hasSubUserId = in_array('user_id', $subCols, true);
    $hasSubDeviceId = in_array('device_id', $subCols, true);
    $subExpCol = in_array('expires_at', $subCols, true) ? 'expires_at' : (in_array('expiry_date', $subCols, true) ? 'expiry_date' : null);
    $subStartCol = in_array('starts_at', $subCols, true) ? 'starts_at' : (in_array('start_date', $subCols, true) ? 'start_date' : null);

    $planCols = admin_table_exists($pdo, 'plans') ? admin_table_columns($pdo, 'plans') : [];
    $hasPlanUnit = in_array('duration_unit', $planCols, true);
    $hasPlanValue = in_array('duration_value', $planCols, true);
    $hasPlanSlug = in_array('slug', $planCols, true);

    if (($_SERVER['REQUEST_METHOD'] ?? '') === 'POST' && isset($_POST['manual_activation'])) {
        if (!admin_verify_csrf_token($_POST['csrf_token'] ?? null)) {
            admin_set_flash('error', 'Invalid security token. Please reload and try again.');
        } else {
            $userId = filter_var($_POST['user_id'] ?? null, FILTER_VALIDATE_INT, ['options' => ['min_range' => 1]]);
            $planId = filter_var($_POST['plan_id'] ?? null, FILTER_VALIDATE_INT, ['options' => ['min_range' => 1]]);
            try {
                if ($userId === false || $planId === false) {
                    throw new InvalidArgumentException('Choose a valid user and paid plan.');
                }

                if (class_exists('SubscriptionService') && method_exists('SubscriptionService', 'activateManualSubscription')) {
                    $activation = SubscriptionService::activateManualSubscription(
                        (int)$userId,
                        (int)$planId,
                        (int)($_SESSION['admin_user_id'] ?? 1),
                        (string)($_POST['reason'] ?? '')
                    );
                    $expDisplay = !empty($activation['expires_at']) ? admin_utc_date($activation['expires_at']) . ' UTC' : 'No expiry (Ongoing)';
                    admin_set_flash(
                        'success',
                        $activation['plan_name'] . ' activated successfully through ' . $expDisplay . '.'
                    );
                } else {
                    // Robust schema-adaptive direct manual activation
                    $uStmt = $pdo->prepare('SELECT id, name, email FROM `users` WHERE id = ? LIMIT 1');
                    $uStmt->execute([$userId]);
                    $targetUser = $uStmt->fetch(PDO::FETCH_ASSOC);
                    if (!$targetUser) {
                        throw new InvalidArgumentException('User account #' . $userId . ' does not exist.');
                    }

                    $pStmt = $pdo->prepare('SELECT * FROM `plans` WHERE id = ? AND status = "active" LIMIT 1');
                    $pStmt->execute([$planId]);
                    $targetPlan = $pStmt->fetch(PDO::FETCH_ASSOC);
                    if (!$targetPlan) {
                        throw new InvalidArgumentException('Selected plan is inactive or does not exist.');
                    }

                    $durDays = (int)($targetPlan['duration_days'] ?? 30);
                    $nowTs = time();
                    $startsAt = date('Y-m-d H:i:s', $nowTs);
                    $expiresAt = ($durDays <= 0) ? null : date('Y-m-d H:i:s', $nowTs + ($durDays * 86400));
                    $planSnapshot = json_encode($targetPlan);
                    $adminId = (int)($_SESSION['admin_user_id'] ?? 1);
                    $reason = trim((string)($_POST['reason'] ?? 'Manual admin activation'));

                    if ($hasSubUserId) {
                        // Deactivate current active subscriptions for this user
                        $pdo->prepare('UPDATE `subscriptions` SET `status` = "expired" WHERE `user_id` = ? AND LOWER(`status`) = "active"')->execute([$userId]);
                        
                        $ins = [
                            'user_id' => $userId,
                            'plan_id' => $planId,
                            'status' => 'active',
                        ];
                        if (in_array('starts_at', $subCols, true)) $ins['starts_at'] = $startsAt;
                        if (in_array('start_date', $subCols, true)) $ins['start_date'] = date('Y-m-d', $nowTs);
                        if (in_array('expires_at', $subCols, true)) $ins['expires_at'] = $expiresAt;
                        if (in_array('expiry_date', $subCols, true)) $ins['expiry_date'] = $expiresAt ? date('Y-m-d', strtotime($expiresAt)) : null;
                        if (in_array('plan_snapshot', $subCols, true)) $ins['plan_snapshot'] = $planSnapshot;
                        if (in_array('activation_source', $subCols, true)) $ins['activation_source'] = 'admin_portal';
                        if (in_array('activated_by', $subCols, true)) $ins['activated_by'] = $adminId;
                        if (in_array('activation_reason', $subCols, true)) $ins['activation_reason'] = $reason;
                        if (in_array('created_at', $subCols, true)) $ins['created_at'] = $startsAt;

                        $colSql = implode(', ', array_map(fn($k) => "`$k`", array_keys($ins)));
                        $valSql = implode(', ', array_fill(0, count($ins), '?'));
                        $insStmt = $pdo->prepare("INSERT INTO `subscriptions` ({$colSql}) VALUES ({$valSql})");
                        $insStmt->execute(array_values($ins));
                    } elseif ($hasSubDeviceId && admin_table_exists($pdo, 'devices')) {
                        // Find user's devices
                        $dStmt = $pdo->prepare('SELECT id, system_id, device_uid FROM `devices` WHERE user_id = ?');
                        $dStmt->execute([$userId]);
                        $userDevs = $dStmt->fetchAll(PDO::FETCH_ASSOC);
                        $targetDeviceId = !empty($userDevs[0]['system_id']) ? $userDevs[0]['system_id'] : (!empty($userDevs[0]['device_uid']) ? $userDevs[0]['device_uid'] : (!empty($userDevs[0]['id']) ? (string)$userDevs[0]['id'] : 'user_' . $userId));

                        $ins = [
                            'device_id' => $targetDeviceId,
                            'plan_id' => $planId,
                            'status' => 'active',
                        ];
                        if (in_array('plan_name', $subCols, true)) $ins['plan_name'] = $targetPlan['name'];
                        if (in_array('amount', $subCols, true)) $ins['amount'] = (float)($targetPlan['price'] ?? 0);
                        if (in_array('payment_status', $subCols, true)) $ins['payment_status'] = 'completed';
                        if (in_array('start_date', $subCols, true)) $ins['start_date'] = date('Y-m-d', $nowTs);
                        if (in_array('expiry_date', $subCols, true)) $ins['expiry_date'] = $expiresAt ? date('Y-m-d', strtotime($expiresAt)) : null;
                        if (in_array('plan_snapshot', $subCols, true)) $ins['plan_snapshot'] = $planSnapshot;
                        if (in_array('activation_source', $subCols, true)) $ins['activation_source'] = 'admin_portal';
                        if (in_array('activated_by', $subCols, true)) $ins['activated_by'] = $adminId;
                        if (in_array('activation_reason', $subCols, true)) $ins['activation_reason'] = $reason;
                        if (in_array('created_at', $subCols, true)) $ins['created_at'] = $startsAt;

                        $colSql = implode(', ', array_map(fn($k) => "`$k`", array_keys($ins)));
                        $valSql = implode(', ', array_fill(0, count($ins), '?'));
                        $insStmt = $pdo->prepare("INSERT INTO `subscriptions` ({$colSql}) VALUES ({$valSql})");
                        $insStmt->execute(array_values($ins));
                    }

                    $expDisplay = $expiresAt ? admin_utc_date($expiresAt) : 'Unlimited';
                    admin_log_action($pdo, 'manual_activation', "Activated plan '{$targetPlan['name']}' for User #{$userId}");
                    admin_set_flash('success', "{$targetPlan['name']} successfully activated through {$expDisplay} UTC.");
                }
            } catch (InvalidArgumentException $error) {
                admin_set_flash('error', $error->getMessage());
            } catch (Throwable $error) {
                admin_log_error($error, 'Manual subscription activation');
                admin_set_flash('error', 'Unable to activate the selected paid plan: ' . $error->getMessage());
            }
        }
        header('Location: ' . admin_url('subscriptions.php'));
        exit();
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // SEARCH, FILTER, SORTING & PAGINATION
    // ─────────────────────────────────────────────────────────────────────────────

    $search = trim((string) ($_GET['q'] ?? ''));
    $status = strtolower(trim((string) ($_GET['status'] ?? 'all')));
    $status = in_array($status, ['all', 'active', 'expired', 'cancelled', 'pending'], true) ? $status : 'all';

    // Sorting Allowlist
    $sortColStarts = $subStartCol ? "s.{$subStartCol}" : 's.id';
    $sortColExpires = $subExpCol ? "s.{$subExpCol}" : 's.id';
    $sortColPrice = in_array('price', $planCols, true) ? 'p.price' : (in_array('amount', $subCols, true) ? 's.amount' : 's.id');

    $allowedSorts = [
        'id' => 's.id',
        'starts_at' => $sortColStarts,
        'expires_at' => $sortColExpires,
        'status' => 's.status',
        'price' => $sortColPrice,
    ];
    $sort = (string) ($_GET['sort'] ?? 'id');
    $sortColumn = $allowedSorts[$sort] ?? 's.id';
    $dir = strtolower((string) ($_GET['dir'] ?? 'desc')) === 'asc' ? 'ASC' : 'DESC';

    $page = max(1, (int) ($_GET['page'] ?? 1));
    $perPage = 20;
    $offset = ($page - 1) * $perPage;

    // Join configuration based on available columns
    $userJoinSql = '';
    if ($hasSubUserId) {
        $userJoinSql = 'LEFT JOIN `users` u ON s.user_id = u.id';
    } elseif ($hasSubDeviceId && admin_table_exists($pdo, 'devices')) {
        $userJoinSql = 'LEFT JOIN `devices` d ON (s.device_id = d.system_id OR s.device_id = d.device_uid OR s.device_id = CAST(d.id AS CHAR))
                        LEFT JOIN `users` u ON d.user_id = u.id';
    }

    $where = [];
    $params = [];

    if ($search !== '') {
        $searchFields = ['p.name'];
        if (admin_table_exists($pdo, 'users')) {
            $userCols = admin_table_columns($pdo, 'users');
            if (in_array('name', $userCols, true)) $searchFields[] = 'u.name';
            if (in_array('email', $userCols, true)) $searchFields[] = 'u.email';
            if (in_array('username', $userCols, true)) $searchFields[] = 'u.username';
            if (in_array('unique_user_id', $userCols, true)) $searchFields[] = 'u.unique_user_id';
        }
        if ($hasSubDeviceId) {
            $searchFields[] = 's.device_id';
        }

        $where[] = '(' . implode(' OR ', array_map(fn($f) => "$f LIKE ?", $searchFields)) . ')';
        foreach ($searchFields as $unused) {
            $params[] = '%' . $search . '%';
        }
    }

    if ($status !== 'all') {
        $where[] = 'LOWER(s.status) = ?';
        $params[] = $status;
    }

    $whereSql = $where ? ' WHERE ' . implode(' AND ', $where) : '';

    // 1. KPI Metric Summary Counters
    $kpiTotal = (int) $pdo->query('SELECT COUNT(*) FROM `subscriptions`')->fetchColumn();
    $kpiActive = (int) $pdo->query('SELECT COUNT(*) FROM `subscriptions` WHERE LOWER(status) = "active"')->fetchColumn();
    
    $kpiExpired = 0;
    $kpiExpiringSoon = 0;
    if ($subExpCol !== null) {
        $kpiExpired = (int) $pdo->query("SELECT COUNT(*) FROM `subscriptions` WHERE `{$subExpCol}` IS NOT NULL AND `{$subExpCol}` < NOW()")->fetchColumn();
        $kpiExpiringSoon = (int) $pdo->query("SELECT COUNT(*) FROM `subscriptions` WHERE `{$subExpCol}` IS NOT NULL AND `{$subExpCol}` BETWEEN NOW() AND DATE_ADD(NOW(), INTERVAL 7 DAY)")->fetchColumn();
    }

    // 2. Count Matching Records
    $countStmt = $pdo->prepare("
        SELECT COUNT(DISTINCT s.id) 
        FROM `subscriptions` s
        {$userJoinSql}
        LEFT JOIN `plans` p ON s.plan_id = p.id
        {$whereSql}
    ");
    $countStmt->execute($params);
    $totalMatching = (int) $countStmt->fetchColumn();

    // 3. Fetch Paginated Records
    $planUnitField = $hasPlanUnit ? 'p.duration_unit' : '"days" AS duration_unit';
    $planPriceField = in_array('price', $planCols, true) ? 'p.price AS plan_price' : 's.amount AS plan_price';
    $activatorJoin = '';
    $activatorSelect = '"" AS activated_by_name,';
    if (in_array('activated_by', $subCols, true)) {
        if (admin_table_exists($pdo, 'admin_users')) {
            $activatorJoin = 'LEFT JOIN `admin_users` activator ON s.activated_by = activator.id';
            $activatorSelect = 'activator.name AS activated_by_name,';
        } elseif (admin_table_exists($pdo, 'users')) {
            $activatorJoin = 'LEFT JOIN `users` activator ON s.activated_by = activator.id';
            $activatorSelect = 'activator.name AS activated_by_name,';
        }
    }

    $sql = "
        SELECT s.*, 
               u.id AS user_pk,
               u.name AS user_name, 
               u.email AS user_email,
               u.username AS user_username,
               {$activatorSelect}
               p.name AS plan_name, 
               {$planPriceField},
               {$planUnitField}
        FROM `subscriptions` s
        {$userJoinSql}
        {$activatorJoin}
        LEFT JOIN `plans` p ON s.plan_id = p.id
        {$whereSql}
        GROUP BY s.id
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
            $userId = (int) ($row['user_pk'] ?? ($row['user_id'] ?? 0));
            $userName = htmlspecialchars((string) ($row['user_name'] ?? ($row['user_username'] ?? ($userId > 0 ? 'User #' . $userId : ($row['device_id'] ?? 'Unassigned')))), ENT_QUOTES, 'UTF-8');
            $userEmail = htmlspecialchars((string) ($row['user_email'] ?? ''), ENT_QUOTES, 'UTF-8');
            
            if ($userId > 0) {
                $userLink = '<a href="' . htmlspecialchars(admin_url('users.php', ['view' => $userId]), ENT_QUOTES, 'UTF-8') . '" style="color:var(--text-primary); font-weight:600;">' . $userName . '</a>' . ($userEmail !== '' ? '<br><small style="color:var(--text-muted);">' . $userEmail . '</small>' : '');
            } else {
                $userLink = '<span style="color:var(--text-primary); font-weight:600;">' . $userName . '</span>' . (!empty($row['device_id']) ? '<br><small style="color:var(--text-muted); font-family:monospace;">Device: ' . htmlspecialchars((string)$row['device_id'], ENT_QUOTES, 'UTF-8') . '</small>' : '');
            }

            $snapshot = json_decode((string)($row['plan_snapshot'] ?? ''), true);
            if (!is_array($snapshot)) {
                $snapshot = [];
            }
            $planDisplay = htmlspecialchars(
                (string)($snapshot['name'] ?? $row['plan_name'] ?? 'Plan #' . ($row['plan_id'] ?? '')),
                ENT_QUOTES,
                'UTF-8'
            );
            
            $startRaw = $row[$subStartCol] ?? ($row['starts_at'] ?? ($row['start_date'] ?? null));
            $startsAt = !empty($startRaw) ? admin_utc_date((string)$startRaw) : 'N/A';
            
            $expVal = $row[$subExpCol] ?? ($row['expires_at'] ?? ($row['expiry_date'] ?? null));
            if (empty($expVal) || ($snapshot['duration_unit'] ?? $row['duration_unit'] ?? '') === 'unlimited') {
                $expiresAt = '<span style="color:#16a34a; font-weight:600;">Unlimited</span>';
            } else {
                $expTs = strtotime((string)$expVal);
                $expDateStr = admin_utc_date((string)$expVal);
                if ($expTs !== false && $expTs < time()) {
                    $expiresAt = "<span style=\"color:#dc2626; font-weight:600;\">{$expDateStr}</span><br><small style=\"color:#dc2626;\">Expired</small>";
                } elseif ($expTs !== false) {
                    $daysLeft = ceil(($expTs - time()) / 86400);
                    $expiresAt = "<strong>{$expDateStr}</strong><br><small style=\"color:var(--text-muted);\">{$daysLeft} days remaining</small>";
                } else {
                    $expiresAt = htmlspecialchars((string)$expVal, ENT_QUOTES, 'UTF-8');
                }
            }

            $statusVal = strtoupper((string) ($row['status'] ?? 'ACTIVE'));
            $statusClass = strtolower($statusVal);
            $amount = (float)($snapshot['price'] ?? $row['amount'] ?? $row['plan_price'] ?? 0);
            $source = htmlspecialchars((string)($row['activation_source'] ?? 'system'), ENT_QUOTES, 'UTF-8');
            $actor = htmlspecialchars((string)($row['activated_by_name'] ?? ''), ENT_QUOTES, 'UTF-8');
            $reason = htmlspecialchars((string)($row['activation_reason'] ?? ''), ENT_QUOTES, 'UTF-8');
            $auditDisplay = $actor !== '' ? "{$source} by {$actor}" : $source;
            if ($reason !== '') {
                $auditDisplay .= '<br><small>' . $reason . '</small>';
            }

            $actionBtn = '';
            if ($userId > 0) {
                $renewUrl = htmlspecialchars(admin_url('subscriptions.php', ['user_id' => $userId]), ENT_QUOTES, 'UTF-8');
                $actionBtn = "<a class=\"outline-button\" href=\"{$renewUrl}#manual-assign\" title=\"Assign or Renew Plan\">Assign / Renew</a>";
            } else {
                $actionBtn = '<span style="color:var(--text-muted); font-size:12px;">Unassigned</span>';
            }

            $rowHtml .= "<tr>
                <td>#{$subId}</td>
                <td>{$userLink}</td>
                <td><strong>{$planDisplay}</strong></td>
                <td>{$startsAt}</td>
                <td>{$expiresAt}</td>
                <td>₹" . number_format($amount, 2) . "</td>
                <td><span class=\"status-badge status-{$statusClass}\">{$statusVal}</span></td>
                <td>{$auditDisplay}</td>
                <td>{$actionBtn}</td>
            </tr>";
        }
    } else {
        $rowHtml = '<tr><td colspan="9" class="empty-state">No matching subscriptions found.</td></tr>';
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

    // Customer search prefill handling (on-demand only, never load all users to DOM)
    $prefillUserId = !empty($_GET['user_id']) ? (int)$_GET['user_id'] : 0;
    $prefillCustomer = null;

    if ($prefillUserId > 0 && admin_table_exists($pdo, 'users')) {
        $singleU = $pdo->prepare("SELECT id, name, username, email FROM `users` WHERE id = ? LIMIT 1");
        $singleU->execute([$prefillUserId]);
        $u = $singleU->fetch(PDO::FETCH_ASSOC);
        if ($u) {
            $prefillCustomer = [
                'id' => (int)$u['id'],
                'name' => (string)($u['name'] ?: ($u['username'] ?: 'User #' . $prefillUserId)),
                'email' => (string)($u['email'] ?: ''),
            ];
        }
    }

    $prefillIdVal = $prefillCustomer ? (int)$prefillCustomer['id'] : '';
    $prefillName = $prefillCustomer ? htmlspecialchars($prefillCustomer['name'], ENT_QUOTES, 'UTF-8') : '';
    $prefillEmail = $prefillCustomer ? htmlspecialchars($prefillCustomer['email'], ENT_QUOTES, 'UTF-8') : '';
    $prefillAvatar = $prefillCustomer ? strtoupper(substr($prefillCustomer['name'], 0, 1)) : 'U';
    $prefillBadge = $prefillCustomer ? 'ID #' . (int)$prefillCustomer['id'] : '';
    $selectedDisplay = $prefillCustomer ? 'flex' : 'none';
    $searchDisplay = $prefillCustomer ? 'none' : 'block';

    // Load available plans for manual activation
    $manualPlanOptions = '';
    if (!empty($planCols)) {
        $planSelect = ['id', 'name', 'price', 'duration_days'];
        if ($hasPlanValue) $planSelect[] = 'duration_value';
        if ($hasPlanUnit) $planSelect[] = 'duration_unit';

        $selectStr = implode(', ', array_map(fn($f) => "`$f`", $planSelect));
        $manualPlanStmt = $pdo->query("SELECT {$selectStr} FROM `plans` WHERE `status` = 'active' ORDER BY `price` ASC");
        
        foreach ($manualPlanStmt->fetchAll(PDO::FETCH_ASSOC) as $plan) {
            $durStr = '';
            if (!empty($plan['duration_value']) && !empty($plan['duration_unit'])) {
                $durStr = (int)$plan['duration_value'] . ' ' . $plan['duration_unit'];
            } elseif (!empty($plan['duration_days'])) {
                $durStr = (int)$plan['duration_days'] . ' days';
            } else {
                $durStr = 'Ongoing (Unlimited)';
            }
            $planLabel = htmlspecialchars(
                $plan['name'] . ' — ₹' . number_format((float)$plan['price'], 2) . ' / ' . $durStr,
                ENT_QUOTES,
                'UTF-8'
            );
            $manualPlanOptions .= '<option value="' . (int)$plan['id'] . '">' . $planLabel . '</option>';
        }
    }

    $adminCsrf = htmlspecialchars(admin_csrf_token(), ENT_QUOTES, 'UTF-8');
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

<div class="section" id="manual-assign">
    <div class="section-header"><h3>Assign, Change or Renew a Subscription Plan</h3></div>
    <form method="POST" action="{$subscriptionsUrl}" id="plan-assignment-form" class="detail-panels" style="display:grid; grid-template-columns: repeat(auto-fit, minmax(280px, 1fr)); gap:16px; align-items:end;">
        <input type="hidden" name="csrf_token" value="{$adminCsrf}">
        <input type="hidden" name="manual_activation" value="1">
        
        <!-- Searchable Customer Selector (Debounced, bounded 25-item API results, zero DOM bloat) -->
        <div class="customer-picker-field" style="position:relative;">
            <label for="customer_search_input" style="display:block; font-weight:600; font-size:13px; margin-bottom:6px; color:var(--text-primary);">
                Customer Account <span style="color:var(--danger);">*</span>
            </label>
            <input type="hidden" name="user_id" id="assign_user_id" value="{$prefillIdVal}" required>
            
            <!-- Selected Customer Display Card -->
            <div id="customer_selected_box" style="display:{$selectedDisplay}; align-items:center; justify-content:space-between; padding:8px 12px; background:var(--bg); border:1px solid var(--border); border-radius:var(--radius-sm); min-height:42px;">
                <div style="display:flex; align-items:center; gap:10px; overflow:hidden;">
                    <div style="background:var(--accent-soft); color:var(--accent); width:28px; height:28px; border-radius:50%; display:flex; align-items:center; justify-content:center; font-weight:700; font-size:12px; flex-shrink:0;">
                        <span id="selected_avatar">{$prefillAvatar}</span>
                    </div>
                    <div style="overflow:hidden; text-overflow:ellipsis; white-space:nowrap;">
                        <strong id="selected_name" style="font-size:13px; color:var(--text-primary);">{$prefillName}</strong>
                        <span id="selected_email" style="font-size:12px; color:var(--text-muted); margin-left:4px;">{$prefillEmail}</span>
                        <span id="selected_id_badge" class="kpi-pill" style="margin-left:6px; font-size:11px;">{$prefillBadge}</span>
                    </div>
                </div>
                <button type="button" id="btn_change_customer" class="outline-button" style="padding:4px 10px; font-size:12px; flex-shrink:0; margin-left:8px;" title="Change selected customer">Change</button>
            </div>

            <!-- Search Input Wrapper -->
            <div id="customer_search_wrapper" style="display:{$searchDisplay}; position:relative;">
                <div style="position:relative; display:flex; align-items:center;">
                    <svg style="position:absolute; left:12px; width:16px; height:16px; color:var(--text-muted); pointer-events:none;" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>
                    <input type="text" id="customer_search_input" placeholder="Type name, email, or account ID to search..." autocomplete="off" style="width:100%; border:1px solid var(--border); border-radius:var(--radius-sm); padding:10px 10px 10px 36px; background:var(--bg-card); color:var(--text-primary); font-size:13px;">
                    <span id="customer_search_spinner" style="position:absolute; right:12px; display:none; font-size:12px; color:var(--text-muted);">Searching...</span>
                </div>
                
                <!-- Results Dropdown Panel -->
                <div id="customer_results_dropdown" style="display:none; position:absolute; top:calc(100% + 4px); left:0; right:0; z-index:1000; background:var(--surface); border:1px solid var(--border); border-radius:var(--radius-md); box-shadow:0 10px 25px -5px rgba(0,0,0,0.15); max-height:280px; overflow-y:auto;">
                    <!-- Dynamically populated via api_customers.php -->
                </div>
            </div>
            <div id="customer_error_msg" style="color:var(--danger); font-size:12px; margin-top:4px; display:none;"></div>
        </div>

        <label style="display:block; font-weight:600; font-size:13px; color:var(--text-primary);">Plan to Assign <span style="color:var(--danger);">*</span>
            <select name="plan_id" id="assign_plan_id" required style="width:100%; border:1px solid var(--border); border-radius:var(--radius-sm); padding:10px; background:var(--bg-card); color:var(--text-primary); font-size:13px; margin-top:6px;">
                <option value="">-- Choose Subscription Plan --</option>
                {$manualPlanOptions}
            </select>
        </label>

        <label style="display:block; font-weight:600; font-size:13px; color:var(--text-primary);">Reason / Reference <span style="color:var(--danger);">*</span>
            <input type="text" name="reason" maxlength="500" placeholder="e.g. Annual renewal or manual administrative activation" required style="width:100%; border:1px solid var(--border); border-radius:var(--radius-sm); padding:10px; background:var(--bg-card); color:var(--text-primary); font-size:13px; margin-top:6px;">
        </label>

        <div>
            <button class="button" type="submit" id="btn_save_assignment" style="height:42px; width:100%; font-size:14px; font-weight:600;">Save Plan Assignment</button>
        </div>
    </form>
    <p style="margin-top:8px; font-size:12px; color:var(--text-muted);">Searchable selector queries customer accounts on-demand via authenticated API without loading full user directory to the browser. Prior active subscriptions are marked expired atomically with an audit snapshot.</p>
</div>

<!-- Search & Filter Bar -->
<div class="filter-bar">
    <form method="GET" action="{$subscriptionsUrl}" style="display:flex; gap:10px; flex-wrap:wrap; align-items:center; width:100%;">
        <div class="filter-group">
            <input type="text" name="q" value="{$searchValue}" placeholder="Search customer, device, plan..." class="filter-input" style="min-width:260px;">
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
                    <th>Customer Account / Device</th>
                    <th>Assigned Plan</th>
                    <th>{$thStarted}</th>
                    <th>{$thExpires}</th>
                    <th>{$thPrice}</th>
                    <th>{$thStatus}</th>
                    <th>Activation Audit</th>
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

<script>
(function() {
    const searchInput = document.getElementById('customer_search_input');
    const searchWrapper = document.getElementById('customer_search_wrapper');
    const searchSpinner = document.getElementById('customer_search_spinner');
    const dropdown = document.getElementById('customer_results_dropdown');
    const hiddenUserId = document.getElementById('assign_user_id');
    const selectedBox = document.getElementById('customer_selected_box');
    const btnChange = document.getElementById('btn_change_customer');
    const selectedAvatar = document.getElementById('selected_avatar');
    const selectedName = document.getElementById('selected_name');
    const selectedEmail = document.getElementById('selected_email');
    const selectedBadge = document.getElementById('selected_id_badge');
    const errorMsg = document.getElementById('customer_error_msg');
    const form = document.getElementById('plan-assignment-form');

    let debounceTimer = null;
    let currentResults = [];
    let highlightedIndex = -1;

    function escapeHtml(str) {
        if (!str) return '';
        return String(str).replace(/[&<>"']/g, function(m) {
            return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#039;' }[m];
        });
    }

    function hideDropdown() {
        if (dropdown) dropdown.style.display = 'none';
        highlightedIndex = -1;
    }

    function selectCustomer(cust) {
        if (!cust) return;
        hiddenUserId.value = cust.id;
        selectedName.textContent = cust.name;
        selectedEmail.textContent = cust.email ? '(' + cust.email + ')' : '';
        selectedAvatar.textContent = (cust.name || 'U').charAt(0).toUpperCase();
        selectedBadge.textContent = 'ID #' + cust.id + (cust.unique_user_id ? ' • ' + cust.unique_user_id : '');
        selectedBox.style.display = 'flex';
        searchWrapper.style.display = 'none';
        searchInput.value = '';
        hideDropdown();
        if (errorMsg) errorMsg.style.display = 'none';
    }

    if (btnChange) {
        btnChange.addEventListener('click', function(e) {
            e.preventDefault();
            hiddenUserId.value = '';
            selectedBox.style.display = 'none';
            searchWrapper.style.display = 'block';
            searchInput.focus();
            fetchCustomers('');
        });
    }

    function renderResults(users, query) {
        currentResults = users || [];
        highlightedIndex = -1;
        if (!currentResults.length) {
            dropdown.innerHTML = '<div style="padding:14px; text-align:center; color:var(--text-muted); font-size:13px;">No customer accounts found' + (query ? ' for &ldquo;' + escapeHtml(query) + '&rdquo;' : '') + '.</div>';
            dropdown.style.display = 'block';
            return;
        }

        let html = '';
        currentResults.forEach((u, idx) => {
            const avatar = escapeHtml((u.name || 'U').charAt(0).toUpperCase());
            const name = escapeHtml(u.name || 'User #' + u.id);
            const email = u.email ? '<span style="color:var(--text-muted); font-size:12px; margin-left:4px;">(' + escapeHtml(u.email) + ')</span>' : '';
            const uid = u.unique_user_id ? '<span style="font-size:11px; color:var(--text-muted); font-family:monospace; margin-left:6px;">UID: ' + escapeHtml(u.unique_user_id) + '</span>' : '';
            html += '<div class="customer-result-item" data-index="' + idx + '" role="option" tabindex="-1" style="padding:10px 14px; cursor:pointer; display:flex; align-items:center; justify-content:space-between; border-bottom:1px solid var(--border-subtle); transition:background 0.12s ease;">'
                  + '<div style="display:flex; align-items:center; gap:10px; overflow:hidden;">'
                  + '<div style="background:var(--accent-soft); color:var(--accent); width:28px; height:28px; border-radius:50%; display:flex; align-items:center; justify-content:center; font-weight:700; font-size:12px; flex-shrink:0;">' + avatar + '</div>'
                  + '<div style="overflow:hidden; text-overflow:ellipsis; white-space:nowrap;">'
                  + '<strong style="color:var(--text-primary); font-size:13px;">' + name + '</strong>' + email + uid
                  + '</div>'
                  + '</div>'
                  + '<div style="flex-shrink:0; margin-left:8px;"><span class="kpi-pill" style="font-size:11px; font-weight:600;">ID #' + u.id + '</span></div>'
                  + '</div>';
        });

        if (currentResults.length >= 25) {
            html += '<div style="padding:8px 12px; font-size:11px; color:var(--text-muted); background:var(--bg); text-align:center;">Showing top 25 matches. Type more characters to refine search.</div>';
        }

        dropdown.innerHTML = html;
        dropdown.style.display = 'block';

        dropdown.querySelectorAll('.customer-result-item').forEach(el => {
            el.addEventListener('click', function() {
                const idx = parseInt(this.getAttribute('data-index'), 10);
                if (currentResults[idx]) {
                    selectCustomer(currentResults[idx]);
                }
            });
            el.addEventListener('mouseenter', function() {
                highlightItem(parseInt(this.getAttribute('data-index'), 10));
            });
        });
    }

    function highlightItem(idx) {
        const items = dropdown.querySelectorAll('.customer-result-item');
        items.forEach((it, i) => {
            if (i === idx) {
                it.style.background = 'var(--bg)';
                it.setAttribute('aria-selected', 'true');
            } else {
                it.style.background = '';
                it.removeAttribute('aria-selected');
            }
        });
        highlightedIndex = idx;
    }

    function fetchCustomers(q) {
        if (searchSpinner) searchSpinner.style.display = 'inline';
        const url = 'api_customers.php?q=' + encodeURIComponent(q) + '&limit=25';
        fetch(url, { headers: { 'Accept': 'application/json' } })
            .then(res => {
                if (!res.ok) throw new Error('HTTP ' + res.status);
                return res.json();
            })
            .then(data => {
                if (searchSpinner) searchSpinner.style.display = 'none';
                if (data.status === 'success' && Array.isArray(data.users)) {
                    renderResults(data.users, q);
                } else {
                    dropdown.innerHTML = '<div style="padding:14px; text-align:center; color:var(--danger); font-size:13px;">' + escapeHtml(data.message || 'Unable to load accounts.') + '</div>';
                    dropdown.style.display = 'block';
                }
            })
            .catch(err => {
                if (searchSpinner) searchSpinner.style.display = 'none';
                dropdown.innerHTML = '<div style="padding:14px; text-align:center; color:var(--danger); font-size:13px;">Failed to load accounts. Please check network.</div>';
                dropdown.style.display = 'block';
            });
    }

    if (searchInput) {
        searchInput.addEventListener('focus', function() {
            if (dropdown.style.display === 'none' && !hiddenUserId.value) {
                fetchCustomers(searchInput.value.trim());
            }
        });

        searchInput.addEventListener('input', function() {
            clearTimeout(debounceTimer);
            const val = searchInput.value.trim();
            debounceTimer = setTimeout(function() {
                fetchCustomers(val);
            }, 250);
        });

        searchInput.addEventListener('keydown', function(e) {
            const items = dropdown.querySelectorAll('.customer-result-item');
            if (e.key === 'ArrowDown') {
                e.preventDefault();
                if (dropdown.style.display === 'none') {
                    fetchCustomers(searchInput.value.trim());
                    return;
                }
                if (highlightedIndex < items.length - 1) {
                    highlightItem(highlightedIndex + 1);
                    items[highlightedIndex].scrollIntoView({ block: 'nearest' });
                }
            } else if (e.key === 'ArrowUp') {
                e.preventDefault();
                if (highlightedIndex > 0) {
                    highlightItem(highlightedIndex - 1);
                    items[highlightedIndex].scrollIntoView({ block: 'nearest' });
                }
            } else if (e.key === 'Enter') {
                if (dropdown.style.display !== 'none' && highlightedIndex >= 0 && currentResults[highlightedIndex]) {
                    e.preventDefault();
                    selectCustomer(currentResults[highlightedIndex]);
                }
            } else if (e.key === 'Escape') {
                hideDropdown();
            }
        });
    }

    document.addEventListener('click', function(e) {
        if (!searchWrapper.contains(e.target) && !dropdown.contains(e.target)) {
            hideDropdown();
        }
    });

    if (form) {
        form.addEventListener('submit', function(e) {
            const uidVal = hiddenUserId.value ? parseInt(hiddenUserId.value, 10) : 0;
            if (isNaN(uidVal) || uidVal <= 0) {
                e.preventDefault();
                if (errorMsg) {
                    errorMsg.textContent = 'Please search and select a customer account first.';
                    errorMsg.style.display = 'block';
                }
                if (searchWrapper.style.display !== 'none') {
                    searchInput.focus();
                }
                return false;
            }
            if (!confirm('Assign this subscription plan now? Prior active subscriptions for this account will be transitioned.')) {
                e.preventDefault();
                return false;
            }
        });
    }
})();
</script>
HTML;

    echo admin_render_page('Subscriptions', 'subscriptions', $body);

} catch (Throwable $e) {
    admin_log_error($e, 'subscriptions.php execution');
    echo admin_render_page('Subscriptions', 'subscriptions', admin_safe_error_html('Unexpected Error', 'An error occurred while loading the Subscriptions page.', $e));
}
