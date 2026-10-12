<?php
require_once __DIR__ . '/includes/db.php';
require_once __DIR__ . '/includes/auth.php';
require_once __DIR__ . '/includes/layout.php';

admin_require_auth();
$pdo = admin_db();
$sessionTable = admin_table_exists($pdo, 'sessions') ? 'sessions' : null;
$deviceTable = admin_table_exists($pdo, 'devices') ? 'devices' : null;
$rowsHtml = '<tr><td colspan="5" class="empty-state">' . ($sessionTable ? 'No sessions found.' : 'The existing sessions table is unavailable.') . '</td></tr>';
$pageError = '';

if ($sessionTable) {
    try {
        $hasStreamingDevice = $deviceTable
            && admin_column_exists($pdo, $sessionTable, 'streaming_device_id')
            && admin_column_exists($pdo, $deviceTable, 'id')
            && admin_column_exists($pdo, $deviceTable, 'name');
        $hasViewingDevice = $deviceTable
            && admin_column_exists($pdo, $sessionTable, 'viewing_device_id')
            && admin_column_exists($pdo, $deviceTable, 'id')
            && admin_column_exists($pdo, $deviceTable, 'name');
        $select = ['s.*'];
        $joins = '';
        if ($hasStreamingDevice) {
            $select[] = 'sd.`name` AS `streaming_device_name`';
            $joins .= ' LEFT JOIN `devices` sd ON sd.`id` = s.`streaming_device_id`';
        }
        if ($hasViewingDevice) {
            $select[] = 'vd.`name` AS `viewing_device_name`';
            $joins .= ' LEFT JOIN `devices` vd ON vd.`id` = s.`viewing_device_id`';
        }
        $orderColumn = admin_column_exists($pdo, $sessionTable, 'created_at')
            ? '`created_at`'
            : (admin_column_exists($pdo, $sessionTable, 'id') ? '`id`' : null);
        $stmt = $pdo->query('SELECT ' . implode(', ', $select) . ' FROM `sessions` s' . $joins . ($orderColumn ? ' ORDER BY s.' . $orderColumn . ' DESC' : '') . ' LIMIT 100');
        $rows = $stmt->fetchAll(PDO::FETCH_ASSOC);
        if ($rows) {
            $rowsHtml = '';
            foreach ($rows as $row) {
                $rowsHtml .= '<tr><td>' . htmlspecialchars((string) ($row['session_id'] ?? ($row['id'] ?? 'N/A')), ENT_QUOTES, 'UTF-8') . '</td>'
                    . '<td>' . htmlspecialchars((string) ($row['streaming_device_name'] ?? ($row['streaming_device_id'] ?? 'N/A')), ENT_QUOTES, 'UTF-8') . '</td>'
                    . '<td>' . htmlspecialchars((string) ($row['viewing_device_name'] ?? ($row['viewing_device_id'] ?? 'N/A')), ENT_QUOTES, 'UTF-8') . '</td>'
                    . '<td>' . htmlspecialchars((string) ($row['status'] ?? 'N/A'), ENT_QUOTES, 'UTF-8') . '</td>'
                    . '<td>' . htmlspecialchars((string) ($row['created_at'] ?? 'N/A'), ENT_QUOTES, 'UTF-8') . '</td></tr>';
            }
        }
    } catch (Throwable $e) {
        error_log('Admin sessions query failed: ' . $e->getMessage());
        $pageError = 'Session records could not be loaded. Check the server error log for the database error.';
    }
}

$body = ($pageError !== '' ? '<div class="flash error">' . htmlspecialchars($pageError, ENT_QUOTES, 'UTF-8') . '</div>' : '')
    . '<div class="section"><div class="section-header"><h3>Sessions</h3></div><div style="overflow:auto"><table><thead><tr><th>Session ID</th><th>Streaming Device</th><th>Viewing Device</th><th>Status</th><th>Created</th></tr></thead><tbody>'
    . $rowsHtml . '</tbody></table></div></div>';

echo admin_render_page('Sessions', 'sessions', $body);
