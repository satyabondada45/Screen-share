<?php
require_once __DIR__ . '/includes/db.php';
require_once __DIR__ . '/includes/auth.php';
require_once __DIR__ . '/includes/layout.php';

admin_require_auth();
$pdo = admin_db();

$search = trim((string) ($_GET['q'] ?? ''));
$status = (string) ($_GET['status'] ?? 'all');
$status = in_array($status, ['all', 'active', 'expired', 'cancelled', 'pending'], true) ? $status : 'all';

$where = [];
$params = [];

if ($search !== '') {
    $where[] = '(u.name LIKE ? OR u.email LIKE ? OR u.username LIKE ? OR p.name LIKE ?)';
    $params[] = '%' . $search . '%';
    $params[] = '%' . $search . '%';
    $params[] = '%' . $search . '%';
    $params[] = '%' . $search . '%';
}

if ($status !== 'all') {
    $where[] = 's.status = ?';
    $params[] = $status;
}

$rowHtml = '<tr><td colspan="7" class="empty-state">No subscriptions found.</td></tr>';

try {
    $sql = "
        SELECT s.*, 
               u.name AS user_name, 
               u.email AS user_email,
               p.name AS plan_name, 
               p.price AS plan_price,
               p.duration_unit
        FROM `subscriptions` s
        LEFT JOIN `users` u ON s.user_id = u.id
        LEFT JOIN `plans` p ON s.plan_id = p.id
    ";

    if ($where) {
        $sql .= ' WHERE ' . implode(' AND ', $where);
    }
    $sql .= ' ORDER BY s.id DESC LIMIT 100';

    $stmt = $pdo->prepare($sql);
    $stmt->execute($params);
    $rows = $stmt->fetchAll(PDO::FETCH_ASSOC);

    if ($rows) {
        $rowHtml = '';
        foreach ($rows as $row) {
            $userDisplay = htmlspecialchars((string)($row['user_name'] ?? 'User #' . $row['user_id'])) . '<br><small style="color:var(--text-muted);">' . htmlspecialchars((string)($row['user_email'] ?? '')) . '</small>';
            $planDisplay = htmlspecialchars((string)($row['plan_name'] ?? 'Plan #' . $row['plan_id']));
            $startsAt = !empty($row['starts_at']) ? htmlspecialchars((string)$row['starts_at']) : 'N/A';
            
            if (empty($row['expires_at']) || ($row['duration_unit'] ?? '') === 'unlimited') {
                $expiresAt = '<span style="color:#16a34a; font-weight:500;">Unlimited</span>';
            } else {
                $expiresAt = htmlspecialchars((string)$row['expires_at']);
            }

            $statusValue = strtoupper((string)($row['status'] ?? 'ACTIVE'));
            $statusClass = strtolower($statusValue);
            $amount = (float)($row['plan_price'] ?? 0);

            $rowHtml .= '<tr>';
            $rowHtml .= '<td>#' . (int)$row['id'] . '</td>';
            $rowHtml .= '<td>' . $userDisplay . '</td>';
            $rowHtml .= '<td><strong>' . $planDisplay . '</strong></td>';
            $rowHtml .= '<td>' . $startsAt . '</td>';
            $rowHtml .= '<td>' . $expiresAt . '</td>';
            $rowHtml .= '<td>₹' . number_format($amount, 2) . '</td>';
            $rowHtml .= '<td><span class="status-badge status-' . $statusClass . '">' . htmlspecialchars($statusValue) . '</span></td>';
            $rowHtml .= '</tr>';
        }
    }
} catch (Throwable $e) {
    error_log('Subscriptions page error: ' . $e->getMessage());
}

$subscriptionsUrl = htmlspecialchars(admin_url('subscriptions.php'), ENT_QUOTES, 'UTF-8');
$searchValue = htmlspecialchars($search, ENT_QUOTES, 'UTF-8');
$statusOptions = '';
foreach (['all' => 'All', 'active' => 'Active', 'expired' => 'Expired', 'pending' => 'Pending', 'cancelled' => 'Cancelled'] as $value => $label) {
    $statusOptions .= '<option value="' . $value . '"' . ($status === $value ? ' selected' : '') . '>' . $label . '</option>';
}

$body = <<<HTML
<div class="section">
    <div class="section-header">
        <h3>Subscriptions</h3>
        <div class="filters">
            <form method="GET" action="{$subscriptionsUrl}" style="display:flex; gap:8px; flex-wrap:wrap; align-items:center;">
                <input type="text" name="q" value="{$searchValue}" placeholder="Search user or plan..." style="border:1px solid var(--border); border-radius:10px; padding:10px 12px; min-width:220px;">
                <select name="status" style="border:1px solid var(--border); border-radius:10px; padding:10px 12px;">{$statusOptions}</select>
                <button class="button" type="submit">Filter</button>
            </form>
        </div>
    </div>
    <div style="overflow:auto;">
        <table>
            <thead>
                <tr>
                    <th>ID</th>
                    <th>User</th>
                    <th>Plan</th>
                    <th>Started At</th>
                    <th>Expires At</th>
                    <th>Amount</th>
                    <th>Status</th>
                </tr>
            </thead>
            <tbody>
                {$rowHtml}
            </tbody>
        </table>
    </div>
</div>
HTML;

echo admin_render_page('Subscriptions', 'subscriptions', $body);
