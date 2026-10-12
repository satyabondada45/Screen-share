<?php
require_once __DIR__ . '/includes/db.php';
require_once __DIR__ . '/includes/auth.php';
require_once __DIR__ . '/includes/layout.php';

admin_require_auth();
$pdo = admin_db();

$summary = [
    'devices' => 0,
    'active_devices' => 0,
    'new_devices' => 0,
    'active_subscriptions' => 0,
    'expired_subscriptions' => 0,
    'revenue' => 0,
    'payments' => 0,
];

$deviceTable = admin_table_exists($pdo, 'devices') ? 'devices' : null;
$subscriptionTable = admin_table_exists($pdo, 'subscriptions') ? 'subscriptions' : null;
$paymentTable = admin_table_exists($pdo, 'payments') ? 'payments' : null;

if ($deviceTable) {
    $summary['devices'] = (int) $pdo->query('SELECT COUNT(*) FROM `' . $deviceTable . '`')->fetchColumn();
    if (admin_column_exists($pdo, $deviceTable, 'is_online')) {
        $summary['active_devices'] = (int) $pdo->query('SELECT COUNT(*) FROM `' . $deviceTable . '` WHERE is_online = 1')->fetchColumn();
    } elseif (admin_column_exists($pdo, $deviceTable, 'status')) {
        $summary['active_devices'] = (int) $pdo->query('SELECT COUNT(*) FROM `' . $deviceTable . '` WHERE LOWER(status) IN ("active", "online")')->fetchColumn();
    }
    if (admin_column_exists($pdo, $deviceTable, 'created_at')) {
        $summary['new_devices'] = (int) $pdo->query('SELECT COUNT(*) FROM `' . $deviceTable . '` WHERE created_at >= DATE_SUB(NOW(), INTERVAL 30 DAY)')->fetchColumn();
    }
}
if ($subscriptionTable) {
    if (admin_column_exists($pdo, $subscriptionTable, 'status')) {
        $summary['active_subscriptions'] = (int) $pdo->query('SELECT COUNT(*) FROM `' . $subscriptionTable . '` WHERE LOWER(status) = "active"')->fetchColumn();
    }
    if (admin_column_exists($pdo, $subscriptionTable, 'expiry_date')) {
        $summary['expired_subscriptions'] = (int) $pdo->query('SELECT COUNT(*) FROM `' . $subscriptionTable . '` WHERE expiry_date < NOW()')->fetchColumn();
    }
}
if ($paymentTable) {
    $summary['payments'] = (int) $pdo->query('SELECT COUNT(*) FROM `' . $paymentTable . '`')->fetchColumn();
    if (admin_column_exists($pdo, $paymentTable, 'amount')) {
        $revenueSql = 'SELECT COALESCE(SUM(amount),0) FROM `' . $paymentTable . '`';
        if (admin_column_exists($pdo, $paymentTable, 'status')) {
            $revenueSql .= ' WHERE LOWER(status) IN ("paid","successful","completed")';
        }
        $summary['revenue'] = (float) $pdo->query($revenueSql)->fetchColumn();
    }
}

$cards = '
<div class="card-grid">
  <div class="card"><span class="label">Total devices</span><p class="value">' . htmlspecialchars((string) $summary['devices']) . '</p></div>
  <div class="card"><span class="label">Active devices</span><p class="value">' . htmlspecialchars((string) $summary['active_devices']) . '</p></div>
  <div class="card"><span class="label">New devices</span><p class="value">' . htmlspecialchars((string) $summary['new_devices']) . '</p></div>
  <div class="card"><span class="label">Active subscriptions</span><p class="value">' . htmlspecialchars((string) $summary['active_subscriptions']) . '</p></div>
  <div class="card"><span class="label">Expired subscriptions</span><p class="value">' . htmlspecialchars((string) $summary['expired_subscriptions']) . '</p></div>
  <div class="card"><span class="label">Revenue</span><p class="value">₹' . number_format((float) $summary['revenue'], 2) . '</p></div>
  <div class="card"><span class="label">Payments</span><p class="value">' . htmlspecialchars((string) $summary['payments']) . '</p></div>
  <div class="card"><span class="label">Subscription growth</span><p class="value">N/A</p></div>
</div>';

$body = $cards . '
<div class="section">
  <div class="section-header"><h3>Summary</h3></div>
  <div class="detail-panels">
    <div class="detail-panel"><h4>Operational views</h4><div class="detail-row"><span>Total devices</span><strong>' . htmlspecialchars((string) $summary['devices']) . '</strong></div><div class="detail-row"><span>Active subscriptions</span><strong>' . htmlspecialchars((string) $summary['active_subscriptions']) . '</strong></div><div class="detail-row"><span>Expired subscriptions</span><strong>' . htmlspecialchars((string) $summary['expired_subscriptions']) . '</strong></div></div>
    <div class="detail-panel"><h4>Revenue</h4><div class="detail-row"><span>Total revenue</span><strong>₹' . number_format((float) $summary['revenue'], 2) . '</strong></div><div class="detail-row"><span>Payments</span><strong>' . htmlspecialchars((string) $summary['payments']) . '</strong></div><div class="detail-row"><span>New devices</span><strong>' . htmlspecialchars((string) $summary['new_devices']) . '</strong></div></div>
  </div>
</div>';

echo admin_render_page('Reports', 'reports', $body);
