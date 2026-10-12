<?php
require_once __DIR__ . '/includes/db.php';
require_once __DIR__ . '/includes/auth.php';
require_once __DIR__ . '/includes/layout.php';

admin_require_auth();
$pdo = admin_db();

$connectionData = ['status' => 'No live connection data available yet.'];
$sessionTable = admin_table_exists($pdo, 'sessions') ? 'sessions' : null;
if ($sessionTable) {
    try {
        $stmt = $pdo->query('SELECT COUNT(*) AS total FROM `' . $sessionTable . '` WHERE status = "active"');
        $total = $stmt->fetch(PDO::FETCH_ASSOC)['total'] ?? 0;
        $connectionData = ['status' => 'Active sessions: ' . (int) $total];
    } catch (Throwable $e) {
        error_log('Connection status query error: ' . $e->getMessage());
    }
}

$body = <<<HTML
<div class="section">
  <div class="section-header"><h3>Connection status</h3></div>
  <div class="detail-panels">
    <div class="detail-panel">
      <h4>Live relay data</h4>
      <div class="empty-state">{$connectionData['status']}</div>
    </div>
  </div>
</div>
HTML;

echo admin_render_page('Connections', 'connections', $body);
