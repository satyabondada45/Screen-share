<?php
require_once __DIR__ . '/includes/db.php';
require_once __DIR__ . '/includes/auth.php';
require_once __DIR__ . '/includes/layout.php';

admin_require_auth();
$pdo = admin_db();
$table = admin_table_exists($pdo, 'audit_logs') ? 'audit_logs' : null;

$rowHtml = '<tr><td colspan="7" class="empty-state">No audit logs found.</td></tr>';
if ($table) {
    try {
        $orderColumn = admin_column_exists($pdo, $table, 'created_at')
            ? '`created_at`'
            : (admin_column_exists($pdo, $table, 'id') ? '`id`' : null);
        $stmt = $pdo->query('SELECT * FROM `audit_logs`' . ($orderColumn ? ' ORDER BY ' . $orderColumn . ' DESC' : '') . ' LIMIT 50');
        $rows = $stmt->fetchAll(PDO::FETCH_ASSOC);
        if ($rows) {
            $rowHtml = '';
            foreach ($rows as $row) {
                $rowHtml .= '<tr><td>' . htmlspecialchars((string) ($row['created_at'] ?? 'N/A')) . '</td><td>' . htmlspecialchars((string) ($row['admin_user_id'] ?? 'System')) . '</td><td>' . htmlspecialchars((string) ($row['action'] ?? 'N/A')) . '</td><td>' . htmlspecialchars((string) ($row['entity_type'] ?? 'N/A')) . '</td><td>' . htmlspecialchars((string) ($row['description'] ?? 'N/A')) . '</td><td>' . htmlspecialchars((string) ($row['ip_address'] ?? 'N/A')) . '</td></tr>';
            }
        }
    } catch (Throwable $e) {
        error_log('Audit logs query error: ' . $e->getMessage());
    }
}

$body = <<<HTML
<div class="section">
  <div class="section-header"><h3>Audit logs</h3></div>
  <div style="overflow:auto;">
    <table>
      <thead><tr><th>Date</th><th>Admin</th><th>Action</th><th>Entity</th><th>Description</th><th>IP</th></tr></thead>
      <tbody>{$rowHtml}</tbody>
    </table>
  </div>
</div>
HTML;

echo admin_render_page('Audit Logs', 'audit-logs', $body);
