<?php
require_once __DIR__ . '/includes/db.php';
require_once __DIR__ . '/includes/auth.php';
require_once __DIR__ . '/includes/layout.php';

admin_require_auth();
$pdo = admin_db();
$table = admin_table_exists($pdo, 'payments') ? 'payments' : null;

$rowHtml = '<tr><td colspan="8" class="empty-state">No payment records found.</td></tr>';
if ($table) {
    try {
        $orderColumn = admin_column_exists($pdo, $table, 'created_at')
            ? '`created_at`'
            : (admin_column_exists($pdo, $table, 'id') ? '`id`' : null);
        $stmt = $pdo->query('SELECT * FROM `payments`' . ($orderColumn ? ' ORDER BY ' . $orderColumn . ' DESC' : '') . ' LIMIT 50');
        $rows = $stmt->fetchAll(PDO::FETCH_ASSOC);
        if ($rows) {
            $rowHtml = '';
            foreach ($rows as $row) {
                $ref = $row['payment_reference'] ?? 'N/A';
                $status = strtoupper((string) ($row['status'] ?? 'Pending'));
                $rowHtml .= '<tr><td>' . htmlspecialchars((string) ($row['id'] ?? '')) . '</td><td>' . htmlspecialchars((string) ($row['device_id'] ?? '')) . '</td><td>' . htmlspecialchars((string) ($row['email'] ?? 'N/A')) . '</td><td>' . htmlspecialchars((string) ($row['plan'] ?? ($row['plan_name'] ?? 'N/A'))) . '</td><td>₹' . number_format((float) ($row['amount'] ?? 0), 2) . '</td><td>' . htmlspecialchars((string) $ref) . '</td><td>' . htmlspecialchars((string) ($row['payment_method'] ?? 'Manual')) . '</td><td><span class="status-badge status-' . strtolower(str_replace(' ', '-', $status)) . '">' . htmlspecialchars($status) . '</span></td><td>' . htmlspecialchars((string) ($row['paid_at'] ?? ($row['created_at'] ?? 'N/A'))) . '</td></tr>';
            }
        }
    } catch (Throwable $e) {
        error_log('Payments page error: ' . $e->getMessage());
    }
}

$body = <<<HTML
<div class="section">
    <div class="section-header">
        <h3>Payments</h3>
        <div class="filters">
            <span class="chip active">Successful</span>
            <span class="chip">Pending</span>
            <span class="chip">Failed</span>
            <span class="chip">Refunded</span>
        </div>
    </div>
    <div style="overflow:auto;">
        <table>
            <thead>
                <tr>
                    <th>Payment ID</th>
                    <th>Device ID</th>
                    <th>Email</th>
                    <th>Plan</th>
                    <th>Amount</th>
                    <th>Payment Reference</th>
                    <th>Payment Method</th>
                    <th>Status</th>
                    <th>Paid Date</th>
                </tr>
            </thead>
            <tbody>
                {$rowHtml}
            </tbody>
        </table>
    </div>
</div>
HTML;

echo admin_render_page('Payments', 'payments', $body);
