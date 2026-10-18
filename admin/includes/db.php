<?php
if (session_status() === PHP_SESSION_NONE) {
    session_start();
}

function admin_service_file(string $filename): ?string
{
    if ($filename === '' || basename($filename) !== $filename) {
        return null;
    }

    $projectRoot = dirname(__DIR__, 2);
    foreach (['services', 'backend' . DIRECTORY_SEPARATOR . 'services'] as $directory) {
        $path = $projectRoot . DIRECTORY_SEPARATOR . $directory . DIRECTORY_SEPARATOR . $filename;
        if (is_file($path)) {
            return $path;
        }
    }

    return null;
}

function admin_request_id(): string
{
    static $requestId = null;
    if ($requestId === null) {
        $requestId = 'req_' . bin2hex(random_bytes(6));
    }
    return $requestId;
}

function admin_log_error(Throwable $e, string $context = ''): string
{
    $reqId = admin_request_id();
    $msg = sprintf(
        '[DeskStream Admin Error][%s] %s in %s:%d | Error: %s',
        $reqId,
        $context !== '' ? "Context: $context |" : '',
        $e->getFile(),
        $e->getLine(),
        $e->getMessage()
    );
    error_log($msg);
    return $reqId;
}

function admin_safe_error_html(string $title, string $userMessage, ?Throwable $e = null): string
{
    $reqId = $e ? admin_log_error($e, $title) : admin_request_id();
    $safeTitle = htmlspecialchars($title, ENT_QUOTES, 'UTF-8');
    $safeMessage = htmlspecialchars($userMessage, ENT_QUOTES, 'UTF-8');
    $safeReqId = htmlspecialchars($reqId, ENT_QUOTES, 'UTF-8');
    
    return <<<HTML
<div class="section" style="max-width:700px; margin:40px auto; text-align:center;">
    <div style="font-size:3rem; margin-bottom:12px;">⚠️</div>
    <h2 style="margin-bottom:10px; color:var(--text-primary); font-size:1.5rem;">{$safeTitle}</h2>
    <p style="color:var(--text-secondary); margin-bottom:20px; line-height:1.5;">{$safeMessage}</p>
    <div style="background:var(--bg-card); border:1px solid var(--border-color); border-radius:8px; padding:12px 16px; display:inline-block; font-family:monospace; font-size:0.875rem; color:var(--text-muted);">
        Reference ID: <strong>{$safeReqId}</strong>
    </div>
    <div style="margin-top:24px;">
        <a href="index.php" class="button" style="display:inline-block;">Return to Dashboard</a>
    </div>
</div>
HTML;
}

function admin_db(): PDO
{
    static $pdo = null;

    if ($pdo instanceof PDO) {
        return $pdo;
    }

    $configPaths = [
        dirname(__DIR__, 2) . DIRECTORY_SEPARATOR . 'config' . DIRECTORY_SEPARATOR . 'database.php',
        dirname(__DIR__, 2) . DIRECTORY_SEPARATOR . 'backend' . DIRECTORY_SEPARATOR . 'config' . DIRECTORY_SEPARATOR . 'database.php',
    ];
    $configPath = null;
    foreach ($configPaths as $candidatePath) {
        if (is_file($candidatePath)) {
            $configPath = $candidatePath;
            break;
        }
    }
    $configError = null;

    if ($configPath !== null) {
        $ADMIN_DB_THROW_ON_ERROR = true;
        try {
            require $configPath;
            $centralPdo = $pdo ?? ($conn ?? null);
            if ($centralPdo instanceof PDO) {
                $pdo = $centralPdo;
                return $pdo;
            }
            $configError = new RuntimeException('Central database configuration did not provide a PDO connection.');
        } catch (Throwable $e) {
            $configError = $e;
        }
    } else {
        $configError = new RuntimeException('No central database configuration was found in the project config paths.');
    }

    $envPath = dirname(__DIR__, 2) . DIRECTORY_SEPARATOR . '.env';
    if (is_file($envPath)) {
        $lines = file($envPath, FILE_IGNORE_NEW_LINES | FILE_SKIP_EMPTY_LINES);
        foreach ($lines as $line) {
            $line = trim($line);
            if ($line === '' || $line[0] === '#' || strpos($line, '=') === false) {
                continue;
            }
            [$key, $value] = array_map('trim', explode('=', $line, 2));
            $value = trim($value, "\"'");
            if ($key !== '' && getenv($key) === false && !array_key_exists($key, $_ENV)) {
                putenv($key . '=' . $value);
                $_ENV[$key] = $value;
            }
        }
    }

    $host = getenv('DB_HOST') ?: ($host ?? null);
    $port = getenv('DB_PORT') ?: ($port ?? '3306');
    $dbName = getenv('DB_NAME') ?: ($db ?? null);
    $userName = getenv('DB_USER') ?: ($user ?? null);
    $password = getenv('DB_PASSWORD') ?: ($pass ?? null);

    if ($host && $dbName && $userName && $password !== null) {
        try {
            $pdo = new PDO(
                "mysql:host=$host;port=$port;dbname=$dbName;charset=utf8mb4",
                $userName,
                $password,
                [
                    PDO::ATTR_ERRMODE => PDO::ERRMODE_EXCEPTION,
                    PDO::ATTR_DEFAULT_FETCH_MODE => PDO::FETCH_ASSOC,
                    PDO::ATTR_EMULATE_PREPARES => false,
                ]
            );
            $pdo->exec("SET time_zone = '+00:00'");
            return $pdo;
        } catch (Throwable $fallbackError) {
            error_log('Admin database fallback connection failed: ' . $fallbackError->getMessage());
            throw new RuntimeException('Admin database connection failed: ' . $fallbackError->getMessage(), 0, $fallbackError);
        }
    }

    error_log('Admin database connection failed: ' . $configError->getMessage());
    throw new RuntimeException('Admin database connection failed: ' . $configError->getMessage(), 0, $configError);
}

