<?php
// backend/api/subscriptions/current.php
header('Content-Type: application/json');
header('Access-Control-Allow-Origin: *');
header('Access-Control-Allow-Methods: GET, OPTIONS');
header('Access-Control-Allow-Headers: Content-Type, Authorization, X-DeskStream-Session, X-Auth-Token');

if ($_SERVER['REQUEST_METHOD'] === 'OPTIONS') {
    exit(0);
}

if (session_status() === PHP_SESSION_NONE) {
    session_start();
}

// Restore session from token header if session was not automatically restored via cookie
if (empty($_SESSION['user_id'])) {
    $token = null;
    if (!empty($_SERVER['HTTP_X_DESKSTREAM_SESSION'])) {
        $token = trim($_SERVER['HTTP_X_DESKSTREAM_SESSION']);
    } elseif (!empty($_SERVER['HTTP_X_AUTH_TOKEN'])) {
        $token = trim($_SERVER['HTTP_X_AUTH_TOKEN']);
    } elseif (!empty($_SERVER['HTTP_AUTHORIZATION']) && preg_match('/Bearer\s+(\S+)/i', $_SERVER['HTTP_AUTHORIZATION'], $m)) {
        $token = $m[1];
    }
    if ($token && preg_match('/^[a-zA-Z0-9,-]{16,128}$/', $token)) {
        if (session_status() === PHP_SESSION_ACTIVE) {
            session_write_close();
        }
        session_id($token);
        session_start();
    }
}

if (!class_exists('SubscriptionService')) {
    $serviceCandidates = [
        __DIR__ . '/../../services/SubscriptionService.php',
        __DIR__ . '/../../backend/services/SubscriptionService.php',
        __DIR__ . '/../../../services/SubscriptionService.php',
        __DIR__ . '/../../../backend/services/SubscriptionService.php',
    ];
    foreach ($serviceCandidates as $candidate) {
        if (is_file($candidate)) {
            require_once $candidate;
            break;
        }
    }
}

$sessionUserId = filter_var(
    $_SESSION['user_id'] ?? null,
    FILTER_VALIDATE_INT,
    ['options' => ['min_range' => 1]]
);
if ($sessionUserId === false) {
    http_response_code(401);
    echo json_encode([
        'status' => 'error',
        'message' => 'Unauthorized: Please sign in to view subscription details.'
    ]);
    exit();
}
$userId = (int)$sessionUserId;

