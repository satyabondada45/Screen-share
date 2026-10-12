<?php
require_once __DIR__ . '/includes/db.php';
require_once __DIR__ . '/includes/auth.php';
require_once __DIR__ . '/includes/layout.php';

admin_require_auth();
$pdo = admin_db();
$table = admin_table_exists($pdo, 'notifications') ? 'notifications' : null;
$rowsHtml = '<tr><td colspan="3" class="empty-state">' . ($table ? 'No notifications found.' : 'The existing notifications table is unavailable.') . '</td></tr>';
$pageError = '';

if ($table) {
    try {
        $columns = admin_table_columns($pdo, $table);
        $orderColumn = in_array('created_at', $columns, true)
            ? '`created_at`'
            : (in_array('id', $columns, true) ? '`id`' : null);
        $stmt = $pdo->query('SELECT * FROM `notifications`' . ($orderColumn ? ' ORDER BY ' . $orderColumn . ' DESC' : '') . ' LIMIT 100');
        $notifications = $stmt->fetchAll(PDO::FETCH_ASSOC);
        if ($notifications) {
            $rowsHtml = '';
            foreach ($notifications as $notification) {
                $type = $notification['type'] ?? ($notification['title'] ?? ($notification['category'] ?? 'Notification'));
                $message = $notification['message'] ?? ($notification['body'] ?? '—');
                $date = $notification['created_at'] ?? ($notification['created'] ?? 'N/A');
                $rowsHtml .= '<tr><td>' . htmlspecialchars((string) $type, ENT_QUOTES, 'UTF-8') . '</td><td>' . htmlspecialchars((string) $message, ENT_QUOTES, 'UTF-8') . '</td><td>' . htmlspecialchars((string) $date, ENT_QUOTES, 'UTF-8') . '</td></tr>';
            }
        }
    } catch (Throwable $e) {
        error_log('Admin notifications query failed: ' . $e->getMessage());
        $pageError = 'Notification records could not be loaded. Check the server error log for the database error.';
    }
}

$body = ($pageError !== '' ? '<div class="flash error">' . htmlspecialchars($pageError, ENT_QUOTES, 'UTF-8') . '</div>' : '')
    . '<div class="section"><div class="section-header"><h3>Notifications</h3></div><div style="overflow:auto"><table><thead><tr><th>Type</th><th>Message</th><th>Date</th></tr></thead><tbody>'
    . $rowsHtml . '</tbody></table></div></div>';

echo admin_render_page('Notifications', 'notifications', $body);
