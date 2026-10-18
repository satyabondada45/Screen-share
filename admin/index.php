<?php
require_once __DIR__ . '/includes/db.php';
require_once __DIR__ . '/includes/auth.php';
require_once __DIR__ . '/includes/layout.php';

admin_require_auth();

try {
    $pdo = admin_db();
} catch (Throwable $e) {
    error_log('Admin database initialization failed: ' . $e->getMessage());
    echo admin_render_page('System Error', 'error', '<div class="flash error">Unable to connect to the database. Please check system configuration.</div>');
    exit;
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. CALCULATE COMPREHENSIVE DYNAMIC METRICS FROM REAL DATABASE
// ─────────────────────────────────────────────────────────────────────────────

$stats = [
    // Users
    'total_users' => 0,
    'active_users' => 0,
    // Devices
    'total_devices' => 0,
    'online_devices' => 0,
    'offline_devices' => 0,
    // Sessions
    'total_sessions' => 0,
    'active_sessions' => 0,
    'total_bandwidth_mb' => 0.0,
    // Connections
    'total_connection_requests' => 0,
    'accepted_connections' => 0,
    'pending_connections' => 0,
    // Plans & Subscriptions
    'total_plans' => 0,
    'total_subscriptions' => 0,
    'active_subscriptions' => 0,
    'expired_subscriptions' => 0,
    'expiring_soon' => 0,
    // Payments & Revenue
    'total_revenue' => 0.0,
    'completed_payments' => 0,
    'pending_payments' => 0,
    'failed_payments' => 0,
    // Support
    'total_tickets' => 0,
    'open_tickets' => 0,
];

try {
    // Users metrics
    if (admin_table_exists($pdo, 'users')) {
        $stats['total_users'] = (int) $pdo->query('SELECT COUNT(*) FROM `users`')->fetchColumn();
        if (admin_column_exists($pdo, 'users', 'status')) {
            $stats['active_users'] = (int) $pdo->query('SELECT COUNT(*) FROM `users` WHERE LOWER(`status`) = "active"')->fetchColumn();
        } else {
            $stats['active_users'] = $stats['total_users'];
        }
    }

    // Devices metrics
    if (admin_table_exists($pdo, 'devices')) {
        $stats['total_devices'] = (int) $pdo->query('SELECT COUNT(*) FROM `devices`')->fetchColumn();
        if (admin_column_exists($pdo, 'devices', 'is_online')) {
            $stats['online_devices'] = (int) $pdo->query('SELECT COUNT(*) FROM `devices` WHERE `is_online` = 1')->fetchColumn();
            $stats['offline_devices'] = max(0, $stats['total_devices'] - $stats['online_devices']);
        } elseif (admin_column_exists($pdo, 'devices', 'status')) {
            $stats['online_devices'] = (int) $pdo->query('SELECT COUNT(*) FROM `devices` WHERE LOWER(`status`) IN ("active", "online")')->fetchColumn();
            $stats['offline_devices'] = max(0, $stats['total_devices'] - $stats['online_devices']);
        }
    }

    // Sessions metrics
    if (admin_table_exists($pdo, 'sessions')) {
        $stats['total_sessions'] = (int) $pdo->query('SELECT COUNT(*) FROM `sessions`')->fetchColumn();
        if (admin_column_exists($pdo, 'sessions', 'status')) {
            $stats['active_sessions'] = (int) $pdo->query('SELECT COUNT(*) FROM `sessions` WHERE LOWER(`status`) = "active"')->fetchColumn();
        }
        if (admin_column_exists($pdo, 'sessions', 'bandwidth_used_mb')) {
            $stats['total_bandwidth_mb'] = (float) $pdo->query('SELECT COALESCE(SUM(`bandwidth_used_mb`), 0) FROM `sessions`')->fetchColumn();
        }
    }

    // Connection requests metrics
    if (admin_table_exists($pdo, 'connection_requests')) {
        $stats['total_connection_requests'] = (int) $pdo->query('SELECT COUNT(*) FROM `connection_requests`')->fetchColumn();
        if (admin_column_exists($pdo, 'connection_requests', 'status')) {
            $stats['accepted_connections'] = (int) $pdo->query('SELECT COUNT(*) FROM `connection_requests` WHERE LOWER(`status`) = "accepted"')->fetchColumn();
            $stats['pending_connections'] = (int) $pdo->query('SELECT COUNT(*) FROM `connection_requests` WHERE LOWER(`status`) = "pending"')->fetchColumn();
        }
    } elseif (admin_table_exists($pdo, 'connections')) {
        $stats['total_connection_requests'] = (int) $pdo->query('SELECT COUNT(*) FROM `connections`')->fetchColumn();
        if (admin_column_exists($pdo, 'connections', 'status')) {
            $stats['accepted_connections'] = (int) $pdo->query('SELECT COUNT(*) FROM `connections` WHERE LOWER(`status`) = "accepted"')->fetchColumn();
            $stats['pending_connections'] = (int) $pdo->query('SELECT COUNT(*) FROM `connections` WHERE LOWER(`status`) IN ("pending", "requested")')->fetchColumn();
        }
    }

    // Plans metrics
    if (admin_table_exists($pdo, 'plans')) {
        $stats['total_plans'] = (int) $pdo->query('SELECT COUNT(*) FROM `plans` WHERE `status` = "active"')->fetchColumn();
    }

    // Subscriptions metrics (correct column: expires_at)
    if (admin_table_exists($pdo, 'subscriptions')) {
        $stats['total_subscriptions'] = (int) $pdo->query('SELECT COUNT(*) FROM `subscriptions`')->fetchColumn();
        if (admin_column_exists($pdo, 'subscriptions', 'status')) {
            $stats['active_subscriptions'] = (int) $pdo->query('SELECT COUNT(*) FROM `subscriptions` WHERE LOWER(`status`) = "active"')->fetchColumn();
        }
        
        $expCol = admin_column_exists($pdo, 'subscriptions', 'expires_at') ? 'expires_at' : (admin_column_exists($pdo, 'subscriptions', 'expiry_date') ? 'expiry_date' : null);
        if ($expCol) {
            $stats['expired_subscriptions'] = (int) $pdo->query("SELECT COUNT(*) FROM `subscriptions` WHERE `{$expCol}` IS NOT NULL AND `{$expCol}` < NOW()")->fetchColumn();
            $stats['expiring_soon'] = (int) $pdo->query("SELECT COUNT(*) FROM `subscriptions` WHERE `{$expCol}` IS NOT NULL AND `{$expCol}` BETWEEN NOW() AND DATE_ADD(NOW(), INTERVAL 7 DAY)")->fetchColumn();
        }
    }

    // Payments & Revenue metrics
    if (admin_table_exists($pdo, 'payments')) {
        $statusCol = admin_column_exists($pdo, 'payments', 'status') ? 'status' : null;
        if ($statusCol) {
            $stats['total_revenue'] = (float) $pdo->query("SELECT COALESCE(SUM(amount), 0) FROM `payments` WHERE LOWER(`{$statusCol}`) IN ('completed', 'paid', 'successful')")->fetchColumn();
            $stats['completed_payments'] = (int) $pdo->query("SELECT COUNT(*) FROM `payments` WHERE LOWER(`{$statusCol}`) IN ('completed', 'paid', 'successful')")->fetchColumn();
            $stats['pending_payments'] = (int) $pdo->query("SELECT COUNT(*) FROM `payments` WHERE LOWER(`{$statusCol}`) = 'pending'")->fetchColumn();
            $stats['failed_payments'] = (int) $pdo->query("SELECT COUNT(*) FROM `payments` WHERE LOWER(`{$statusCol}`) IN ('failed', 'refunded')")->fetchColumn();
        } else {
            $stats['total_revenue'] = (float) $pdo->query("SELECT COALESCE(SUM(amount), 0) FROM `payments`")->fetchColumn();
            $stats['completed_payments'] = (int) $pdo->query("SELECT COUNT(*) FROM `payments`")->fetchColumn();
        }
    }

    // Support tickets metrics
    if (admin_table_exists($pdo, 'support_tickets')) {
        $stats['total_tickets'] = (int) $pdo->query('SELECT COUNT(*) FROM `support_tickets`')->fetchColumn();
        if (admin_column_exists($pdo, 'support_tickets', 'status')) {
            $stats['open_tickets'] = (int) $pdo->query('SELECT COUNT(*) FROM `support_tickets` WHERE LOWER(`status`) = "open"')->fetchColumn();
        }
    }
} catch (Throwable $e) {
    error_log('Dashboard metrics calculation error: ' . $e->getMessage());
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. QUERY REAL RECENT DEVICES (JOIN OWNER USER)
// ─────────────────────────────────────────────────────────────────────────────

$recentDevices = [];
if (admin_table_exists($pdo, 'devices')) {
    try {
        $sql = "
            SELECT d.*, 
                   u.name AS owner_name, 
                   u.email AS owner_email,
                   u.username AS owner_username
            FROM `devices` d
            LEFT JOIN `users` u ON d.user_id = u.id
            ORDER BY d.id DESC
            LIMIT 5
        ";
        $recentDevices = $pdo->query($sql)->fetchAll(PDO::FETCH_ASSOC);
    } catch (Throwable $e) {
        error_log('Recent devices query error: ' . $e->getMessage());
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ─────────────────────────────────────────────────────────────────────────────
// 3. QUERY REAL RECENT SUBSCRIPTIONS (JOIN USER & PLAN)
// ─────────────────────────────────────────────────────────────────────────────

$recentSubscriptions = [];
if (admin_table_exists($pdo, 'subscriptions')) {
    try {
        $subCols = admin_table_columns($pdo, 'subscriptions');
        $hasSubUserId = in_array('user_id', $subCols, true);
        $hasSubDeviceId = in_array('device_id', $subCols, true);
        $planCols = admin_table_exists($pdo, 'plans') ? admin_table_columns($pdo, 'plans') : [];
        $hasPlanUnit = in_array('duration_unit', $planCols, true);
        $unitSelect = $hasPlanUnit ? 'p.duration_unit' : '"days" AS duration_unit';
        $priceSelect = in_array('price', $planCols, true) ? 'p.price AS plan_price' : 's.amount AS plan_price';

        $userJoin = '';
        if ($hasSubUserId && admin_table_exists($pdo, 'users')) {
            $userJoin = 'LEFT JOIN `users` u ON s.user_id = u.id';
        } elseif ($hasSubDeviceId && admin_table_exists($pdo, 'devices') && admin_table_exists($pdo, 'users')) {
            $userJoin = 'LEFT JOIN `devices` d ON (s.device_id = d.system_id OR s.device_id = d.device_uid OR s.device_id = CAST(d.id AS CHAR))
                         LEFT JOIN `users` u ON d.user_id = u.id';
        }

        $sql = "
            SELECT s.*, 
                   u.name AS user_name, 
                   u.email AS user_email,
                   p.name AS plan_name,
                   {$priceSelect},
                   {$unitSelect}
            FROM `subscriptions` s
            {$userJoin}
            LEFT JOIN `plans` p ON s.plan_id = p.id
            GROUP BY s.id
            ORDER BY s.id DESC
            LIMIT 5
        ";
        $recentSubscriptions = $pdo->query($sql)->fetchAll(PDO::FETCH_ASSOC);
    } catch (Throwable $e) {
        admin_log_error($e, 'Recent subscriptions query error');
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. QUERY REAL SUBSCRIPTIONS EXPIRING SOON (NEXT 7 DAYS)
// ─────────────────────────────────────────────────────────────────────────────

$expiringSoonRows = [];
if (admin_table_exists($pdo, 'subscriptions')) {
    try {
        $expCol = admin_column_exists($pdo, 'subscriptions', 'expires_at') ? 'expires_at' : (admin_column_exists($pdo, 'subscriptions', 'expiry_date') ? 'expiry_date' : null);
        if ($expCol) {
            $subCols = admin_table_columns($pdo, 'subscriptions');
            $hasSubUserId = in_array('user_id', $subCols, true);
            $hasSubDeviceId = in_array('device_id', $subCols, true);
            $userJoin = '';
            if ($hasSubUserId && admin_table_exists($pdo, 'users')) {
                $userJoin = 'LEFT JOIN `users` u ON s.user_id = u.id';
            } elseif ($hasSubDeviceId && admin_table_exists($pdo, 'devices') && admin_table_exists($pdo, 'users')) {
                $userJoin = 'LEFT JOIN `devices` d ON (s.device_id = d.system_id OR s.device_id = d.device_uid OR s.device_id = CAST(d.id AS CHAR))
                             LEFT JOIN `users` u ON d.user_id = u.id';
            }

            $sql = "
                SELECT s.*, 
                       u.name AS user_name, 
                       u.email AS user_email,
                       p.name AS plan_name
                FROM `subscriptions` s
                {$userJoin}
                LEFT JOIN `plans` p ON s.plan_id = p.id
                WHERE s.`{$expCol}` IS NOT NULL 
                  AND s.`{$expCol}` BETWEEN NOW() AND DATE_ADD(NOW(), INTERVAL 7 DAY)
                GROUP BY s.id
                ORDER BY s.`{$expCol}` ASC
                LIMIT 5
            ";
            $expiringSoonRows = $pdo->query($sql)->fetchAll(PDO::FETCH_ASSOC);
        }
    } catch (Throwable $e) {
        admin_log_error($e, 'Expiring soon query error');
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. QUERY REAL RECENT SESSIONS (JOIN HOST DEVICE & OWNER)
// ─────────────────────────────────────────────────────────────────────────────

$recentSessions = [];
if (admin_table_exists($pdo, 'sessions')) {
    try {
        $sql = "
            SELECT s.*, 
                   d.name AS host_device_name,
                   d.system_id AS host_system_id,
                   u.name AS host_user_name
            FROM `sessions` s
            LEFT JOIN `devices` d ON s.host_device_id = d.id
            LEFT JOIN `users` u ON d.user_id = u.id
            ORDER BY s.id DESC
            LIMIT 5
        ";
        $recentSessions = $pdo->query($sql)->fetchAll(PDO::FETCH_ASSOC);
    } catch (Throwable $e) {
        error_log('Recent sessions query error: ' . $e->getMessage());
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 6. REAL CHART DATA AGGREGATION (DATABASE-DRIVEN)
// ─────────────────────────────────────────────────────────────────────────────

// Chart A: Device Distribution (Online vs Offline)
$totalDev = max(1, $stats['total_devices']);
$onlinePercent = round(($stats['online_devices'] / $totalDev) * 100);
$offlinePercent = 100 - $onlinePercent;

// Chart B: 7-Day Session Activity (count sessions grouped by date)
$dailySessions = [];
for ($i = 6; $i >= 0; $i--) {
    $dateKey = date('Y-m-d', strtotime("-$i days"));
    $displayLabel = date('D', strtotime("-$i days"));
    $dailySessions[$dateKey] = [
        'label' => $displayLabel,
        'date' => $dateKey,
        'count' => 0,
    ];
}
if (admin_table_exists($pdo, 'sessions') && admin_column_exists($pdo, 'sessions', 'started_at')) {
    try {
        $startDate = date('Y-m-d 00:00:00', strtotime('-6 days'));
        $stmt = $pdo->prepare("
            SELECT DATE(started_at) AS session_date, COUNT(*) AS count
            FROM `sessions`
            WHERE started_at >= ?
            GROUP BY DATE(started_at)
        ");
        $stmt->execute([$startDate]);
        $rows = $stmt->fetchAll(PDO::FETCH_ASSOC);
        foreach ($rows as $r) {
            if (isset($dailySessions[$r['session_date']])) {
                $dailySessions[$r['session_date']]['count'] = (int) $r['count'];
            }
        }
    } catch (Throwable $e) {
        error_log('Chart 7-day session error: ' . $e->getMessage());
    }
}

// Chart C: Subscription Distribution by Plan
$planDistribution = [];
if (admin_table_exists($pdo, 'plans')) {
    try {
        $stmt = $pdo->query("
            SELECT p.name AS plan_name, COUNT(s.id) AS sub_count
            FROM `plans` p
            LEFT JOIN `subscriptions` s ON s.plan_id = p.id AND s.status = 'active'
            GROUP BY p.id, p.name
            ORDER BY sub_count DESC
            LIMIT 5
        ");
        $planDistribution = $stmt->fetchAll(PDO::FETCH_ASSOC);
    } catch (Throwable $e) {
        error_log('Chart plan distribution error: ' . $e->getMessage());
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 7. BUILD HTML MARKUP FOR TABLES & CHARTS
// ─────────────────────────────────────────────────────────────────────────────

$recentDevicesMarkup = '';
if (!empty($recentDevices)) {
    foreach ($recentDevices as $dev) {
        $id = (int) $dev['id'];
        $sysId = htmlspecialchars((string) ($dev['system_id'] ?? ($dev['device_uid'] ?? 'ID#' . $id)), ENT_QUOTES, 'UTF-8');
        $name = htmlspecialchars((string) ($dev['name'] ?? 'Unnamed device'), ENT_QUOTES, 'UTF-8');
        $owner = htmlspecialchars((string) ($dev['owner_name'] ?? ($dev['owner_email'] ?? 'Unassigned')), ENT_QUOTES, 'UTF-8');
        $isOnline = !empty($dev['is_online']);
        $statusBadge = $isOnline 
            ? '<span class="status-badge status-active">Online</span>' 
            : '<span class="status-badge status-inactive">Offline</span>';
        $os = htmlspecialchars((string) ($dev['os_type'] ?? 'Windows'), ENT_QUOTES, 'UTF-8');
        $lastSeen = admin_time_ago($dev['last_seen_at'] ?? null);
        $viewUrl = htmlspecialchars(admin_url('devices.php', ['view' => $id]), ENT_QUOTES, 'UTF-8');

        $recentDevicesMarkup .= "<tr>
            <td><code>{$sysId}</code></td>
            <td><strong>{$name}</strong></td>
            <td>{$owner}</td>
            <td>{$statusBadge}</td>
            <td>{$os}</td>
            <td><small>{$lastSeen}</small></td>
            <td><a class=\"outline-button\" href=\"{$viewUrl}\">View</a></td>
        </tr>";
    }
} else {
    $recentDevicesMarkup = '<tr><td colspan="7" class="empty-state">No remote devices registered yet.</td></tr>';
}

$recentSubscriptionsMarkup = '';
if (!empty($recentSubscriptions)) {
    foreach ($recentSubscriptions as $sub) {
        $subId = (int) $sub['id'];
        $user = htmlspecialchars((string) ($sub['user_name'] ?? ($sub['user_email'] ?? ($sub['device_id'] ?? 'User #' . ($sub['user_id'] ?? '')))), ENT_QUOTES, 'UTF-8');
        $plan = htmlspecialchars((string) ($sub['plan_name'] ?? 'Plan #' . ($sub['plan_id'] ?? '')), ENT_QUOTES, 'UTF-8');
        $startVal = $sub['starts_at'] ?? ($sub['start_date'] ?? null);
        $start = !empty($startVal) ? admin_utc_date((string)$startVal) : 'N/A';
        $expVal = $sub['expires_at'] ?? ($sub['expiry_date'] ?? null);
        if (empty($expVal) || ($sub['duration_unit'] ?? '') === 'unlimited') {
            $expiry = '<span style="color:#16a34a; font-weight:600;">Unlimited</span>';
        } else {
            $expiry = htmlspecialchars(admin_utc_date((string)$expVal), ENT_QUOTES, 'UTF-8');
        }
        $statusVal = strtoupper((string) ($sub['status'] ?? 'ACTIVE'));
        $statusClass = strtolower($statusVal);

        $recentSubscriptionsMarkup .= "<tr>
            <td>#{$subId}</td>
            <td>{$user}</td>
            <td><strong>{$plan}</strong></td>
            <td>{$start}</td>
            <td>{$expiry}</td>
            <td><span class=\"status-badge status-{$statusClass}\">{$statusVal}</span></td>
        </tr>";
    }
} else {
    $recentSubscriptionsMarkup = '<tr><td colspan="6" class="empty-state">No active or past subscriptions found.</td></tr>';
}

$recentSessionsMarkup = '';
if (!empty($recentSessions)) {
    foreach ($recentSessions as $sess) {
        $sessId = (int) $sess['id'];
        $sessionKey = htmlspecialchars((string) ($sess['session_key'] ?? 'KEY#' . $sessId), ENT_QUOTES, 'UTF-8');
        $device = htmlspecialchars((string) ($sess['host_device_name'] ?? 'Device #' . ($sess['host_device_id'] ?? '')), ENT_QUOTES, 'UTF-8');
        $user = htmlspecialchars((string) ($sess['host_user_name'] ?? '—'), ENT_QUOTES, 'UTF-8');
        $status = strtoupper((string) ($sess['status'] ?? 'COMPLETED'));
        $statusClass = strtolower($status);
        $duration = admin_format_duration($sess['started_at'] ?? null, $sess['ended_at'] ?? null);
        $started = admin_time_ago($sess['started_at'] ?? null);

        $recentSessionsMarkup .= "<tr>
            <td><code>{$sessionKey}</code></td>
            <td>{$device}</td>
            <td>{$user}</td>
            <td><span class=\"status-badge status-{$statusClass}\">{$status}</span></td>
            <td>{$duration}</td>
            <td><small>{$started}</small></td>
        </tr>";
    }
} else {
    $recentSessionsMarkup = '<tr><td colspan="6" class="empty-state">No remote sessions recorded yet.</td></tr>';
}

$expiringMarkup = '';
if (!empty($expiringSoonRows)) {
    foreach ($expiringSoonRows as $row) {
        $user = htmlspecialchars((string) ($row['user_name'] ?? ($row['user_email'] ?? ($row['device_id'] ?? 'User #' . ($row['user_id'] ?? '')))), ENT_QUOTES, 'UTF-8');
        $plan = htmlspecialchars((string) ($row['plan_name'] ?? 'Plan #' . ($row['plan_id'] ?? '')), ENT_QUOTES, 'UTF-8');
        $expVal = $row['expires_at'] ?? ($row['expiry_date'] ?? null);
        $expDate = !empty($expVal) ? htmlspecialchars(admin_utc_date((string)$expVal), ENT_QUOTES, 'UTF-8') : 'N/A';
        $expiringMarkup .= "<li style=\"margin-bottom:8px;\"><strong>{$user}</strong> ({$plan}) &mdash; <span style=\"color:#b45309; font-weight:600;\">Expires: {$expDate}</span></li>";
    }
} else {
    $expiringMarkup = '<li class="muted">No subscriptions expiring in the next 7 days.</li>';
}

// SVG Chart 1: 7-Day Session Activity Bar Chart
$maxSessionsCount = 1;
foreach ($dailySessions as $ds) {
    if ($ds['count'] > $maxSessionsCount) $maxSessionsCount = $ds['count'];
}
$barChartSvg = '<svg viewBox="0 0 350 140" class="chart-svg" xmlns="http://www.w3.org/2000/svg">';
$barChartSvg .= '<line x1="30" y1="110" x2="340" y2="110" stroke="#e3e7ef" stroke-width="1"/>';
$barChartSvg .= '<line x1="30" y1="60" x2="340" y2="60" stroke="#f0f2f6" stroke-width="1" stroke-dasharray="3"/>';
$barChartSvg .= '<text x="10" y="113" fill="#8b95a6" font-size="9">0</text>';
$barChartSvg .= '<text x="10" y="63" fill="#8b95a6" font-size="9">' . ceil($maxSessionsCount / 2) . '</text>';
$barChartSvg .= '<text x="10" y="15" fill="#8b95a6" font-size="9">' . $maxSessionsCount . '</text>';

$barIndex = 0;
$barWidth = 24;
$barGap = 42;
$startX = 45;
foreach ($dailySessions as $ds) {
    $x = $startX + ($barIndex * $barGap);
    $height = round(($ds['count'] / $maxSessionsCount) * 85);
    $y = 110 - $height;
    $barFill = ($ds['count'] > 0) ? 'url(#barGradient)' : '#e5e7eb';
    $barChartSvg .= '<rect x="' . $x . '" y="' . $y . '" width="' . $barWidth . '" height="' . max(2, $height) . '" rx="4" fill="' . $barFill . '"/>';
    $barChartSvg .= '<text x="' . ($x + ($barWidth / 2)) . '" y="' . ($y - 4) . '" fill="#4b5563" font-size="9" font-weight="600" text-anchor="middle">' . $ds['count'] . '</text>';
    $barChartSvg .= '<text x="' . ($x + ($barWidth / 2)) . '" y="125" fill="#8b95a6" font-size="9" text-anchor="middle">' . $ds['label'] . '</text>';
    $barIndex++;
}
$barChartSvg .= '<defs><linearGradient id="barGradient" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stop-color="#e53935"/><stop offset="100%" stop-color="#c0392b"/></linearGradient></defs>';
$barChartSvg .= '</svg>';

// SVG Chart 2: Device Health Donut Chart
$donutRadius = 38;
$circumference = 2 * M_PI * $donutRadius;
$onlineOffset = $circumference - (($onlinePercent / 100) * $circumference);
$deviceChartSvg = '
<svg viewBox="0 0 200 140" class="chart-svg" xmlns="http://www.w3.org/2000/svg">
    <circle cx="70" cy="70" r="' . $donutRadius . '" fill="transparent" stroke="#edf0f5" stroke-width="14"/>
    <circle cx="70" cy="70" r="' . $donutRadius . '" fill="transparent" stroke="#16a34a" stroke-width="14"
            stroke-dasharray="' . $circumference . '" stroke-dashoffset="' . $onlineOffset . '"
            stroke-linecap="round" transform="rotate(-90 70 70)"/>
    <text x="70" y="66" text-anchor="middle" font-size="18" font-weight="700" fill="#0d1117">' . $onlinePercent . '%</text>
    <text x="70" y="80" text-anchor="middle" font-size="8" font-weight="600" fill="#8b95a6" letter-spacing="0.05em">ONLINE</text>
    <g transform="translate(130, 45)">
        <circle cx="5" cy="5" r="4" fill="#16a34a"/>
        <text x="14" y="8" font-size="10" font-weight="600" fill="#0d1117">Online: ' . $stats['online_devices'] . '</text>
        <circle cx="5" cy="25" r="4" fill="#9ca3af"/>
        <text x="14" y="28" font-size="10" font-weight="600" fill="#0d1117">Offline: ' . $stats['offline_devices'] . '</text>
        <text x="14" y="46" font-size="9" fill="#8b95a6">Total: ' . $stats['total_devices'] . '</text>
    </g>
</svg>';

// URLs
$devicesUrl = htmlspecialchars(admin_url('devices.php'), ENT_QUOTES, 'UTF-8');
$usersUrl = htmlspecialchars(admin_url('users.php'), ENT_QUOTES, 'UTF-8');
$sessionsUrl = htmlspecialchars(admin_url('sessions.php'), ENT_QUOTES, 'UTF-8');
$subscriptionsUrl = htmlspecialchars(admin_url('subscriptions.php'), ENT_QUOTES, 'UTF-8');
$paymentsUrl = htmlspecialchars(admin_url('payments.php'), ENT_QUOTES, 'UTF-8');
$connectionsUrl = htmlspecialchars(admin_url('connections.php'), ENT_QUOTES, 'UTF-8');
$reportsUrl = htmlspecialchars(admin_url('reports.php'), ENT_QUOTES, 'UTF-8');

$formattedRevenue = '₹' . number_format($stats['total_revenue'], 2);

$body = <<<HTML
<!-- Top Metric Stat Cards -->
<div class="card-grid">
    <div class="card">
        <span class="label">Total Registered Users</span>
        <p class="value">{$stats['total_users']}</p>
        <div class="trend">{$stats['active_users']} active user accounts</div>
    </div>
    <div class="card">
        <span class="label">Remote Devices</span>
        <p class="value">{$stats['total_devices']}</p>
        <div class="trend"><span style="color:#16a34a; font-weight:600;">{$stats['online_devices']} Online</span> &bull; {$stats['offline_devices']} Offline</div>
    </div>
    <div class="card">
        <span class="label">Remote Sessions</span>
        <p class="value">{$stats['total_sessions']}</p>
        <div class="trend"><span style="color:#e53935; font-weight:600;">{$stats['active_sessions']} Active Now</span></div>
    </div>
    <div class="card">
        <span class="label">Active Subscriptions</span>
        <p class="value">{$stats['active_subscriptions']}</p>
        <div class="trend">{$stats['expired_subscriptions']} expired &bull; {$stats['expiring_soon']} expiring soon</div>
    </div>
    <div class="card">
        <span class="label">Total Revenue</span>
        <p class="value">{$formattedRevenue}</p>
        <div class="trend">{$stats['completed_payments']} completed payments</div>
    </div>
    <div class="card">
        <span class="label">Connection Requests</span>
        <p class="value">{$stats['total_connection_requests']}</p>
        <div class="trend">{$stats['accepted_connections']} accepted &bull; {$stats['pending_connections']} pending</div>
    </div>
</div>

<!-- Interactive Analytics Charts Grid -->
<div class="chart-grid">
    <div class="chart-card">
        <div class="chart-card-header">
            <div>
                <h4 class="chart-title">7-Day Remote Session Activity</h4>
                <div class="chart-subtitle">Daily remote desktop session volume</div>
            </div>
            <a class="outline-button" style="padding:4px 8px; font-size:0.75rem;" href="{$sessionsUrl}">All Sessions</a>
        </div>
        <div class="chart-container">
            {$barChartSvg}
        </div>
    </div>

    <div class="chart-card">
        <div class="chart-card-header">
            <div>
                <h4 class="chart-title">Device Fleet Availability</h4>
                <div class="chart-subtitle">Live online ratio across all registered devices</div>
            </div>
            <a class="outline-button" style="padding:4px 8px; font-size:0.75rem;" href="{$devicesUrl}">View Fleet</a>
        </div>
        <div class="chart-container">
            {$deviceChartSvg}
        </div>
    </div>
</div>

<!-- Recent Devices Section -->
<div class="section">
    <div class="section-header">
        <h3>Recent Remote Devices</h3>
        <div class="mini-actions"><a class="button" href="{$devicesUrl}">View all devices</a></div>
    </div>
    <div style="overflow:auto;">
        <table>
            <thead>
                <tr>
                    <th>System ID</th>
                    <th>Device Name</th>
                    <th>Owner</th>
                    <th>Status</th>
                    <th>OS</th>
                    <th>Last Seen</th>
                    <th>Actions</th>
                </tr>
            </thead>
            <tbody>
                {$recentDevicesMarkup}
            </tbody>
        </table>
    </div>
</div>

<!-- Recent Sessions Section -->
<div class="section">
    <div class="section-header">
        <h3>Recent Remote Sessions</h3>
        <div class="mini-actions"><a class="button" href="{$sessionsUrl}">View all sessions</a></div>
    </div>
    <div style="overflow:auto;">
        <table>
            <thead>
                <tr>
                    <th>Session Key</th>
                    <th>Host Device</th>
                    <th>Host User</th>
                    <th>Status</th>
                    <th>Duration</th>
                    <th>Started</th>
                </tr>
            </thead>
            <tbody>
                {$recentSessionsMarkup}
            </tbody>
        </table>
    </div>
</div>

<!-- Subscriptions & Expiring Soon Row -->
<div style="display:grid; grid-template-columns: repeat(auto-fit, minmax(320px, 1fr)); gap:20px;">
    <div class="section" style="margin-bottom:0;">
        <div class="section-header">
            <h3>Recent Subscriptions</h3>
            <div class="mini-actions"><a class="outline-button" href="{$subscriptionsUrl}">All Subscriptions</a></div>
        </div>
        <div style="overflow:auto;">
            <table>
                <thead>
                    <tr>
                        <th>ID</th>
                        <th>User</th>
                        <th>Plan</th>
                        <th>Started</th>
                        <th>Expires</th>
                        <th>Status</th>
                    </tr>
                </thead>
                <tbody>
                    {$recentSubscriptionsMarkup}
                </tbody>
            </table>
        </div>
    </div>

    <div class="section" style="margin-bottom:0;">
        <div class="section-header">
            <h3>Subscriptions Expiring Soon (Next 7 Days)</h3>
        </div>
        <div class="detail-panels" style="padding:16px;">
            <ul style="margin:0; padding-left:18px; line-height:1.8;">
                {$expiringMarkup}
            </ul>
        </div>
    </div>
</div>
HTML;

echo admin_render_page('Dashboard', 'dashboard', $body);
