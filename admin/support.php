<?php
require_once __DIR__ . '/includes/db.php';
require_once __DIR__ . '/includes/auth.php';
require_once __DIR__ . '/includes/layout.php';

admin_require_auth();
$pdo = admin_db();
$table = admin_table_exists($pdo, 'support_tickets') ? 'support_tickets' : null;

$rowHtml = '<tr><td colspan="7" class="empty-state">No support tickets found.</td></tr>';
if ($table) {
    try {
        $orderColumn = admin_column_exists($pdo, $table, 'updated_at')
            ? '`updated_at`'
            : (admin_column_exists($pdo, $table, 'created_at') ? '`created_at`' : null);
        $stmt = $pdo->query('SELECT * FROM `support_tickets`' . ($orderColumn ? ' ORDER BY ' . $orderColumn . ' DESC' : '') . ' LIMIT 20');
        $rows = $stmt->fetchAll(PDO::FETCH_ASSOC);
        if ($rows) {
            $rowHtml = '';
            foreach ($rows as $row) {
                $status = strtoupper((string) ($row['status'] ?? 'OPEN'));
                $rowHtml .= '<tr><td>' . htmlspecialchars((string) ($row['id'] ?? '')) . '</td><td>' . htmlspecialchars((string) ($row['user_name'] ?? ($row['email'] ?? 'N/A'))) . '</td><td>' . htmlspecialchars((string) ($row['device_id'] ?? 'N/A')) . '</td><td>' . htmlspecialchars((string) ($row['subject'] ?? 'General inquiry')) . '</td><td><span class="status-badge status-' . strtolower(str_replace(' ', '-', $status)) . '">' . htmlspecialchars($status) . '</span></td><td>' . htmlspecialchars((string) ($row['created_at'] ?? 'N/A')) . '</td><td>' . htmlspecialchars((string) ($row['updated_at'] ?? 'N/A')) . '</td></tr>';
            }
        }
    } catch (Throwable $e) {
        error_log('Support page error: ' . $e->getMessage());
    }
}

$body = <<<HTML
<div class="section">
  <div class="section-header"><h3>Support tickets</h3></div>
  <div style="overflow:auto;">
    <table>
      <thead><tr><th>Ticket ID</th><th>User</th><th>Device</th><th>Subject</th><th>Status</th><th>Created</th><th>Updated</th></tr></thead>
      <tbody>{$rowHtml}</tbody>
    </table>
  </div>
</div>
HTML;

echo admin_render_page('Support', 'support', $body);
