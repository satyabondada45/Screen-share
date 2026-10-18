<?php
require_once __DIR__ . '/includes/db.php';
require_once __DIR__ . '/includes/auth.php';
require_once __DIR__ . '/includes/layout.php';

admin_require_auth();

try {
    $pdo = admin_db();
} catch (Throwable $e) {
    admin_log_error($e, 'Database connection');
    echo admin_render_page('System Error', 'error', admin_safe_error_html('Database Connection Failed', 'Unable to connect to the database. Please check system configuration.', $e));
    exit;
}

$userTable = admin_table_exists($pdo, 'users') ? 'users' : null;
if (!$userTable) {
    echo admin_render_page('Users', 'users', admin_safe_error_html('Table Unavailable', 'The users database table is currently unavailable.'));
    exit;
}

try {
    $userCols = admin_table_columns($pdo, 'users');
    $hasRole = in_array('role', $userCols, true);
    $hasStatus = in_array('status', $userCols, true);
    $hasUsername = in_array('username', $userCols, true);
    $hasUniqueId = in_array('unique_user_id', $userCols, true);

    $subCols = admin_table_exists($pdo, 'subscriptions') ? admin_table_columns($pdo, 'subscriptions') : [];
    $hasSubUserId = in_array('user_id', $subCols, true);
    $hasSubDeviceId = in_array('device_id', $subCols, true);
    $subExpCol = in_array('expires_at', $subCols, true) ? 'expires_at' : (in_array('expiry_date', $subCols, true) ? 'expiry_date' : null);
    $subStartCol = in_array('starts_at', $subCols, true) ? 'starts_at' : (in_array('start_date', $subCols, true) ? 'start_date' : null);

    $planCols = admin_table_exists($pdo, 'plans') ? admin_table_columns($pdo, 'plans') : [];
    $hasPlanUnit = in_array('duration_unit', $planCols, true);

    $payCols = admin_table_exists($pdo, 'payments') ? admin_table_columns($pdo, 'payments') : [];
    $hasPayUserId = in_array('user_id', $payCols, true);
    $hasPayDeviceId = in_array('device_id', $payCols, true);
    $payTxnCol = in_array('transaction_id', $payCols, true) ? 'transaction_id' : (in_array('payment_reference', $payCols, true) ? 'payment_reference' : (in_array('gateway_payment_id', $payCols, true) ? 'gateway_payment_id' : null));

    // ─────────────────────────────────────────────────────────────────────────────
    // HANDLE USER STATUS UPDATE (Active / Inactive / Suspended)
    // ─────────────────────────────────────────────────────────────────────────────
    if (isset($_GET['action']) && $_GET['action'] === 'status' && isset($_GET['id'])) {
        $targetUserId = (int)$_GET['id'];
        $newStatus = strtolower(trim((string)($_GET['status'] ?? 'active')));
        $token = $_GET['token'] ?? '';
        if (!admin_verify_csrf_token($token)) {
            admin_set_flash('error', 'Invalid security token. Please try again.');
            header('Location: ' . admin_url('users.php', ['view' => $targetUserId]));
            exit;
        }

        if (!in_array($newStatus, ['active', 'inactive', 'suspended'], true)) {
            admin_set_flash('error', 'Invalid status selected.');
            header('Location: ' . admin_url('users.php', ['view' => $targetUserId]));
            exit;
        }

        try {
            $uStmt = $pdo->prepare('SELECT id, name, email FROM `users` WHERE id = ? LIMIT 1');
            $uStmt->execute([$targetUserId]);
            $targetUser = $uStmt->fetch(PDO::FETCH_ASSOC);
            if (!$targetUser) {
                admin_set_flash('error', 'User not found.');
            } else {
                if ($hasStatus) {
                    $upd = $pdo->prepare('UPDATE `users` SET `status` = ? WHERE `id` = ?');
                    $upd->execute([$newStatus, $targetUserId]);
                }
                admin_log_action($pdo, 'user_status_update', "Updated User #{$targetUserId} status to '{$newStatus}'");
                admin_set_flash('success', "Account for '{$targetUser['name']}' is now " . ucfirst($newStatus) . ".");
            }
        } catch (Throwable $e) {
            admin_log_error($e, 'User status toggle error');
            admin_set_flash('error', 'Unable to update user status.');
        }

        header('Location: ' . admin_url('users.php', ['view' => $targetUserId]));
        exit;
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // HANDLE ADMIN CREATE USER ACCOUNT WITH AUTOMATIC WELCOME PLAN
    // ─────────────────────────────────────────────────────────────────────────────
    if (($_SERVER['REQUEST_METHOD'] ?? '') === 'POST' && isset($_POST['create_user'])) {
        $token = $_POST['csrf_token'] ?? '';
        if (!admin_verify_csrf_token($token)) {
            admin_set_flash('error', 'Invalid security token. Please try again.');
        } else {
            $newUserName = trim((string)($_POST['name'] ?? ''));
            $newUserEmail = trim((string)($_POST['email'] ?? ''));
            $newUserPass = (string)($_POST['password'] ?? '');

            if ($newUserName === '' || $newUserEmail === '' || $newUserPass === '') {
                admin_set_flash('error', 'All fields (Name, Email, Password) are required.');
            } elseif (!filter_var($newUserEmail, FILTER_VALIDATE_EMAIL)) {
                admin_set_flash('error', 'Invalid email address provided.');
            } else {
                try {
                    $chk = $pdo->prepare('SELECT id FROM `users` WHERE email = ? LIMIT 1');
                    $chk->execute([$newUserEmail]);
                    if ($chk->fetch()) {
                        throw new InvalidArgumentException('Email address is already registered.');
                    }

                    do {
                        $uniqueUserId = (string)random_int(100000000, 999999999);
                        $checkUid = $pdo->prepare('SELECT id FROM `users` WHERE unique_user_id = ? LIMIT 1');
                        $checkUid->execute([$uniqueUserId]);
                    } while ($checkUid->fetch());

                    $passHash = password_hash($newUserPass, PASSWORD_DEFAULT);

                    $pdo->beginTransaction();
                    $insCols = ['name', 'email', 'password_hash', 'unique_user_id', 'created_at'];
                    $insVals = ['?', '?', '?', '?', 'NOW()'];
                    $insParams = [$newUserName, $newUserEmail, $passHash, $uniqueUserId];

                    if ($hasUsername) {
                        $insCols[] = 'username';
                        $insVals[] = '?';
                        $insParams[] = $newUserName;
                    }
                    if ($hasStatus) {
                        $insCols[] = 'status';
                        $insVals[] = '?';
                        $insParams[] = 'active';
                    }

                    $insSql = "INSERT INTO `users` (" . implode(', ', array_map(fn($c) => "`$c`", $insCols)) . ") VALUES (" . implode(', ', $insVals) . ")";
                    $pdo->prepare($insSql)->execute($insParams);
                    $newUserId = (int)$pdo->lastInsertId();

                    $subService = admin_service_file('SubscriptionService.php');
                    if ($subService && is_file($subService)) {
                        require_once $subService;
                        $welcomeSub = SubscriptionService::assignWelcomePlanIfEligible($newUserId, true);
                        if (!$welcomeSub) {
                            throw new RuntimeException("Welcome Plan assignment could not be verified for new account.");
                        }
                    }

                    $pdo->commit();
                    admin_log_action($pdo, 'user_create', "Created user account '{$newUserEmail}' (#{$newUserId}) with automatic Welcome Plan.");
                    $durVal = !empty($welcomeSub['duration_value']) ? (int)$welcomeSub['duration_value'] : 6;
                    admin_set_flash('success', "User account '{$newUserName}' created successfully with {$durVal}-month Welcome Plan activated.");
                } catch (Throwable $e) {
                    if ($pdo->inTransaction()) {
                        $pdo->rollBack();
                    }
                    admin_log_error($e, 'Admin create user');
                    admin_set_flash('error', 'Unable to create user: ' . $e->getMessage());
                }
            }
        }
        header('Location: ' . admin_url('users.php'));
        exit;
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // DETAIL VIEW (view = USER_ID)
    // ─────────────────────────────────────────────────────────────────────────────
    if (isset($_GET['view'])) {
        $viewId = (int) $_GET['view'];
        $stmt = $pdo->prepare('SELECT * FROM `users` WHERE `id` = ? LIMIT 1');
        $stmt->execute([$viewId]);
        $user = $stmt->fetch(PDO::FETCH_ASSOC);

        if (!$user) {
            admin_set_flash('error', 'The requested user could not be found.');
            header('Location: ' . admin_url('users.php'));
            exit;
        }

        $name = htmlspecialchars((string) ($user['name'] ?? ($user['username'] ?? 'User #' . $user['id'])), ENT_QUOTES, 'UTF-8');
        $email = htmlspecialchars((string) ($user['email'] ?? 'N/A'), ENT_QUOTES, 'UTF-8');
        $username = htmlspecialchars((string) ($user['username'] ?? '—'), ENT_QUOTES, 'UTF-8');
        $uniqueId = htmlspecialchars((string) ($user['unique_user_id'] ?? '—'), ENT_QUOTES, 'UTF-8');
        $role = strtoupper((string) ($user['role'] ?? 'USER'));
        $status = strtoupper((string) ($user['status'] ?? 'ACTIVE'));
        $statusClass = strtolower($status);
        $created = !empty($user['created_at']) ? htmlspecialchars((string) $user['created_at'], ENT_QUOTES, 'UTF-8') : 'N/A';

        // 1. User Devices
        $userDevices = [];
        $deviceIdentifiers = [];
        if (admin_table_exists($pdo, 'devices')) {
            $devStmt = $pdo->prepare('SELECT * FROM `devices` WHERE `user_id` = ? ORDER BY id DESC');
            $devStmt->execute([$viewId]);
            $userDevices = $devStmt->fetchAll(PDO::FETCH_ASSOC);
            foreach ($userDevices as $dev) {
                if (!empty($dev['id'])) $deviceIdentifiers[] = (string)$dev['id'];
                if (!empty($dev['system_id'])) $deviceIdentifiers[] = (string)$dev['system_id'];
                if (!empty($dev['device_uid'])) $deviceIdentifiers[] = (string)$dev['device_uid'];
            }
            $deviceIdentifiers = array_values(array_unique(array_filter($deviceIdentifiers)));
        }

        $deviceHtml = '';
        if (!empty($userDevices)) {
            $deviceHtml = '<table><thead><tr><th>System ID</th><th>Device Name</th><th>Status</th><th>OS</th><th>IP Address</th><th>Last Seen</th><th>Actions</th></tr></thead><tbody>';
            foreach ($userDevices as $dev) {
                $devId = (int) $dev['id'];
                $sysId = htmlspecialchars((string) ($dev['system_id'] ?? ($dev['device_uid'] ?? 'ID#' . $devId)), ENT_QUOTES, 'UTF-8');
                $devName = htmlspecialchars((string) ($dev['name'] ?? 'Unnamed'), ENT_QUOTES, 'UTF-8');
                $isOnline = !empty($dev['is_online']);
                $devStatus = $isOnline ? '<span class="status-badge status-active">Online</span>' : '<span class="status-badge status-inactive">Offline</span>';
                $os = htmlspecialchars((string) ($dev['os_type'] ?? '—'), ENT_QUOTES, 'UTF-8');
                $ip = htmlspecialchars((string) ($dev['ip_address'] ?? '—'), ENT_QUOTES, 'UTF-8');
                $lastSeen = admin_time_ago($dev['last_seen_at'] ?? null);
                $viewDevUrl = htmlspecialchars(admin_url('devices.php', ['view' => $devId]), ENT_QUOTES, 'UTF-8');

                $deviceHtml .= "<tr>
                    <td><code>{$sysId}</code></td>
                    <td><strong>{$devName}</strong></td>
                    <td>{$devStatus}</td>
                    <td>{$os}</td>
                    <td>{$ip}</td>
                    <td><small>{$lastSeen}</small></td>
                    <td><a class=\"outline-button\" href=\"{$viewDevUrl}\">View Device</a></td>
                </tr>";
            }
            $deviceHtml .= '</tbody></table>';
        } else {
            $deviceHtml = '<div class="empty-state">No remote devices are registered to this user account.</div>';
        }

        // 2. User Subscriptions (Schema Adaptive)
        $userSubs = [];
        $currentSubscription = null;
        if (!empty($subCols)) {
            $unitSelect = $hasPlanUnit ? ', p.duration_unit' : ', "days" AS duration_unit';
            $priceSelect = in_array('price', $planCols, true) ? ', p.price' : '';
            
            if ($hasSubUserId) {
                $subStmt = $pdo->prepare("
                    SELECT s.*, p.name AS plan_name {$priceSelect} {$unitSelect}
                    FROM `subscriptions` s
                    LEFT JOIN `plans` p ON s.plan_id = p.id
                    WHERE s.user_id = ?
                    ORDER BY s.id DESC
                ");
                $subStmt->execute([$viewId]);
                $userSubs = $subStmt->fetchAll(PDO::FETCH_ASSOC);
            } elseif ($hasSubDeviceId && !empty($deviceIdentifiers)) {
                $placeholders = implode(',', array_fill(0, count($deviceIdentifiers), '?'));
                $subStmt = $pdo->prepare("
                    SELECT s.*, p.name AS plan_name {$priceSelect} {$unitSelect}
                    FROM `subscriptions` s
                    LEFT JOIN `plans` p ON s.plan_id = p.id
                    WHERE s.device_id IN ({$placeholders})
                    ORDER BY s.id DESC
                ");
                $subStmt->execute($deviceIdentifiers);
                $userSubs = $subStmt->fetchAll(PDO::FETCH_ASSOC);
            }

            foreach ($userSubs as $subCandidate) {
                if (strtolower($subCandidate['status'] ?? '') === 'active') {
                    $currentSubscription = $subCandidate;
                    break;
                }
            }
        }

        $currentSnapshot = json_decode((string)($currentSubscription['plan_snapshot'] ?? ''), true);
        if (!is_array($currentSnapshot)) {
            $currentSnapshot = [];
        }
        $currentPlan = htmlspecialchars(
            (string)($currentSnapshot['name'] ?? $currentSubscription['plan_name'] ?? 'Free Plan'),
            ENT_QUOTES,
            'UTF-8'
        );
        $currentExpiryValue = $currentSubscription[$subExpCol] ?? ($currentSubscription['expires_at'] ?? ($currentSubscription['expiry_date'] ?? null));
        $currentExpiry = $currentExpiryValue
            ? htmlspecialchars(admin_utc_date($currentExpiryValue), ENT_QUOTES, 'UTF-8')
            : 'Ongoing';

        $subHtml = '';
        if (!empty($userSubs)) {
            $subHtml = '<table><thead><tr><th>Sub ID</th><th>Plan</th><th>Price</th><th>Starts At</th><th>Expires At</th><th>Status</th><th>Activation Audit</th></tr></thead><tbody>';
            foreach ($userSubs as $sub) {
                $subId = (int) $sub['id'];
                $snapshot = json_decode((string)($sub['plan_snapshot'] ?? ''), true);
                if (!is_array($snapshot)) {
                    $snapshot = [];
                }
                $planName = htmlspecialchars(
                    (string)($snapshot['name'] ?? $sub['plan_name'] ?? 'Plan #' . ($sub['plan_id'] ?? '')),
                    ENT_QUOTES,
                    'UTF-8'
                );
                $price = '₹' . number_format((float)($snapshot['price'] ?? $sub['amount'] ?? $sub['price'] ?? 0), 2);
                $startVal = $sub[$subStartCol] ?? ($sub['starts_at'] ?? ($sub['start_date'] ?? null));
                $startsAt = !empty($startVal) ? htmlspecialchars(admin_utc_date((string)$startVal), ENT_QUOTES, 'UTF-8') : 'N/A';
                
                $expVal = $sub[$subExpCol] ?? ($sub['expires_at'] ?? ($sub['expiry_date'] ?? null));
                if (empty($expVal) || ($sub['duration_unit'] ?? '') === 'unlimited') {
                    $expiresAt = '<span style="color:#16a34a; font-weight:600;">Unlimited</span>';
                } else {
                    $expiresAt = htmlspecialchars(admin_utc_date((string)$expVal), ENT_QUOTES, 'UTF-8');
                }
                $subStat = strtoupper((string) ($sub['status'] ?? 'ACTIVE'));
                $subClass = strtolower($subStat);
                $source = htmlspecialchars((string)($sub['activation_source'] ?? 'legacy'), ENT_QUOTES, 'UTF-8');
                $actorId = !empty($sub['activated_by']) ? ' (admin #' . (int)$sub['activated_by'] . ')' : '';
                $reason = htmlspecialchars((string)($sub['activation_reason'] ?? ''), ENT_QUOTES, 'UTF-8');
                $audit = $source . $actorId . ($reason !== '' ? '<br><small>' . $reason . '</small>' : '');

                $subHtml .= "<tr>
                    <td>#{$subId}</td>
                    <td><strong>{$planName}</strong></td>
                    <td>{$price}</td>
                    <td>{$startsAt}</td>
                    <td>{$expiresAt}</td>
                    <td><span class=\"status-badge status-{$subClass}\">{$subStat}</span></td>
                    <td>{$audit}</td>
                </tr>";
            }
            $subHtml .= '</tbody></table>';
        } else {
            $subHtml = '<div class="empty-state">No subscriptions have been assigned to this user yet.</div>';
        }

        // 3. User Remote Sessions
        $userSessions = [];
        if (admin_table_exists($pdo, 'sessions') && !empty($userDevices)) {
            $deviceIds = array_column($userDevices, 'id');
            if (!empty($deviceIds)) {
                $placeholders = implode(',', array_fill(0, count($deviceIds), '?'));
                $sessStmt = $pdo->prepare("
                    SELECT s.*, d.name AS device_name
                    FROM `sessions` s
                    LEFT JOIN `devices` d ON s.host_device_id = d.id
                    WHERE s.host_device_id IN ({$placeholders})
                    ORDER BY s.id DESC
                    LIMIT 20
                ");
                $sessStmt->execute($deviceIds);
                $userSessions = $sessStmt->fetchAll(PDO::FETCH_ASSOC);
            }
        }

        $sessHtml = '';
        if (!empty($userSessions)) {
            $sessHtml = '<table><thead><tr><th>Session Key</th><th>Device</th><th>Status</th><th>Bandwidth</th><th>Duration</th><th>Started</th></tr></thead><tbody>';
            foreach ($userSessions as $s) {
                $key = htmlspecialchars((string) ($s['session_key'] ?? 'N/A'), ENT_QUOTES, 'UTF-8');
                $dName = htmlspecialchars((string) ($s['device_name'] ?? 'Device #' . $s['host_device_id']), ENT_QUOTES, 'UTF-8');
                $st = strtoupper((string) ($s['status'] ?? 'COMPLETED'));
                $stClass = strtolower($st);
                $bw = admin_format_bytes((float) ($s['bandwidth_used_mb'] ?? 0) * 1024 * 1024);
                $dur = admin_format_duration($s['started_at'] ?? null, $s['ended_at'] ?? null);
                $start = admin_time_ago($s['started_at'] ?? null);

                $sessHtml .= "<tr>
                    <td><code>{$key}</code></td>
                    <td>{$dName}</td>
                    <td><span class=\"status-badge status-{$stClass}\">{$st}</span></td>
                    <td>{$bw}</td>
                    <td>{$dur}</td>
                    <td><small>{$start}</small></td>
                </tr>";
            }
            $sessHtml .= '</tbody></table>';
        } else {
            $sessHtml = '<div class="empty-state">No remote sessions recorded for this user\'s devices.</div>';
        }

        // 4. User Payments (Schema Adaptive)
        $userPayments = [];
        if (!empty($payCols)) {
            if ($hasPayUserId) {
                $payStmt = $pdo->prepare('SELECT * FROM `payments` WHERE `user_id` = ? ORDER BY id DESC');
                $payStmt->execute([$viewId]);
                $userPayments = $payStmt->fetchAll(PDO::FETCH_ASSOC);
            } elseif ($hasPayDeviceId && !empty($deviceIdentifiers)) {
                $placeholders = implode(',', array_fill(0, count($deviceIdentifiers), '?'));
                $payStmt = $pdo->prepare("SELECT * FROM `payments` WHERE `device_id` IN ({$placeholders}) ORDER BY id DESC");
                $payStmt->execute($deviceIdentifiers);
                $userPayments = $payStmt->fetchAll(PDO::FETCH_ASSOC);
            }
        }

        $payHtml = '';
        if (!empty($userPayments)) {
            $payHtml = '<table><thead><tr><th>Payment ID</th><th>Transaction / Ref</th><th>Amount</th><th>Method</th><th>Status</th><th>Date</th></tr></thead><tbody>';
            foreach ($userPayments as $p) {
                $pId = (int) $p['id'];
                $txn = htmlspecialchars((string) ($p[$payTxnCol] ?? ($p['transaction_id'] ?? ($p['payment_reference'] ?? ($p['gateway_payment_id'] ?? '—')))), ENT_QUOTES, 'UTF-8');
                $amt = '₹' . number_format((float) ($p['amount'] ?? 0), 2);
                $meth = htmlspecialchars((string) ($p['payment_method'] ?? 'Online'), ENT_QUOTES, 'UTF-8');
                $pStat = strtoupper((string) ($p['status'] ?? 'COMPLETED'));
                $pClass = strtolower($pStat);
                $pDate = !empty($p['created_at']) ? htmlspecialchars((string) $p['created_at'], ENT_QUOTES, 'UTF-8') : 'N/A';

                $payHtml .= "<tr>
                    <td>#{$pId}</td>
                    <td><code>{$txn}</code></td>
                    <td>{$amt}</td>
                    <td>{$meth}</td>
                    <td><span class=\"status-badge status-{$pClass}\">{$pStat}</span></td>
                    <td>{$pDate}</td>
                </tr>";
            }
            $payHtml .= '</tbody></table>';
        } else {
            $payHtml = '<div class="empty-state">No payment records found for this user.</div>';
        }

        $usersBackUrl = htmlspecialchars(admin_url('users.php'), ENT_QUOTES, 'UTF-8');
        $devCount = count($userDevices);
        $subCount = count($userSubs);
        $sessCount = count($userSessions);
        $payCount = count($userPayments);

        $csrfToken = admin_csrf_token();
        $activateUrl = htmlspecialchars(admin_url('users.php', ['action' => 'status', 'id' => $viewId, 'status' => 'active', 'token' => $csrfToken]), ENT_QUOTES, 'UTF-8');
        $deactivateUrl = htmlspecialchars(admin_url('users.php', ['action' => 'status', 'id' => $viewId, 'status' => 'inactive', 'token' => $csrfToken]), ENT_QUOTES, 'UTF-8');
        $suspendUrl = htmlspecialchars(admin_url('users.php', ['action' => 'status', 'id' => $viewId, 'status' => 'suspended', 'token' => $csrfToken]), ENT_QUOTES, 'UTF-8');
        $assignPlanUrl = htmlspecialchars(admin_url('subscriptions.php', ['user_id' => $viewId]), ENT_QUOTES, 'UTF-8');

        $statusActionsHtml = '';
        if (strtolower($status) !== 'active') {
            $statusActionsHtml .= '<a class="outline-button" href="' . $activateUrl . '" onclick="return confirm(\'Set this user account to ACTIVE?\');" style="color:#16a34a; border-color:#bbf7d0;">Set Active</a>';
        }
        if (strtolower($status) !== 'inactive') {
            $statusActionsHtml .= '<a class="outline-button" href="' . $deactivateUrl . '" onclick="return confirm(\'Set this user account to INACTIVE?\');" style="color:#6b7280;">Deactivate</a>';
        }
        if (strtolower($status) !== 'suspended') {
            $statusActionsHtml .= '<a class="outline-button" href="' . $suspendUrl . '" onclick="return confirm(\'SUSPEND this user account?\');" style="color:#dc2626; border-color:#fecaca;">Suspend</a>';
        }

        $body = <<<HTML
<div class="section">
    <div class="section-header" style="flex-wrap:wrap; gap:10px;">
        <h3>User Profile: {$name}</h3>
        <div class="mini-actions" style="display:flex; gap:8px; align-items:center; flex-wrap:wrap;">
            <a class="button" href="{$assignPlanUrl}" style="background:#16a34a; font-size:0.825rem; padding:6px 14px;">+ Assign Plan</a>
            {$statusActionsHtml}
            <a class="outline-button" href="{$usersBackUrl}">&larr; Back to Users</a>
        </div>
    </div>
    <div class="detail-panels">
        <div class="detail-panel">
            <h4>Account Information</h4>
            <div class="detail-row"><span>User ID</span><strong>#{$viewId}</strong></div>
            <div class="detail-row"><span>Unique User ID</span><strong><code>{$uniqueId}</code></strong></div>
            <div class="detail-row"><span>Full Name</span><strong>{$name}</strong></div>
            <div class="detail-row"><span>Username</span><strong>{$username}</strong></div>
            <div class="detail-row"><span>Email Address</span><strong>{$email}</strong></div>
            <div class="detail-row"><span>Role</span><strong><span class="status-badge">{$role}</span></strong></div>
            <div class="detail-row"><span>Account Status</span><strong><span class="status-badge status-{$statusClass}">{$status}</span></strong></div>
            <div class="detail-row"><span>Registered On</span><strong>{$created}</strong></div>
        </div>
        <div class="detail-panel">
            <h4>Associated Assets & Subscription</h4>
            <div class="detail-row"><span>Current Active Plan</span><strong>{$currentPlan}</strong></div>
            <div class="detail-row"><span>Plan Expiry</span><strong>{$currentExpiry}</strong></div>
            <div class="detail-row"><span>Registered Devices</span><strong>{$devCount} devices</strong></div>
            <div class="detail-row"><span>Assigned Subscriptions</span><strong>{$subCount} records</strong></div>
            <div class="detail-row"><span>Recorded Sessions</span><strong>{$sessCount} sessions</strong></div>
            <div class="detail-row"><span>Payment Transactions</span><strong>{$payCount} records</strong></div>
        </div>
    </div>
</div>

<div class="section">
    <div class="section-header">
        <h3>Registered Devices ({$devCount})</h3>
    </div>
    <div style="overflow:auto;">
        {$deviceHtml}
    </div>
</div>

<div class="section">
    <div class="section-header">
        <h3>Assigned Subscriptions ({$subCount})</h3>
    </div>
    <div style="overflow:auto;">
        {$subHtml}
    </div>
</div>

<div class="section">
    <div class="section-header">
        <h3>Recent Remote Sessions ({$sessCount})</h3>
    </div>
    <div style="overflow:auto;">
        {$sessHtml}
    </div>
</div>

<div class="section">
    <div class="section-header">
        <h3>Payments & Transactions ({$payCount})</h3>
    </div>
    <div style="overflow:auto;">
        {$payHtml}
    </div>
</div>
HTML;

        echo admin_render_page('User Details', 'users', $body);
        exit;
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // LIST VIEW WITH SERVER-SIDE PAGINATION, SEARCH, SORTING & METRICS
    // ─────────────────────────────────────────────────────────────────────────────

    // Filters & Query Parameters
    $search = trim((string) ($_GET['q'] ?? ''));
    $statusFilter = strtolower(trim((string) ($_GET['status'] ?? 'all')));
    $roleFilter = strtolower(trim((string) ($_GET['role'] ?? 'all')));
    $statusFilter = in_array($statusFilter, ['all', 'active', 'inactive', 'suspended'], true) ? $statusFilter : 'all';
    $roleFilter = in_array($roleFilter, ['all', 'user', 'admin'], true) ? $roleFilter : 'all';

    // Sorting Allowlist
    $allowedSorts = [
        'id' => 'u.id',
        'name' => 'u.name',
        'email' => 'u.email',
        'created_at' => 'u.created_at',
        'device_count' => 'device_count',
    ];
    if ($hasRole) {
        $allowedSorts['role'] = 'u.role';
    }
    if ($hasStatus) {
        $allowedSorts['status'] = 'u.status';
    }

    $sort = (string) ($_GET['sort'] ?? 'id');
    $sortColumn = $allowedSorts[$sort] ?? 'u.id';
    $dir = strtolower((string) ($_GET['dir'] ?? 'desc')) === 'asc' ? 'ASC' : 'DESC';

    // Pagination
    $page = max(1, (int) ($_GET['page'] ?? 1));
    $perPage = 15;
    $offset = ($page - 1) * $perPage;

    // Build WHERE Clause dynamically based on available columns
    $where = [];
    $params = [];

    if ($search !== '') {
        $searchFields = ['u.name', 'u.email'];
        if ($hasUsername) $searchFields[] = 'u.username';
        if ($hasUniqueId) $searchFields[] = 'u.unique_user_id';
        
        $orClauses = array_map(fn($f) => "{$f} LIKE ?", $searchFields);
        $where[] = '(' . implode(' OR ', $orClauses) . ')';
        foreach ($searchFields as $unused) {
            $params[] = '%' . $search . '%';
        }
    }

    if ($hasStatus && $statusFilter !== 'all') {
        $where[] = 'LOWER(u.status) = ?';
        $params[] = $statusFilter;
    }

    if ($hasRole && $roleFilter !== 'all') {
        $where[] = 'LOWER(u.role) = ?';
        $params[] = $roleFilter;
    }

    $whereSql = $where ? ' WHERE ' . implode(' AND ', $where) : '';

    // 1. Total Count for Pagination
    $countStmt = $pdo->prepare('SELECT COUNT(*) FROM `users` u' . $whereSql);
    $countStmt->execute($params);
    $totalUsersCount = (int) $countStmt->fetchColumn();

    // 2. Dynamic subquery for active plan resolution
    $subQueryPlan = "NULL AS active_plan_name";
    if (!empty($subCols) && admin_table_exists($pdo, 'plans')) {
        if ($hasSubUserId) {
            $subQueryPlan = "(SELECT p.name FROM `subscriptions` s JOIN `plans` p ON s.plan_id = p.id WHERE s.user_id = u.id AND LOWER(s.status) = 'active' ORDER BY s.id DESC LIMIT 1) AS active_plan_name";
        } elseif ($hasSubDeviceId && admin_table_exists($pdo, 'devices')) {
            $subQueryPlan = "(SELECT p.name FROM `subscriptions` s JOIN `plans` p ON s.plan_id = p.id JOIN `devices` d ON (s.device_id = d.system_id OR s.device_id = d.device_uid OR s.device_id = CAST(d.id AS CHAR)) WHERE d.user_id = u.id AND LOWER(s.status) = 'active' ORDER BY s.id DESC LIMIT 1) AS active_plan_name";
        }
    }

    // 3. Fetch Aggregated User Records
    $deviceCountSubquery = admin_table_exists($pdo, 'devices')
        ? "(SELECT COUNT(*) FROM `devices` d WHERE d.user_id = u.id) AS device_count"
        : "0 AS device_count";

    $sql = "
        SELECT u.*,
               {$deviceCountSubquery},
               {$subQueryPlan}
        FROM `users` u
        {$whereSql}
        ORDER BY {$sortColumn} {$dir}
        LIMIT {$perPage} OFFSET {$offset}
    ";

    $stmt = $pdo->prepare($sql);
    $stmt->execute($params);
    $userRows = $stmt->fetchAll(PDO::FETCH_ASSOC);

    // Top KPI Metric Summary Counters
    $kpiTotal = (int) $pdo->query('SELECT COUNT(*) FROM `users`')->fetchColumn();
    $kpiActive = $hasStatus
        ? (int) $pdo->query('SELECT COUNT(*) FROM `users` WHERE LOWER(status) = "active"')->fetchColumn()
        : $kpiTotal;
    
    $kpiWithDevices = admin_table_exists($pdo, 'devices')
        ? (int) $pdo->query('SELECT COUNT(DISTINCT user_id) FROM `devices` WHERE user_id IS NOT NULL')->fetchColumn()
        : 0;

    $kpiWithActiveSub = 0;
    if (!empty($subCols)) {
        if ($hasSubUserId) {
            $kpiWithActiveSub = (int) $pdo->query('SELECT COUNT(DISTINCT user_id) FROM `subscriptions` WHERE LOWER(status) = "active"')->fetchColumn();
        } elseif ($hasSubDeviceId && admin_table_exists($pdo, 'devices')) {
            $kpiWithActiveSub = (int) $pdo->query("SELECT COUNT(DISTINCT d.user_id) FROM `subscriptions` s JOIN `devices` d ON (s.device_id = d.system_id OR s.device_id = d.device_uid OR s.device_id = CAST(d.id AS CHAR)) WHERE LOWER(s.status) = 'active'")->fetchColumn();
        }
    }

    // Build Table Rows
    $rowHtml = '';
    if (!empty($userRows)) {
        foreach ($userRows as $u) {
            $uid = (int) $u['id'];
            $uniqueId = htmlspecialchars((string) ($u['unique_user_id'] ?? '—'), ENT_QUOTES, 'UTF-8');
            $uName = htmlspecialchars((string) ($u['name'] ?? ($u['username'] ?? 'User #' . $uid)), ENT_QUOTES, 'UTF-8');
            $uEmail = htmlspecialchars((string) ($u['email'] ?? 'N/A'), ENT_QUOTES, 'UTF-8');
            $devCount = (int) ($u['device_count'] ?? 0);
            $activePlan = htmlspecialchars((string) ($u['active_plan_name'] ?? 'None'), ENT_QUOTES, 'UTF-8');
            $uRole = strtoupper((string) ($u['role'] ?? 'USER'));
            $uStatus = strtoupper((string) ($u['status'] ?? 'ACTIVE'));
            $uStatusClass = strtolower($uStatus);
            $created = admin_utc_date($u['created_at'] ?? null);
            $viewUrl = htmlspecialchars(admin_url('users.php', ['view' => $uid]), ENT_QUOTES, 'UTF-8');

            $rowHtml .= "<tr>
                <td>#{$uid}</td>
                <td><strong>{$uName}</strong></td>
                <td>{$uEmail}</td>
                <td><span class=\"status-badge\">{$uRole}</span></td>
                <td><span class=\"kpi-pill\">{$devCount}</span></td>
                <td>{$activePlan}</td>
                <td><span class=\"status-badge status-{$uStatusClass}\">{$uStatus}</span></td>
                <td>{$created}</td>
                <td><a class=\"outline-button\" href=\"{$viewUrl}\">View</a></td>
            </tr>";
        }
    } else {
        $rowHtml = '<tr><td colspan="9" class="empty-state">No matching users found for this query.</td></tr>';
    }

    // Build Filter Form & Pagination Controls
    $usersUrl = htmlspecialchars(admin_url('users.php'), ENT_QUOTES, 'UTF-8');
    $queryParams = array_filter([
        'q' => $search !== '' ? $search : null,
        'status' => ($hasStatus && $statusFilter !== 'all') ? $statusFilter : null,
        'role' => ($hasRole && $roleFilter !== 'all') ? $roleFilter : null,
        'sort' => $sort !== 'id' ? $sort : null,
        'dir' => $dir !== 'DESC' ? strtolower($dir) : null,
    ]);

    $paginationHtml = admin_pagination($totalUsersCount, $perPage, $page, 'users.php', $queryParams);

    $statusOptions = '';
    foreach (['all' => 'All Statuses', 'active' => 'Active', 'inactive' => 'Inactive', 'suspended' => 'Suspended'] as $v => $l) {
        $statusOptions .= '<option value="' . $v . '"' . ($statusFilter === $v ? ' selected' : '') . '>' . $l . '</option>';
    }

    $roleOptions = '';
    foreach (['all' => 'All Roles', 'user' => 'User', 'admin' => 'Admin'] as $v => $l) {
        $roleOptions .= '<option value="' . $v . '"' . ($roleFilter === $v ? ' selected' : '') . '>' . $l . '</option>';
    }

    $adminCsrf = htmlspecialchars(admin_csrf_token(), ENT_QUOTES, 'UTF-8');
    $searchSafe = htmlspecialchars($search, ENT_QUOTES, 'UTF-8');

    $thId = admin_sort_header('id', 'ID', $sort, $dir, 'users.php', $queryParams);
    $thName = admin_sort_header('name', 'Name', $sort, $dir, 'users.php', $queryParams);
    $thEmail = admin_sort_header('email', 'Email', $sort, $dir, 'users.php', $queryParams);
    $thRole = admin_sort_header('role', 'Role', $sort, $dir, 'users.php', $queryParams);
    $thDevices = admin_sort_header('device_count', 'Devices', $sort, $dir, 'users.php', $queryParams);
    $thStatus = admin_sort_header('status', 'Status', $sort, $dir, 'users.php', $queryParams);
    $thCreated = admin_sort_header('created_at', 'Registered', $sort, $dir, 'users.php', $queryParams);

    $body = <<<HTML
<!-- User Metrics Cards -->
<div class="card-grid">
    <div class="card">
        <span class="label">Total Registered Users</span>
        <p class="value">{$kpiTotal}</p>
        <div class="trend">All-time customer accounts</div>
    </div>
    <div class="card">
        <span class="label">Active Users</span>
        <p class="value">{$kpiActive}</p>
        <div class="trend">Accounts in active standing</div>
    </div>
    <div class="card">
        <span class="label">Users with Devices</span>
        <p class="value">{$kpiWithDevices}</p>
        <div class="trend">Have at least 1 registered device</div>
    </div>
    <div class="card">
        <span class="label">Active Subscriptions</span>
        <p class="value">{$kpiWithActiveSub}</p>
        <div class="trend">Subscribed to a paid/active plan</div>
    </div>
</div>

<div class="section" id="create-user">
    <div class="section-header"><h3>Create New User Account</h3></div>
    <form method="POST" action="{$usersUrl}" class="detail-panels" style="display:grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap:16px; align-items:end;">
        <input type="hidden" name="csrf_token" value="{$adminCsrf}">
        <input type="hidden" name="create_user" value="1">
        <label>Full Name
            <input type="text" name="name" placeholder="John Doe" required style="width:100%; border:1px solid var(--border); border-radius:8px; padding:10px; background:var(--bg-card); color:var(--text-primary);">
        </label>
        <label>Email Address
            <input type="email" name="email" placeholder="john@example.com" required style="width:100%; border:1px solid var(--border); border-radius:8px; padding:10px; background:var(--bg-card); color:var(--text-primary);">
        </label>
        <label>Initial Password
            <input type="password" name="password" placeholder="Minimum 6 characters" required minlength="6" style="width:100%; border:1px solid var(--border); border-radius:8px; padding:10px; background:var(--bg-card); color:var(--text-primary);">
        </label>
        <div>
            <button class="button" type="submit" style="height:42px; width:100%;">Create Account & Activate Welcome Plan</button>
        </div>
    </form>
    <p style="margin-top:8px; font-size:13px; color:var(--text-muted);">New customer accounts created by administrators automatically receive a 6-month Welcome Plan with full features in an atomic database transaction.</p>
</div>

<!-- Search & Filter Bar -->
<div class="filter-bar">
    <form method="GET" action="{$usersUrl}" style="display:flex; gap:10px; flex-wrap:wrap; align-items:center; width:100%;">
        <div class="filter-group">
            <input type="text" name="q" value="{$searchSafe}" placeholder="Search name, email, username..." class="filter-input">
        </div>
        <div class="filter-group">
            <select name="status" class="filter-select">
                {$statusOptions}
            </select>
        </div>
        <div class="filter-group">
            <select name="role" class="filter-select">
                {$roleOptions}
            </select>
        </div>
        <button type="submit" class="filter-btn">Apply Filters</button>
        <a href="{$usersUrl}" class="filter-reset">Reset</a>
    </form>
</div>

<!-- Users Table -->
<div class="section">
    <div class="section-header">
        <h3>User Accounts</h3>
        <span class="kpi-pill">{$totalUsersCount} Total Results</span>
    </div>
    <div style="overflow:auto;">
        <table>
            <thead>
                <tr>
                    <th>{$thId}</th>
                    <th>{$thName}</th>
                    <th>{$thEmail}</th>
                    <th>{$thRole}</th>
                    <th>{$thDevices}</th>
                    <th>Active Plan</th>
                    <th>{$thStatus}</th>
                    <th>{$thCreated}</th>
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

    echo admin_render_page('Users', 'users', $body);

} catch (Throwable $e) {
    admin_log_error($e, 'users.php execution');
    echo admin_render_page('Users', 'users', admin_safe_error_html('Unexpected Error', 'An error occurred while loading the Users page.', $e));
}
