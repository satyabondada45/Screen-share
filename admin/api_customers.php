<?php
// admin/api_customers.php - Authenticated customer search endpoint for administrators
require_once __DIR__ . '/includes/db.php';
require_once __DIR__ . '/includes/auth.php';

if (!headers_sent()) {
    header('Content-Type: application/json; charset=UTF-8');
    header('Cache-Control: no-cache, no-store, must-revalidate');
}

if (session_status() === PHP_SESSION_NONE && !headers_sent()) {
    @session_start();
}

if (empty($_SESSION['admin_logged_in']) || empty($_SESSION['admin_user_id'])) {
    http_response_code(401);
    echo json_encode([
        'status' => 'error',
        'message' => 'Unauthorized: Administrator authentication required.'
    ], JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES);
    return;
}

try {
    $pdo = admin_db();
} catch (Throwable $e) {
    http_response_code(500);
    echo json_encode([
        'status' => 'error',
        'message' => 'Database connection failed.'
    ], JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES);
    return;
}

if (!admin_table_exists($pdo, 'users')) {
    http_response_code(404);
    echo json_encode([
        'status' => 'error',
        'message' => 'Users table not found.'
    ], JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES);
    return;
}

$userCols = admin_table_columns($pdo, 'users');
$hasUnique = in_array('unique_user_id', $userCols, true);
$hasStatus = in_array('status', $userCols, true);
$hasUsername = in_array('username', $userCols, true);
$hasCreatedAt = in_array('created_at', $userCols, true);

// Support querying a specific customer by ID directly (for prefill / verification)
if (!empty($_GET['id']) && is_numeric($_GET['id'])) {
    $targetId = (int)$_GET['id'];
    $sel = ['id', 'name', 'email'];
    if ($hasUsername) $sel[] = 'username';
    if ($hasUnique) $sel[] = 'unique_user_id';
    if ($hasStatus) $sel[] = 'status';
    $selSql = implode(', ', array_map(fn($c) => "`$c`", $sel));
    
    $stmt = $pdo->prepare("SELECT {$selSql} FROM `users` WHERE `id` = ? LIMIT 1");
    $stmt->execute([$targetId]);
    $u = $stmt->fetch(PDO::FETCH_ASSOC);
    if ($u) {
        $name = (string)($u['name'] ?? ($u['username'] ?? 'User #' . $u['id']));
        $email = (string)($u['email'] ?? '');
        $uid = (string)($u['unique_user_id'] ?? '');
        $disp = $name . ($email ? ' (' . $email . ')' : '') . ' — ID #' . $u['id'];
        echo json_encode([
            'status' => 'success',
            'user' => [
                'id' => (int)$u['id'],
                'name' => $name,
                'email' => $email,
                'username' => (string)($u['username'] ?? ''),
                'unique_user_id' => $uid,
                'status' => (string)($u['status'] ?? 'active'),
                'display' => $disp,
            ]
        ], JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES);
        return;
    } else {
        http_response_code(404);
        echo json_encode(['status' => 'error', 'message' => 'Customer not found.'], JSON_UNESCAPED_UNICODE);
        return;
    }
}

// Search query
$query = trim((string)($_GET['q'] ?? ''));
$limit = min(50, max(1, (int)($_GET['limit'] ?? 25)));
$page = max(1, (int)($_GET['page'] ?? 1));
$offset = ($page - 1) * $limit;

$where = [];
$params = [];

if ($query !== '') {
    $searchParts = [];
    if (ctype_digit($query)) {
        $searchParts[] = '`id` = ?';
        $params[] = (int)$query;
        if ($hasUnique) {
            $searchParts[] = '`unique_user_id` = ?';
            $params[] = $query;
        }
    }
    $searchParts[] = '`name` LIKE ?';
    $params[] = '%' . $query . '%';
    $searchParts[] = '`email` LIKE ?';
    $params[] = '%' . $query . '%';
    if ($hasUsername) {
        $searchParts[] = '`username` LIKE ?';
        $params[] = '%' . $query . '%';
    }
    if ($hasUnique && !ctype_digit($query)) {
        $searchParts[] = '`unique_user_id` LIKE ?';
        $params[] = '%' . $query . '%';
    }
    $where[] = '(' . implode(' OR ', $searchParts) . ')';
}

$whereSql = !empty($where) ? 'WHERE ' . implode(' AND ', $where) : '';

try {
    $countStmt = $pdo->prepare("SELECT COUNT(*) FROM `users` {$whereSql}");
    $countStmt->execute($params);
    $totalMatching = (int)$countStmt->fetchColumn();

    $sel = ['id', 'name', 'email'];
    if ($hasUsername) $sel[] = 'username';
    if ($hasUnique) $sel[] = 'unique_user_id';
    if ($hasStatus) $sel[] = 'status';
    if ($hasCreatedAt) $sel[] = 'created_at';
    $selSql = implode(', ', array_map(fn($c) => "`$c`", $sel));

    $sql = "SELECT {$selSql} FROM `users` {$whereSql} ORDER BY `id` DESC LIMIT {$limit} OFFSET {$offset}";
    $stmt = $pdo->prepare($sql);
    $stmt->execute($params);
    $rows = $stmt->fetchAll(PDO::FETCH_ASSOC);

    $results = [];
    foreach ($rows as $row) {
        $id = (int)$row['id'];
        $name = (string)($row['name'] ?? ($row['username'] ?? 'User #' . $id));
        $email = (string)($row['email'] ?? '');
        $uid = (string)($row['unique_user_id'] ?? '');
        $display = $name . ($email ? ' (' . $email . ')' : '') . ' — ID #' . $id;

        $results[] = [
            'id' => $id,
            'name' => $name,
            'email' => $email,
            'username' => (string)($row['username'] ?? ''),
            'unique_user_id' => $uid,
            'status' => (string)($row['status'] ?? 'active'),
            'display' => $display,
        ];
    }

    echo json_encode([
        'status' => 'success',
        'query' => $query,
        'users' => $results,
        'total' => $totalMatching,
        'page' => $page,
        'limit' => $limit,
        'has_more' => ($offset + count($results)) < $totalMatching,
    ], JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES);

} catch (Throwable $e) {
    admin_log_error($e, 'api_customers search error');
    http_response_code(500);
    echo json_encode([
        'status' => 'error',
        'message' => 'An error occurred while searching customers: ' . $e->getMessage()
    ], JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES);
}