try {
    global $pdo;

    // Helper functions for schema safety
    $columnExists = static function (PDO $db, string $table, string $column): bool {
        try {
            $stmt = $db->prepare("
                SELECT 1 FROM information_schema.COLUMNS
                WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ? AND COLUMN_NAME = ?
                LIMIT 1
            ");
            $stmt->execute([$table, $column]);
            return (bool)$stmt->fetchColumn();
        } catch (\Throwable $e) {
            return false;
        }
    };

    $userCols = ['id', 'username', 'email'];
    $hasUniqueUserId = $columnExists($pdo, 'users', 'unique_user_id');
    $uniqueSelect = $hasUniqueUserId ? ', unique_user_id' : ', NULL AS unique_user_id';

    $accountStmt = $pdo->prepare("SELECT id, username, email {$uniqueSelect} FROM `users` WHERE `id` = ? LIMIT 1");
    $accountStmt->execute([$userId]);
    $account = $accountStmt->fetch(PDO::FETCH_ASSOC);
    if (!$account) {
        http_response_code(401);
        echo json_encode(['status' => 'error', 'message' => 'The signed-in account could not be found.']);
        exit();
    }

    $sub = SubscriptionService::getUserSubscription($userId);
    $entitlements = SubscriptionService::getUserEntitlements($userId);
    $planSlug = strtolower((string)($sub['plan_slug'] ?? 'free'));
    
    // Resolve display name for plan
    $planNames = [
        'welcome' => 'Welcome Plan',
        'free' => 'Free Plan',
        'monthly' => 'Monthly Plan',
        'yearly' => 'Yearly Plan'
    ];
    $displayPlanName = $planNames[$planSlug] ?? (!empty($sub['plan_name']) ? (string)$sub['plan_name'] : 'Free Plan');

    $startsAt = $sub['starts_at'] ?? ($sub['start_date'] ?? null);
    $expiresAt = $sub['expires_at'] ?? ($sub['expiry_date'] ?? null);
    $previous = null;
    $previousExpiry = null;
    $previousPlanName = null;

    if ($planSlug === 'free') {
        $hasSubUserId = $columnExists($pdo, 'subscriptions', 'user_id');
        $hasSubStartsAt = $columnExists($pdo, 'subscriptions', 'starts_at');
        $hasSubExpiresAt = $columnExists($pdo, 'subscriptions', 'expires_at');
        $hasSubSnapshot = $columnExists($pdo, 'subscriptions', 'plan_snapshot');
        $hasPlanSlug = $columnExists($pdo, 'plans', 'slug');

        $startCol = $hasSubStartsAt ? 's.starts_at' : ($columnExists($pdo, 'subscriptions', 'start_date') ? 's.start_date AS starts_at' : 'NULL AS starts_at');
        $expireCol = $hasSubExpiresAt ? 's.expires_at' : ($columnExists($pdo, 'subscriptions', 'expiry_date') ? 's.expiry_date AS expires_at' : 'NULL AS expires_at');
        $orderCol = $hasSubExpiresAt ? 's.expires_at' : ($columnExists($pdo, 'subscriptions', 'expiry_date') ? 's.expiry_date' : 's.id');
        $snapCol = $hasSubSnapshot ? 's.plan_snapshot' : 'NULL AS plan_snapshot';
        $slugCol = $hasPlanSlug ? 'p.slug' : "LOWER(p.name) AS slug";

        $joinDevice = '';
        $userWhere = '';
        if ($hasSubUserId) {
            $userWhere = 's.user_id = ?';
        } else {
            $joinDevice = 'JOIN `devices` d ON (s.device_id = d.system_id OR s.device_id = d.device_uid OR s.device_id = CAST(d.id AS CHAR))';
            $userWhere = 'd.user_id = ?';
        }

        $planFilter = $hasPlanSlug 
            ? "p.slug IN ('welcome', 'monthly', 'yearly')"
            : "(LOWER(p.name) LIKE '%welcome%' OR LOWER(p.name) LIKE '%monthly%' OR LOWER(p.name) LIKE '%yearly%')";

        $historySql = "
            SELECT {$startCol}, {$expireCol}, {$snapCol}, {$slugCol}, p.name
            FROM `subscriptions` s
            JOIN `plans` p ON p.id = s.plan_id
            {$joinDevice}
            WHERE {$userWhere}
              AND s.status = 'expired'
              AND {$planFilter}
            ORDER BY {$orderCol} DESC, s.id DESC
            LIMIT 1
        ";

        try {
            $historyStmt = $pdo->prepare($historySql);
            $historyStmt->execute([$userId]);
            $history = $historyStmt->fetch(PDO::FETCH_ASSOC);
            if ($history && !empty($history['expires_at'])) {
                $historySnapshot = !empty($history['plan_snapshot']) ? json_decode((string)$history['plan_snapshot'], true) : [];
                $historySlug = strtolower((string)($historySnapshot['slug'] ?? $history['slug'] ?? ''));
                $histName = $planNames[$historySlug] ?? (!empty($historySnapshot['name']) ? $historySnapshot['name'] : (!empty($history['name']) ? $history['name'] : 'Previous Plan'));
                
                try {
                    $dt = new DateTimeImmutable((string)$history['expires_at'], new DateTimeZone('UTC'));
                    $formattedHistExpiry = $dt->format('Y-m-d\TH:i:s\Z');
                } catch (\Throwable $e) {
                    $formattedHistExpiry = (string)$history['expires_at'];
                }

                $previous = [
                    'plan_name' => $histName,
                    'starts_at' => $history['starts_at'],
                    'expires_at' => $formattedHistExpiry
                ];
                $previousExpiry = $previous['expires_at'];
                $previousPlanName = $previous['plan_name'];
            }
        } catch (\Throwable $histEx) {
            error_log('Failed to query subscription history for user ' . $userId . ': ' . $histEx->getMessage());
        }
    }

    $formattedStartsAt = null;
    if ($startsAt) {
        try {
            $formattedStartsAt = (new DateTimeImmutable((string)$startsAt, new DateTimeZone('UTC')))->format('Y-m-d\TH:i:s\Z');
        } catch (\Throwable $e) {
            $formattedStartsAt = (string)$startsAt;
        }
    }

    $formattedExpiresAt = null;
    if ($expiresAt) {
        try {
            $formattedExpiresAt = (new DateTimeImmutable((string)$expiresAt, new DateTimeZone('UTC')))->format('Y-m-d\TH:i:s\Z');
        } catch (\Throwable $e) {
            $formattedExpiresAt = (string)$expiresAt;
        }
    }

    $serverNow = gmdate('Y-m-d\TH:i:s\Z');
    $currentStatus = $planSlug === 'free' ? 'free' : 'active';
    $email = (string)$account['email'];
    $identifier = (string)(!empty($account['unique_user_id']) ? $account['unique_user_id'] : $account['email']);

    echo json_encode([
        'status' => 'success',
        'current_status' => $currentStatus,
        'email' => $email,
        'account_identifier' => $identifier,
        'plan_slug' => $planSlug,
        'plan_name' => $displayPlanName,
        'starts_at' => $formattedStartsAt,
        'expires_at' => $formattedExpiresAt,
        'server_now' => $serverNow,
        'previous_plan_name' => $previousPlanName,
        'previous_expires_at' => $previousExpiry,
        'account' => [
            'id' => $userId,
            'username' => (string)$account['username'],
            'email' => $email,
            'identifier' => $identifier
        ],
        'subscription' => [
            'id' => (int)($sub['id'] ?? 0),
            'plan_id' => (int)($sub['plan_id'] ?? 0),
            'plan_name' => $displayPlanName,
            'plan_slug' => $planSlug,
            'price' => (float)($sub['plan_price'] ?? 0),
            'status' => $currentStatus,
            'starts_at' => $formattedStartsAt,
            'expires_at' => $formattedExpiresAt,
            'previous_plan_name' => $previousPlanName,
            'previous_expires_at' => $previousExpiry,
            'previous_expired_subscription' => $previous,
            'is_fallback' => !empty($sub['is_fallback']),
            'is_unlimited' => empty($expiresAt),
            'server_now' => $serverNow
        ],
        'entitlements' => $entitlements
    ]);
} catch (Throwable $e) {
    error_log('Current subscription lookup failed for signed-in user ' . $userId . ': ' . $e->getMessage());
    http_response_code(500);
    echo json_encode([
        'status' => 'error',
        'message' => 'Failed to load subscription details.'
    ]);
}