function admin_base_path(): string
{
    $scriptName = (string) ($_SERVER['SCRIPT_NAME'] ?? '/admin/index.php');
    $directory = rawurldecode(str_replace('\\', '/', dirname($scriptName)));
    $segments = array_filter(explode('/', trim($directory, '/')), 'strlen');
    $encodedSegments = array_map('rawurlencode', $segments);

    return '/' . implode('/', $encodedSegments);
}

function admin_url(string $page = '', array $query = []): string
{
    $page = trim($page, '/');
    $encodedPage = implode('/', array_map('rawurlencode', array_filter(explode('/', $page), 'strlen')));
    $url = admin_base_path() . ($encodedPage === '' ? '/' : '/' . $encodedPage);

    if ($query !== []) {
        $url .= '?' . http_build_query($query, '', '&', PHP_QUERY_RFC3986);
    }

    return $url;
}

function admin_csrf_token(): string
{
    if (empty($_SESSION['admin_csrf_token'])) {
        $_SESSION['admin_csrf_token'] = bin2hex(random_bytes(32));
    }

    return (string) $_SESSION['admin_csrf_token'];
}

function admin_verify_csrf_token($token): bool
{
    return is_string($token)
        && isset($_SESSION['admin_csrf_token'])
        && hash_equals((string) $_SESSION['admin_csrf_token'], $token);
}

function admin_table_exists(PDO $pdo, string $table): bool
{
    static $cache = [];
    if (isset($cache[$table])) {
        return $cache[$table];
    }
    try {
        $stmt = $pdo->prepare(
            'SELECT 1 FROM information_schema.TABLES
             WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ?
             LIMIT 1'
        );
        $stmt->execute([$table]);
        return $cache[$table] = (bool) $stmt->fetchColumn();
    } catch (Throwable $e) {
        return false;
    }
}

function admin_table_columns(PDO $pdo, string $table): array
{
    static $cache = [];
    if (isset($cache[$table])) {
        return $cache[$table];
    }
    if (!preg_match('/^[A-Za-z0-9_]+$/D', $table)) {
        return [];
    }

    try {
        $stmt = $pdo->prepare(
            'SELECT COLUMN_NAME FROM information_schema.COLUMNS
             WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ?
             ORDER BY ORDINAL_POSITION'
        );
        $stmt->execute([$table]);
        $cols = $stmt->fetchAll(PDO::FETCH_COLUMN);
        return $cache[$table] = is_array($cols) ? $cols : [];
    } catch (Throwable $e) {
        return [];
    }
}

function admin_column_exists(PDO $pdo, string $table, string $column): bool
{
    $columns = admin_table_columns($pdo, $table);
    return in_array($column, $columns, true);
}

