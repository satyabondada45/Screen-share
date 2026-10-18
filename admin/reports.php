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

// ─────────────────────────────────────────────────────────────────────────────
// 1. COMPREHENSIVE BACKEND DATA AGGREGATION
// ─────────────────────────────────────────────────────────────────────────────

$stats = [
    'users_total' => 0,
    'users_active' => 0,
    'devices_total' => 0,
    'devices_online' => 0,
    'devices_offline' => 0,
    'sessions_total' => 0,
    'sessions_active' => 0,
    'sessions_completed' => 0,
    'bandwidth_mb' => 0.0,
    'conn_total' => 0,
    'conn_accepted' => 0,
    'conn_pending' => 0,
    'conn_expired' => 0,
    'subs_total' => 0,
    'subs_active' => 0,
    'subs_expired' => 0,
    'revenue_total' => 0.0,
    'payments_completed' => 0,
    'payments_pending' => 0,
    'payments_failed' => 0,
];

try {
    // Users
    if (admin_table_exists($pdo, 'users')) {
        $stats['users_total'] = (int) $pdo->query('SELECT COUNT(*) FROM `users`')->fetchColumn();
        if (admin_column_exists($pdo, 'users', 'status')) {
            $stats['users_active'] = (int) $pdo->query('SELECT COUNT(*) FROM `users` WHERE LOWER(status) = "active"')->fetchColumn();
        } else {
            $stats['users_active'] = $stats['users_total'];
        }
    }

    // Devices
    if (admin_table_exists($pdo, 'devices')) {
        $stats['devices_total'] = (int) $pdo->query('SELECT COUNT(*) FROM `devices`')->fetchColumn();
        if (admin_column_exists($pdo, 'devices', 'is_online')) {
            $stats['devices_online'] = (int) $pdo->query('SELECT COUNT(*) FROM `devices` WHERE `is_online` = 1')->fetchColumn();
            $stats['devices_offline'] = max(0, $stats['devices_total'] - $stats['devices_online']);
        }
    }

    // Sessions
    if (admin_table_exists($pdo, 'sessions')) {
        $stats['sessions_total'] = (int) $pdo->query('SELECT COUNT(*) FROM `sessions`')->fetchColumn();
        if (admin_column_exists($pdo, 'sessions', 'status')) {
            $stats['sessions_active'] = (int) $pdo->query('SELECT COUNT(*) FROM `sessions` WHERE LOWER(status) = "active"')->fetchColumn();
            $stats['sessions_completed'] = (int) $pdo->query('SELECT COUNT(*) FROM `sessions` WHERE LOWER(status) = "completed"')->fetchColumn();
        }
        if (admin_column_exists($pdo, 'sessions', 'bandwidth_used_mb')) {
            $stats['bandwidth_mb'] = (float) $pdo->query('SELECT COALESCE(SUM(bandwidth_used_mb), 0) FROM `sessions`')->fetchColumn();
        }
    }

    // Connection Requests
    if (admin_table_exists($pdo, 'connection_requests')) {
        $stats['conn_total'] = (int) $pdo->query('SELECT COUNT(*) FROM `connection_requests`')->fetchColumn();
        if (admin_column_exists($pdo, 'connection_requests', 'status')) {
            $stats['conn_accepted'] = (int) $pdo->query('SELECT COUNT(*) FROM `connection_requests` WHERE LOWER(status) = "accepted"')->fetchColumn();
            $stats['conn_pending'] = (int) $pdo->query('SELECT COUNT(*) FROM `connection_requests` WHERE LOWER(status) = "pending"')->fetchColumn();
            $stats['conn_expired'] = (int) $pdo->query('SELECT COUNT(*) FROM `connection_requests` WHERE LOWER(status) IN ("expired", "rejected", "cancelled")')->fetchColumn();
        }
    } elseif (admin_table_exists($pdo, 'connections')) {
        $stats['conn_total'] = (int) $pdo->query('SELECT COUNT(*) FROM `connections`')->fetchColumn();
        if (admin_column_exists($pdo, 'connections', 'status')) {
            $stats['conn_accepted'] = (int) $pdo->query('SELECT COUNT(*) FROM `connections` WHERE LOWER(status) = "accepted"')->fetchColumn();
            $stats['conn_pending'] = (int) $pdo->query('SELECT COUNT(*) FROM `connections` WHERE LOWER(status) IN ("pending", "requested")')->fetchColumn();
            $stats['conn_expired'] = (int) $pdo->query('SELECT COUNT(*) FROM `connections` WHERE LOWER(status) IN ("rejected", "disconnected")')->fetchColumn();
        }
    }

    // Subscriptions
    if (admin_table_exists($pdo, 'subscriptions')) {
        $stats['subs_total'] = (int) $pdo->query('SELECT COUNT(*) FROM `subscriptions`')->fetchColumn();
        if (admin_column_exists($pdo, 'subscriptions', 'status')) {
            $stats['subs_active'] = (int) $pdo->query('SELECT COUNT(*) FROM `subscriptions` WHERE LOWER(status) = "active"')->fetchColumn();
        }
        $expCol = admin_column_exists($pdo, 'subscriptions', 'expires_at') ? 'expires_at' : (admin_column_exists($pdo, 'subscriptions', 'expiry_date') ? 'expiry_date' : null);
        if ($expCol) {
            $stats['subs_expired'] = (int) $pdo->query("SELECT COUNT(*) FROM `subscriptions` WHERE `{$expCol}` IS NOT NULL AND `{$expCol}` < NOW()")->fetchColumn();
        }
    }

    // Payments
    if (admin_table_exists($pdo, 'payments')) {
        $statusCol = admin_column_exists($pdo, 'payments', 'status') ? 'status' : null;
        if ($statusCol) {
            $stats['revenue_total'] = (float) $pdo->query("SELECT COALESCE(SUM(amount), 0) FROM `payments` WHERE LOWER(`{$statusCol}`) IN ('completed', 'paid', 'successful')")->fetchColumn();
            $stats['payments_completed'] = (int) $pdo->query("SELECT COUNT(*) FROM `payments` WHERE LOWER(`{$statusCol}`) IN ('completed', 'paid', 'successful')")->fetchColumn();
            $stats['payments_pending'] = (int) $pdo->query("SELECT COUNT(*) FROM `payments` WHERE LOWER(`{$statusCol}`) = 'pending'")->fetchColumn();
            $stats['payments_failed'] = (int) $pdo->query("SELECT COUNT(*) FROM `payments` WHERE LOWER(`{$statusCol}`) IN ('failed', 'refunded')")->fetchColumn();
        } else {
            $stats['revenue_total'] = (float) $pdo->query('SELECT COALESCE(SUM(amount), 0) FROM `payments`')->fetchColumn();
            $stats['payments_completed'] = (int) $pdo->query('SELECT COUNT(*) FROM `payments`')->fetchColumn();
        }
    }
} catch (Throwable $e) {
    admin_log_error($e, 'Reports aggregation error');
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. CHART 1: MONTHLY USER REGISTRATION TREND
// ─────────────────────────────────────────────────────────────────────────────

$userMonths = [];
for ($m = 5; $m >= 0; $m--) {
    $mKey = date('Y-m', strtotime("-$m months"));
    $mLabel = date('M Y', strtotime("-$m months"));
    $userMonths[$mKey] = ['label' => $mLabel, 'count' => 0];
}

if (admin_table_exists($pdo, 'users') && admin_column_exists($pdo, 'users', 'created_at')) {
    try {
        $uStmt = $pdo->query("
            SELECT DATE_FORMAT(created_at, '%Y-%m') AS ym, COUNT(*) AS cnt
            FROM `users`
            WHERE created_at >= DATE_SUB(CURDATE(), INTERVAL 6 MONTH)
            GROUP BY ym
            ORDER BY ym ASC
        ");
        foreach ($uStmt->fetchAll(PDO::FETCH_ASSOC) as $uRow) {
            if (isset($userMonths[$uRow['ym']])) {
                $userMonths[$uRow['ym']]['count'] = (int) $uRow['cnt'];
            }
        }
    } catch (Throwable $e) {}
}

$maxUserCount = 1;
foreach ($userMonths as $um) {
    if ($um['count'] > $maxUserCount) $maxUserCount = $um['count'];
}

$userChartSvg = '<svg viewBox="0 0 420 150" class="chart-svg" xmlns="http://www.w3.org/2000/svg">';
$userChartSvg .= '<line x1="30" y1="120" x2="410" y2="120" stroke="#e3e7ef" stroke-width="1"/>';
$userChartSvg .= '<line x1="30" y1="70" x2="410" y2="70" stroke="#f0f2f6" stroke-width="1" stroke-dasharray="3"/>';
$userChartSvg .= '<text x="10" y="123" fill="#8b95a6" font-size="9">0</text>';
$userChartSvg .= '<text x="10" y="73" fill="#8b95a6" font-size="9">' . ceil($maxUserCount / 2) . '</text>';
$userChartSvg .= '<text x="10" y="20" fill="#8b95a6" font-size="9">' . $maxUserCount . '</text>';

$uIndex = 0;
$uBarW = 32;
$uGap = 62;
$uStartX = 45;
foreach ($userMonths as $um) {
    $x = $uStartX + ($uIndex * $uGap);
    $h = round(($um['count'] / $maxUserCount) * 95);
    $y = 120 - $h;
    $fill = ($um['count'] > 0) ? '#1d4ed8' : '#e5e7eb';
    $userChartSvg .= '<rect x="' . $x . '" y="' . $y . '" width="' . $uBarW . '" height="' . max(3, $h) . '" rx="4" fill="' . $fill . '"/>';
    $userChartSvg .= '<text x="' . ($x + ($uBarW / 2)) . '" y="' . ($y - 5) . '" fill="#1d4ed8" font-size="10" font-weight="700" text-anchor="middle">' . $um['count'] . '</text>';
    $userChartSvg .= '<text x="' . ($x + ($uBarW / 2)) . '" y="136" fill="#8b95a6" font-size="9" text-anchor="middle">' . $um['label'] . '</text>';
    $uIndex++;
}
$userChartSvg .= '</svg>';

// ─────────────────────────────────────────────────────────────────────────────
// 3. CHART 2: OS DISTRIBUTION ACROSS FLEET
// ─────────────────────────────────────────────────────────────────────────────

$osStats = [];
if (admin_table_exists($pdo, 'devices')) {
    try {
        $osStmt = $pdo->query("SELECT COALESCE(os_type, 'Unknown') AS os_name, COUNT(*) AS cnt FROM `devices` GROUP BY os_name ORDER BY cnt DESC");
        $osStats = $osStmt->fetchAll(PDO::FETCH_ASSOC);
    } catch (Throwable $e) {}
}

$osProgressBars = '';
$totalFleet = max(1, $stats['devices_total']);
$osColors = ['#c0392b', '#1d4ed8', '#16a34a', '#b45309', '#6b7280'];
$ci = 0;
foreach ($osStats as $os) {
    $color = $osColors[$ci % count($osColors)];
    $pct = round(($os['cnt'] / $totalFleet) * 100, 1);
    $osName = htmlspecialchars(ucfirst((string) $os['os_name']), ENT_QUOTES, 'UTF-8');
    $osProgressBars .= "
        <div style=\"margin-bottom:12px;\">
            <div style=\"display:flex; justify-content:space-between; font-size:0.8125rem; margin-bottom:4px;\">
                <span style=\"font-weight:600;\">{$osName}</span>
                <span style=\"color:var(--text-muted);\">{$os['cnt']} ({$pct}%)</span>
            </div>
            <div style=\"height:8px; background:var(--bg); border-radius:4px; overflow:hidden; border:1px solid var(--border);\">
                <div style=\"width:{$pct}%; height:100%; background:{$color}; border-radius:4px;\"></div>
            </div>
        </div>
    ";
    $ci++;
}
if ($osProgressBars === '') {
    $osProgressBars = '<div class="empty-state">No operating system metrics recorded.</div>';
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. CHART 3: SUBSCRIPTION PLAN MARKET SHARE
// ─────────────────────────────────────────────────────────────────────────────

$planStats = [];
if (admin_table_exists($pdo, 'plans')) {
    try {
        $pStmt = $pdo->query("
            SELECT p.name AS plan_name, p.price, COUNT(s.id) AS active_subs
            FROM `plans` p
            LEFT JOIN `subscriptions` s ON s.plan_id = p.id AND s.status = 'active'
            GROUP BY p.id, p.name, p.price
            ORDER BY active_subs DESC
        ");
        $planStats = $pStmt->fetchAll(PDO::FETCH_ASSOC);
    } catch (Throwable $e) {}
}

$planBars = '';
$totalActiveSubs = max(1, $stats['subs_active']);
$pi = 0;
foreach ($planStats as $ps) {
    $color = $osColors[$pi % count($osColors)];
    $pct = round(($ps['active_subs'] / $totalActiveSubs) * 100, 1);
    $pName = htmlspecialchars((string) $ps['plan_name'], ENT_QUOTES, 'UTF-8');
    $priceFmt = '₹' . number_format((float) $ps['price'], 2);
    $planBars .= "
        <div style=\"margin-bottom:12px;\">
            <div style=\"display:flex; justify-content:space-between; font-size:0.8125rem; margin-bottom:4px;\">
                <span style=\"font-weight:600;\">{$pName} <small style=\"color:var(--text-muted);\">({$priceFmt})</small></span>
                <span style=\"color:var(--text-muted);\">{$ps['active_subs']} active ({$pct}%)</span>
            </div>
            <div style=\"height:8px; background:var(--bg); border-radius:4px; overflow:hidden; border:1px solid var(--border);\">
                <div style=\"width:{$pct}%; height:100%; background:{$color}; border-radius:4px;\"></div>
            </div>
        </div>
    ";
    $pi++;
}
if ($planBars === '') {
    $planBars = '<div class="empty-state">No subscription plan metrics available.</div>';
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. CHART 4: CONNECTION REQUESTS DISTRIBUTION (RATIO)
// ─────────────────────────────────────────────────────────────────────────────

$totalConns = max(1, $stats['conn_total']);
$pctAccepted = round(($stats['conn_accepted'] / $totalConns) * 100, 1);
$pctPending = round(($stats['conn_pending'] / $totalConns) * 100, 1);
$pctExpired = round(($stats['conn_expired'] / $totalConns) * 100, 1);

// Conversion Rate
$userDeviceRatio = ($stats['users_total'] > 0) ? round($stats['devices_total'] / $stats['users_total'], 2) : 0;
$activeSubRatio = ($stats['users_total'] > 0) ? round(($stats['subs_active'] / $stats['users_total']) * 100, 1) : 0;
$bandwidthFmt = admin_format_bytes($stats['bandwidth_mb'] * 1024 * 1024);
$revenueFmt = '₹' . number_format($stats['revenue_total'], 2);

// Precompute ratios for heredoc
$fleetOnlineRatio = round(($stats['devices_online'] / $totalFleet) * 100, 1) . '%';
$avgStreamBandwidth = ($stats['sessions_total'] > 0 ? admin_format_bytes(($stats['bandwidth_mb'] / $stats['sessions_total']) * 1024 * 1024) : '0 B') . ' / session';
$totalPaymentsCount = $stats['payments_completed'] + $stats['payments_pending'] + $stats['payments_failed'];
$completedTxnRate = ($totalPaymentsCount > 0 ? round(($stats['payments_completed'] / $totalPaymentsCount) * 100, 1) . '%' : '100%');
$activePlansRatio = ($stats['subs_total'] > 0 ? round(($stats['subs_active'] / $stats['subs_total']) * 100, 1) . '%' : '0%');

$body = <<<HTML
<!-- Overall Platform KPI Metric Cards -->
<div class="card-grid">
    <div class="card">
        <span class="label">Total Platform Users</span>
        <p class="value">{$stats['users_total']}</p>
        <div class="trend">{$stats['users_active']} active user accounts</div>
    </div>
    <div class="card">
        <span class="label">Total Fleet Devices</span>
        <p class="value">{$stats['devices_total']}</p>
        <div class="trend"><span style="color:#16a34a; font-weight:600;">{$stats['devices_online']} Online</span> &bull; {$stats['devices_offline']} Offline</div>
    </div>
    <div class="card">
        <span class="label">Remote Sessions</span>
        <p class="value">{$stats['sessions_total']}</p>
        <div class="trend">{$stats['sessions_completed']} completed &bull; {$stats['sessions_active']} active</div>
    </div>
    <div class="card">
        <span class="label">Active Subscriptions</span>
        <p class="value" style="color:#16a34a;">{$stats['subs_active']}</p>
        <div class="trend">{$stats['subs_expired']} expired subscriptions</div>
    </div>
    <div class="card">
        <span class="label">Total Revenue</span>
        <p class="value" style="color:#c0392b;">{$revenueFmt}</p>
        <div class="trend">{$stats['payments_completed']} settled transactions</div>
    </div>
    <div class="card">
        <span class="label">Session Bandwidth</span>
        <p class="value">{$bandwidthFmt}</p>
        <div class="trend">Total data streamed across relay</div>
    </div>
</div>

<!-- Primary Charts Grid -->
<div class="chart-grid">
    <!-- User Growth Trend Chart -->
    <div class="chart-card">
        <div class="chart-card-header">
            <div>
                <h4 class="chart-title">New User Registrations (Past 6 Months)</h4>
                <div class="chart-subtitle">Account registration volume over time</div>
            </div>
            <span class="kpi-pill">{$stats['users_total']} Total Users</span>
        </div>
        <div class="chart-container">
            {$userChartSvg}
        </div>
    </div>

    <!-- Operating System Fleet Breakdown -->
    <div class="chart-card">
        <div class="chart-card-header">
            <div>
                <h4 class="chart-title">Device Fleet Operating Systems</h4>
                <div class="chart-subtitle">OS distribution across all connected endpoints</div>
            </div>
            <span class="kpi-pill">{$stats['devices_total']} Devices</span>
        </div>
        <div style="margin-top:10px;">
            {$osProgressBars}
        </div>
    </div>
</div>

<!-- Secondary Analytics Grid -->
<div class="chart-grid">
    <!-- Subscription Market Share -->
    <div class="chart-card">
        <div class="chart-card-header">
            <div>
                <h4 class="chart-title">Subscription Plan Distribution</h4>
                <div class="chart-subtitle">Active subscriptions by plan tier</div>
            </div>
            <span class="kpi-pill">{$stats['subs_active']} Active Subs</span>
        </div>
        <div style="margin-top:10px;">
            {$planBars}
        </div>
    </div>

    <!-- Connection Verification Distribution -->
    <div class="chart-card">
        <div class="chart-card-header">
            <div>
                <h4 class="chart-title">Peer Connection Request Outcomes</h4>
                <div class="chart-subtitle">Acceptance vs timeout ratio across {$stats['conn_total']} requests</div>
            </div>
            <span class="kpi-pill">{$pctAccepted}% Success</span>
        </div>
        <div style="margin-top:10px;">
            <div style="margin-bottom:12px;">
                <div style="display:flex; justify-content:space-between; font-size:0.8125rem; margin-bottom:4px;">
                    <span style="font-weight:600; color:#16a34a;">Accepted & Paired</span>
                    <span style="color:var(--text-muted);">{$stats['conn_accepted']} ({$pctAccepted}%)</span>
                </div>
                <div style="height:8px; background:var(--bg); border-radius:4px; overflow:hidden; border:1px solid var(--border);">
                    <div style="width:{$pctAccepted}%; height:100%; background:#16a34a; border-radius:4px;"></div>
                </div>
            </div>
            <div style="margin-bottom:12px;">
                <div style="display:flex; justify-content:space-between; font-size:0.8125rem; margin-bottom:4px;">
                    <span style="font-weight:600; color:#b45309;">Pending Acceptance</span>
                    <span style="color:var(--text-muted);">{$stats['conn_pending']} ({$pctPending}%)</span>
                </div>
                <div style="height:8px; background:var(--bg); border-radius:4px; overflow:hidden; border:1px solid var(--border);">
                    <div style="width:{$pctPending}%; height:100%; background:#b45309; border-radius:4px;"></div>
                </div>
            </div>
            <div style="margin-bottom:12px;">
                <div style="display:flex; justify-content:space-between; font-size:0.8125rem; margin-bottom:4px;">
                    <span style="font-weight:600; color:#6b7280;">Expired / Cancelled / Rejected</span>
                    <span style="color:var(--text-muted);">{$stats['conn_expired']} ({$pctExpired}%)</span>
                </div>
                <div style="height:8px; background:var(--bg); border-radius:4px; overflow:hidden; border:1px solid var(--border);">
                    <div style="width:{$pctExpired}%; height:100%; background:#6b7280; border-radius:4px;"></div>
                </div>
            </div>
        </div>
    </div>
</div>

<!-- Operational Ratios Breakdown -->
<div class="section">
    <div class="section-header">
        <h3>Platform Operational Ratios</h3>
    </div>
    <div class="detail-panels">
        <div class="detail-panel">
            <h4>Fleet & Connectivity Ratios</h4>
            <div class="detail-row"><span>Fleet Online Ratio</span><strong>{$fleetOnlineRatio}</strong></div>
            <div class="detail-row"><span>Average Devices Per User</span><strong>{$userDeviceRatio} devices</strong></div>
            <div class="detail-row"><span>Connection Acceptance Rate</span><strong>{$pctAccepted}%</strong></div>
            <div class="detail-row"><span>Average Stream Bandwidth</span><strong>{$avgStreamBandwidth}</strong></div>
        </div>
        <div class="detail-panel">
            <h4>Billing & Revenue Health</h4>
            <div class="detail-row"><span>Active Subscriber Penetration</span><strong>{$activeSubRatio}% of registered users</strong></div>
            <div class="detail-row"><span>Completed Transaction Rate</span><strong>{$completedTxnRate}</strong></div>
            <div class="detail-row"><span>Active Plans Ratio</span><strong>{$activePlansRatio}</strong></div>
            <div class="detail-row"><span>Total Settled Revenue</span><strong>{$revenueFmt}</strong></div>
        </div>
    </div>
</div>
HTML;

echo admin_render_page('Reports & Analytics', 'reports', $body);
