<?php
if (session_status() === PHP_SESSION_NONE) {
    session_start();
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
    $stmt = $pdo->prepare(
        'SELECT 1 FROM information_schema.TABLES
         WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ?
         LIMIT 1'
    );
    $stmt->execute([$table]);
    return (bool) $stmt->fetchColumn();
}

function admin_column_exists(PDO $pdo, string $table, string $column): bool
{
    $stmt = $pdo->prepare(
        'SELECT 1 FROM information_schema.COLUMNS
         WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ? AND COLUMN_NAME = ?
         LIMIT 1'
    );
    $stmt->execute([$table, $column]);
    return (bool) $stmt->fetchColumn();
}

function admin_table_columns(PDO $pdo, string $table): array
{
    if (!preg_match('/^[A-Za-z0-9_]+$/D', $table)) {
        throw new InvalidArgumentException('Invalid database table name.');
    }

    $stmt = $pdo->prepare(
        'SELECT COLUMN_NAME FROM information_schema.COLUMNS
         WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ?
         ORDER BY ORDINAL_POSITION'
    );
    $stmt->execute([$table]);
    return $stmt->fetchAll(PDO::FETCH_COLUMN);
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
        if ($table !== 'admin_users' && !in_array('role', $select, true)) {
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
    $role = strtolower(trim((string) ($user['role'] ?? '')));
    return in_array($role, ['admin', 'administrator', 'super_admin'], true);
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