function admin_related_rows(PDO $pdo, string $table, array $device, int $limit = 20, array $subscriptionIds = []): array
{
    $columns = admin_table_columns($pdo, $table);
    $conditions = [];
    $parameters = [];
    $deviceKeys = [
        'device_id' => ['id', 'device_id', 'device_uid', 'system_id'],
        'device_uid' => ['device_uid', 'system_id'],
        'system_id' => ['system_id', 'device_uid'],
    ];

    foreach ($deviceKeys as $relatedColumn => $deviceFields) {
        if (!in_array($relatedColumn, $columns, true)) {
            continue;
        }
        foreach ($deviceFields as $deviceField) {
            if (isset($device[$deviceField]) && $device[$deviceField] !== '') {
                $conditions[] = '`' . $relatedColumn . '` = ?';
                $parameters[] = (string) $device[$deviceField];
            }
        }
    }

    if ($subscriptionIds && in_array('subscription_id', $columns, true)) {
        $subscriptionIds = array_values(array_unique(array_filter(array_map('intval', $subscriptionIds), fn($id) => $id > 0)));
        if ($subscriptionIds) {
            $conditions[] = '`subscription_id` IN (' . implode(', ', array_fill(0, count($subscriptionIds), '?')) . ')';
            array_push($parameters, ...$subscriptionIds);
        }
    }

    if (!$conditions) {
        return [];
    }

    $orderColumn = in_array('created_at', $columns, true)
        ? 'created_at'
        : (in_array('id', $columns, true) ? 'id' : null);
    $order = $orderColumn ? ' ORDER BY `' . $orderColumn . '` DESC' : '';
    $limit = max(1, min(100, $limit));
    $stmt = $pdo->prepare('SELECT * FROM `' . $table . '` WHERE (' . implode(' OR ', $conditions) . ')' . $order . ' LIMIT ' . $limit);
    $stmt->execute($parameters);
    return $stmt->fetchAll(PDO::FETCH_ASSOC);
}

function admin_admin_user_query(PDO $pdo)
{
    $candidateTables = ['admin_users', 'users'];
    foreach ($candidateTables as $table) {
        if (!admin_table_exists($pdo, $table)) {
            continue;
        }

        $fields = ['id', 'name', 'username', 'email', 'password_hash', 'role', 'created_at'];
        $select = array_values(array_filter($fields, fn($field) => admin_column_exists($pdo, $table, $field)));
        if (!in_array('id', $select, true) || !in_array('password_hash', $select, true)) {
            continue;
        }
        $identityColumns = array_values(array_intersect(['email', 'username'], $select));
        if (!$identityColumns) {
            continue;
        }
        // If users table has role column, require role in select. If users table does not have role column, do not skip.
        if ($table !== 'admin_users' && admin_column_exists($pdo, $table, 'role') && !in_array('role', $select, true)) {
            continue;
        }

        $columns = implode(', ', array_map(fn($field) => '`' . str_replace('`', '', $field) . '`', $select));
        $columns .= ", '$table' AS `_source_table`";
        
        $identityWhere = implode(' OR ', array_map(fn($field) => '`' . $field . '` = ?', $identityColumns));
        return "SELECT $columns FROM `$table` WHERE $identityWhere LIMIT 1";
    }

    return null;
}

function admin_user_is_admin(PDO $pdo, array $user): bool
{
    if (($user['_source_table'] ?? '') === 'admin_users') {
        return true;
    }
    if (!empty($user['role'])) {
        $role = strtolower(trim((string) $user['role']));
        if (in_array($role, ['admin', 'administrator', 'super_admin'], true)) {
            return true;
        }
    }
    // If users table does NOT have a role column in schema, allow ID 1 or username/email matching admin
    if (!admin_column_exists($pdo, 'users', 'role')) {
        $id = (int) ($user['id'] ?? 0);
        $email = strtolower(trim((string) ($user['email'] ?? '')));
        $username = strtolower(trim((string) ($user['username'] ?? '')));
        if ($id === 1 || strpos($email, 'admin') !== false || strpos($username, 'admin') !== false) {
            return true;
        }
    }
    return false;
}

function admin_value_or_default($value, $default = 'N/A')
{
    return $value !== null && $value !== '' ? $value : $default;
}

function admin_utc_date(?string $value): string
{
    if (!$value) {
        return 'N/A';
    }
    $ts = strtotime($value);
    if ($ts === false) {
        return 'N/A';
    }
    return date('d M Y', $ts);
}

function admin_login_attempt(PDO $pdo, string $email, string $password): ?array
{
    $query = admin_admin_user_query($pdo);
    if ($query === null) {
        return null;
    }

    $stmt = $pdo->prepare($query);
    $stmt->execute([$email, $email]);
    $user = $stmt->fetch(PDO::FETCH_ASSOC);
    if (!$user) {
        return null;
    }

    $passwordHash = $user['password_hash'] ?? null;
    if (!$passwordHash || !password_verify($password, $passwordHash)) {
        return null;
    }

    if (!admin_user_is_admin($pdo, $user)) {
        return null;
    }

    return $user;
}

function admin_set_flash(string $type, string $message): void
{
    $_SESSION['admin_flash'] = ['type' => $type, 'message' => $message];
}

function admin_get_flash(): ?array
{
    $flash = $_SESSION['admin_flash'] ?? null;
    unset($_SESSION['admin_flash']);
    return $flash;
}

function admin_logged_in_user(): ?array
{
    if (empty($_SESSION['admin_logged_in']) || empty($_SESSION['admin_user_id'])) {
        return null;
    }

    return [
        'id' => $_SESSION['admin_user_id'],
        'name' => $_SESSION['admin_user_name'] ?? 'Admin',
        'email' => $_SESSION['admin_user_email'] ?? '',
    ];
}

function admin_log_action(PDO $pdo, string $action, ?string $details = null, ?int $userId = null): void
{
    try {
        if (!admin_table_exists($pdo, 'audit_logs')) {
            return;
        }
        $cols = admin_table_columns($pdo, 'audit_logs');
        $adminId = $userId ?? ($_SESSION['admin_user_id'] ?? null);
        $ip = $_SERVER['REMOTE_ADDR'] ?? '127.0.0.1';
        
        $data = [];
        if (in_array('user_id', $cols, true)) {
            $data['user_id'] = $adminId;
        } elseif (in_array('admin_user_id', $cols, true)) {
            $data['admin_user_id'] = $adminId;
        }
        
        if (in_array('action', $cols, true)) {
            $data['action'] = $action;
        }
        
        if (in_array('details', $cols, true)) {
            $data['details'] = $details;
        } elseif (in_array('description', $cols, true)) {
            $data['description'] = $details;
        }
        
        if (in_array('ip_address', $cols, true)) {
            $data['ip_address'] = $ip;
        } elseif (in_array('ip', $cols, true)) {
            $data['ip'] = $ip;
        }
        
        if (in_array('created_at', $cols, true)) {
            $data['created_at'] = date('Y-m-d H:i:s');
        }

        if (empty($data)) {
            return;
        }

        $colList = implode(', ', array_map(fn($k) => "`$k`", array_keys($data)));
        $valList = implode(', ', array_fill(0, count($data), '?'));
        $stmt = $pdo->prepare("INSERT INTO `audit_logs` ({$colList}) VALUES ({$valList})");
        $stmt->execute(array_values($data));
    } catch (Throwable $e) {
        error_log('admin_log_action failed: ' . $e->getMessage());
    }
}

function admin_pagination(int $total, int $perPage, int $currentPage, string $baseUrl, array $queryParams = []): string
{
    $totalPages = (int) ceil(max(1, $total) / max(1, $perPage));
    if ($totalPages <= 1) {
        return '';
    }

    $currentPage = max(1, min($currentPage, $totalPages));
    $html = '<div class="pagination-bar">';
    $from = (($currentPage - 1) * $perPage) + 1;
    $to = min($currentPage * $perPage, $total);
    $html .= '<span class="pagination-info">Showing <strong>' . $from . '</strong> to <strong>' . $to . '</strong> of <strong>' . $total . '</strong> records</span>';
    $html .= '<div class="pagination-links">';

    // Previous
    if ($currentPage > 1) {
        $prevParams = array_merge($queryParams, ['page' => $currentPage - 1]);
        $html .= '<a class="pagination-btn" href="' . htmlspecialchars(admin_url($baseUrl, $prevParams), ENT_QUOTES, 'UTF-8') . '">&larr; Prev</a>';
    } else {
        $html .= '<span class="pagination-btn disabled">&larr; Prev</span>';
    }

    // Page window
    $startPage = max(1, $currentPage - 2);
    $endPage = min($totalPages, $currentPage + 2);
    if ($startPage > 1) {
        $firstParams = array_merge($queryParams, ['page' => 1]);
        $html .= '<a class="pagination-btn" href="' . htmlspecialchars(admin_url($baseUrl, $firstParams), ENT_QUOTES, 'UTF-8') . '">1</a>';
        if ($startPage > 2) {
            $html .= '<span class="pagination-ellipsis">&hellip;</span>';
        }
    }

    for ($p = $startPage; $p <= $endPage; $p++) {
        $pageParams = array_merge($queryParams, ['page' => $p]);
        $activeClass = ($p === $currentPage) ? ' active' : '';
        $html .= '<a class="pagination-btn' . $activeClass . '" href="' . htmlspecialchars(admin_url($baseUrl, $pageParams), ENT_QUOTES, 'UTF-8') . '">' . $p . '</a>';
    }

    if ($endPage < $totalPages) {
        if ($endPage < $totalPages - 1) {
            $html .= '<span class="pagination-ellipsis">&hellip;</span>';
        }
        $lastParams = array_merge($queryParams, ['page' => $totalPages]);
        $html .= '<a class="pagination-btn" href="' . htmlspecialchars(admin_url($baseUrl, $lastParams), ENT_QUOTES, 'UTF-8') . '">' . $totalPages . '</a>';
    }

    // Next
    if ($currentPage < $totalPages) {
        $nextParams = array_merge($queryParams, ['page' => $currentPage + 1]);
        $html .= '<a class="pagination-btn" href="' . htmlspecialchars(admin_url($baseUrl, $nextParams), ENT_QUOTES, 'UTF-8') . '">Next &rarr;</a>';
    } else {
        $html .= '<span class="pagination-btn disabled">Next &rarr;</span>';
    }

    $html .= '</div></div>';
    return $html;
}

function admin_sort_header(string $column, string $label, string $currentSort, string $currentDir, string $baseUrl, array $queryParams = []): string
{
    $isSorted = ($currentSort === $column);
    $nextDir = ($isSorted && strtolower($currentDir) === 'asc') ? 'desc' : 'asc';
    $arrow = '';
    if ($isSorted) {
        $arrow = (strtolower($currentDir) === 'asc') ? ' &uarr;' : ' &darr;';
    }
    $urlParams = array_merge($queryParams, ['sort' => $column, 'dir' => $nextDir, 'page' => 1]);
    $url = htmlspecialchars(admin_url($baseUrl, $urlParams), ENT_QUOTES, 'UTF-8');
    return '<a href="' . $url . '" class="sort-header' . ($isSorted ? ' is-sorted' : '') . '">' . htmlspecialchars($label, ENT_QUOTES, 'UTF-8') . '<span class="sort-arrow">' . $arrow . '</span></a>';
}

function admin_format_duration(?string $start, ?string $end): string
{
    if (!$start) {
        return '—';
    }
    $startTime = strtotime($start);
    if ($startTime === false) {
        return '—';
    }
    $endTime = $end ? strtotime($end) : time();
    if ($endTime === false || $endTime < $startTime) {
        return '—';
    }
    $diff = $endTime - $startTime;
    if ($diff < 60) {
        return $diff . 's';
    }
    if ($diff < 3600) {
        $m = floor($diff / 60);
        $s = $diff % 60;
        return $m . 'm ' . $s . 's';
    }
    $h = floor($diff / 3600);
    $m = floor(($diff % 3600) / 60);
    return $h . 'h ' . $m . 'm';
}

function admin_format_bytes($bytes, int $precision = 2): string
{
    $bytes = (float) $bytes;
    if ($bytes <= 0) {
        return '0 B';
    }
    $units = ['B', 'KB', 'MB', 'GB', 'TB'];
    $i = floor(log($bytes, 1024));
    $i = min($i, count($units) - 1);
    return round($bytes / pow(1024, $i), $precision) . ' ' . $units[$i];
}

function admin_time_ago(?string $datetime): string
{
    if (!$datetime) {
        return 'Never';
    }
    $ts = strtotime($datetime);
    if ($ts === false) {
        return 'Never';
    }
    $diff = time() - $ts;
    if ($diff < 0) {
        return 'Just now';
    }
    if ($diff < 60) {
        return $diff . 's ago';
    }
    if ($diff < 3600) {
        return floor($diff / 60) . 'm ago';
    }
    if ($diff < 86400) {
        return floor($diff / 3600) . 'h ago';
    }
    if ($diff < 604800) {
        return floor($diff / 86400) . 'd ago';
    }
    return date('d M Y', $ts);
}
