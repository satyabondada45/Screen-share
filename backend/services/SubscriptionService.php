<?php
/**
 * SubscriptionService - Centralized subscription management and entitlement resolution
 * for DeskStream. Schema-adaptive to support legacy, transitional, and fully migrated databases.
 */

require_once __DIR__ . '/../config/database.php';

class SubscriptionService
{
    private static function getPdo(): PDO
    {
        global $pdo;
        if ($pdo instanceof PDO) {
            self::ensureDatabaseCompatibility($pdo);
            return $pdo;
        }
        throw new RuntimeException("Database connection unavailable.");
    }

    public static function columnExists(PDO $pdo, string $table, string $column): bool
    {
        static $cache = [];
        $key = "$table.$column";
        if (isset($cache[$key])) {
            return $cache[$key];
        }
        try {
            $stmt = $pdo->prepare("
                SELECT 1
                FROM information_schema.COLUMNS
                WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ? AND COLUMN_NAME = ?
                LIMIT 1
            ");
            $stmt->execute([$table, $column]);
            return $cache[$key] = (bool)$stmt->fetchColumn();
        } catch (\Throwable $e) {
            return false;
        }
    }

    public static function tableExists(PDO $pdo, string $table): bool
    {
        static $cache = [];
        if (isset($cache[$table])) {
            return $cache[$table];
        }
        try {
            $stmt = $pdo->prepare("
                SELECT 1
                FROM information_schema.TABLES
                WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ?
                LIMIT 1
            ");
            $stmt->execute([$table]);
            return $cache[$table] = (bool)$stmt->fetchColumn();
        } catch (\Throwable $e) {
            return false;
        }
    }

    private static function ensureDatabaseCompatibility(PDO $pdo): void
    {
        static $ensured = false;
        if ($ensured) {
            return;
        }
        $ensured = true;

        try {
            // 1. Align subscriptions table
            if (self::tableExists($pdo, 'subscriptions')) {
                $subCols = [
                    'user_id' => 'INT NULL',
                    'starts_at' => 'DATETIME NULL',
                    'expires_at' => 'DATETIME NULL',
                    'plan_snapshot' => 'LONGTEXT NULL',
                    'activation_source' => "VARCHAR(32) NOT NULL DEFAULT 'legacy'",
                    'activated_by' => 'INT NULL',
                    'activation_reason' => 'VARCHAR(500) NULL',
                ];
                foreach ($subCols as $col => $def) {
                    if (!self::columnExists($pdo, 'subscriptions', $col)) {
                        try {
                            $pdo->exec("ALTER TABLE `subscriptions` ADD COLUMN `$col` $def");
                        } catch (\Throwable $e) {
                            error_log("Note: ALTER TABLE subscriptions ADD COLUMN $col notice: " . $e->getMessage());
                        }
                    }
                }

                // If user_id exists in subscriptions and device_id exists, backfill user_id where possible
                if (self::columnExists($pdo, 'subscriptions', 'user_id') && self::columnExists($pdo, 'subscriptions', 'device_id') && self::tableExists($pdo, 'devices')) {
                    try {
                        $pdo->exec("
                            UPDATE `subscriptions` s
                            JOIN `devices` d ON (s.device_id = d.system_id OR s.device_id = d.device_uid OR s.device_id = CAST(d.id AS CHAR))
                            SET s.user_id = d.user_id
                            WHERE s.user_id IS NULL AND d.user_id IS NOT NULL
                        ");
                    } catch (\Throwable $e) {}
                }

                // If start_date/expiry_date exist, backfill starts_at/expires_at
                if (self::columnExists($pdo, 'subscriptions', 'starts_at') && self::columnExists($pdo, 'subscriptions', 'start_date')) {
                    try {
                        $pdo->exec("UPDATE `subscriptions` SET `starts_at` = `start_date` WHERE `starts_at` IS NULL AND `start_date` IS NOT NULL");
                    } catch (\Throwable $e) {}
                }
                if (self::columnExists($pdo, 'subscriptions', 'expires_at') && self::columnExists($pdo, 'subscriptions', 'expiry_date')) {
                    try {
                        $pdo->exec("UPDATE `subscriptions` SET `expires_at` = `expiry_date` WHERE `expires_at` IS NULL AND `expiry_date` IS NOT NULL");
                    } catch (\Throwable $e) {}
                }
            }

            // 2. Align plans table
            if (self::tableExists($pdo, 'plans')) {
                $planCols = [
                    'slug' => 'VARCHAR(50) NULL',
                    'duration_value' => 'INT NOT NULL DEFAULT 1',
                    'duration_unit' => "VARCHAR(20) NOT NULL DEFAULT 'months'",
                    'concurrent_sessions' => 'INT NOT NULL DEFAULT 1',
                    'max_devices' => 'INT NOT NULL DEFAULT 10',
                    'features' => 'LONGTEXT NULL',
                ];
                foreach ($planCols as $col => $def) {
                    if (!self::columnExists($pdo, 'plans', $col)) {
                        try {
                            $pdo->exec("ALTER TABLE `plans` ADD COLUMN `$col` $def");
                        } catch (\Throwable $e) {
                            error_log("Note: ALTER TABLE plans ADD COLUMN $col notice: " . $e->getMessage());
                        }
                    }
                }

                // Populate slug based on name if slug column exists
                if (self::columnExists($pdo, 'plans', 'slug')) {
                    try {
                        $pdo->exec("UPDATE `plans` SET `slug` = 'welcome' WHERE (`slug` IS NULL OR `slug` = '') AND LOWER(`name`) LIKE '%welcome%'");
                        $pdo->exec("UPDATE `plans` SET `slug` = 'free' WHERE (`slug` IS NULL OR `slug` = '') AND LOWER(`name`) LIKE '%free%'");
                        $pdo->exec("UPDATE `plans` SET `slug` = 'monthly' WHERE (`slug` IS NULL OR `slug` = '') AND LOWER(`name`) LIKE '%monthly%'");
                        $pdo->exec("UPDATE `plans` SET `slug` = 'yearly' WHERE (`slug` IS NULL OR `slug` = '') AND LOWER(`name`) LIKE '%yearly%'");
                    } catch (\Throwable $e) {}
                }

                // Update plan metadata safely if columns exist
                if (self::columnExists($pdo, 'plans', 'slug')) {
                    try {
                        $pdo->exec("UPDATE `plans` SET `name` = 'Welcome Plan offered by Friends Software Solutions' WHERE `slug` = 'welcome' AND `name` <> 'Welcome Plan offered by Friends Software Solutions'");
                    } catch (\Throwable $e) {}
                    if (self::columnExists($pdo, 'plans', 'duration_value') && self::columnExists($pdo, 'plans', 'duration_unit')) {
                        try {
                            $pdo->exec("UPDATE `plans` SET `duration_value` = 1, `duration_unit` = 'months', `duration_days` = 30 WHERE `slug` = 'monthly' AND `price` = 100 AND (`duration_value` = 30 OR `duration_unit` = 'days' OR `duration_unit` IS NULL)");
                            $pdo->exec("UPDATE `plans` SET `duration_value` = 1, `duration_unit` = 'years', `duration_days` = 365 WHERE `slug` = 'yearly' AND `price` = 1100 AND (`duration_value` = 365 OR `duration_unit` = 'days' OR `duration_unit` IS NULL)");
                        } catch (\Throwable $e) {}
                    }
                    if (self::columnExists($pdo, 'plans', 'max_devices')) {
                        try {
                            $pdo->exec("UPDATE `plans` SET `max_devices` = 100 WHERE `slug` = 'free' AND `max_devices` = 1");
                        } catch (\Throwable $e) {}
                    }
                }

                if (self::columnExists($pdo, 'plans', 'slug') && self::columnExists($pdo, 'plans', 'features')) {
                    try {
                        $planRows = $pdo->query("SELECT `id`, `slug`, `features` FROM `plans` WHERE `slug` IN ('welcome', 'free', 'monthly', 'yearly')")->fetchAll(PDO::FETCH_ASSOC);
                        foreach ($planRows as $planRow) {
                            $features = json_decode((string)($planRow['features'] ?? ''), true);
                            if (!is_array($features)) {
                                $features = [];
                            }
                            foreach (['audio' => true, 'chat' => true, 'file_transfer' => true] as $feature => $enabled) {
                                $features[$feature] ??= $enabled;
                            }

                            if ($planRow['slug'] === 'free') {
                                $features['recording'] = false;
                                $features['reverse_control'] = false;
                                $features['max_quality'] = '720p';
                                $features['concurrent_sessions'] = 1;
                            }

                            $stmt = $pdo->prepare("UPDATE `plans` SET `features` = ? WHERE `id` = ?");
                            $stmt->execute([json_encode($features, JSON_THROW_ON_ERROR), (int)$planRow['id']]);
                        }
                    } catch (\Throwable $e) {}
                }
            }

            // 3. Align payments table
            if (self::tableExists($pdo, 'payments')) {
                foreach ([
                    'gateway_order_id' => 'VARCHAR(100) NULL',
                    'gateway_payment_id' => 'VARCHAR(100) NULL',
                    'currency' => "CHAR(3) NOT NULL DEFAULT 'INR'",
                    'plan_id' => 'INT NULL',
                    'plan_snapshot' => 'LONGTEXT NULL',
                    'signature_verified_at' => 'DATETIME NULL',
                ] as $column => $definition) {
                    if (!self::columnExists($pdo, 'payments', $column)) {
                        try {
                            $pdo->exec("ALTER TABLE `payments` ADD COLUMN `$column` $definition");
                        } catch (\Throwable $e) {}
                    }
                }
            }

            // 4. Align connection_requests
            if (self::tableExists($pdo, 'connection_requests')) {
                foreach ([
                    'requester_user_id' => 'INT NULL',
                    'target_user_id' => 'INT NULL',
                    'requester_system_id' => 'VARCHAR(32) NULL',
                    'requester_name' => 'VARCHAR(255) NULL',
                    'target_system_id' => 'VARCHAR(32) NULL',
                    'expires_at' => 'DATETIME NULL',
                ] as $column => $definition) {
                    if (!self::columnExists($pdo, 'connection_requests', $column)) {
                        try {
                            $pdo->exec("ALTER TABLE `connection_requests` ADD COLUMN `$column` $definition");
                        } catch (\Throwable $e) {}
                    }
                }
            }

            // 5. Align sessions
            if (self::tableExists($pdo, 'sessions')) {
                foreach ([
                    'requester_user_id' => 'INT NULL',
                    'connection_request_id' => 'INT NULL',
                    'last_activity_at' => 'DATETIME NULL',
                    'requester_plan_slug' => 'VARCHAR(50) NULL',
                    'host_plan_slug' => 'VARCHAR(50) NULL',
                ] as $column => $definition) {
                    if (!self::columnExists($pdo, 'sessions', $column)) {
                        try {
                            $pdo->exec("ALTER TABLE `sessions` ADD COLUMN `$column` $definition");
                        } catch (\Throwable $e) {}
                    }
                }

                $indexExists = static function (string $index) use ($pdo): bool {
                    try {
                        $stmt = $pdo->prepare("SELECT 1 FROM information_schema.STATISTICS WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'sessions' AND INDEX_NAME = ? LIMIT 1");
                        $stmt->execute([$index]);
                        return (bool)$stmt->fetchColumn();
                    } catch (\Throwable $e) {
                        return false;
                    }
                };

                try {
                    if (!$indexExists('idx_sessions_requester_status')) {
                        $pdo->exec("ALTER TABLE `sessions` ADD INDEX `idx_sessions_requester_status` (`requester_user_id`, `status`)");
                    }
                    if (!$indexExists('uq_sessions_connection_request')) {
                        $pdo->exec("ALTER TABLE `sessions` ADD UNIQUE INDEX `uq_sessions_connection_request` (`connection_request_id`)");
                    }
                } catch (\Throwable $e) {}
            }
        } catch (\Throwable $e) {
            error_log('ensureDatabaseCompatibility caught error: ' . $e->getMessage());
        }
    }

    /**
     * Normalizes a raw database plan row with default fallback entitlements and slug.
     */
    public static function normalizePlanRow(array $plan): array
    {
        if (empty($plan['slug'])) {
            $nameLower = strtolower((string)($plan['name'] ?? ''));
            if (str_contains($nameLower, 'welcome')) {
                $plan['slug'] = 'welcome';
            } elseif (str_contains($nameLower, 'monthly')) {
                $plan['slug'] = 'monthly';
            } elseif (str_contains($nameLower, 'yearly')) {
                $plan['slug'] = 'yearly';
            } elseif (str_contains($nameLower, 'free')) {
                $plan['slug'] = 'free';
            } else {
                $plan['slug'] = preg_replace('/[^a-z0-9]+/', '-', $nameLower) ?: 'custom';
            }
        }
        $slug = $plan['slug'];
        $plan['duration_days'] = (int)($plan['duration_days'] ?? ($slug === 'yearly' ? 365 : ($slug === 'monthly' ? 30 : ($slug === 'welcome' ? 183 : 0))));
        $plan['duration_value'] = (int)($plan['duration_value'] ?? ($slug === 'yearly' ? 1 : ($slug === 'monthly' ? 1 : ($slug === 'welcome' ? 6 : 0))));
        $plan['duration_unit'] = (string)($plan['duration_unit'] ?? ($slug === 'yearly' ? 'years' : ($slug === 'monthly' ? 'months' : ($slug === 'welcome' ? 'months' : 'unlimited'))));
        $plan['concurrent_sessions'] = (int)($plan['concurrent_sessions'] ?? ($slug === 'free' ? 1 : 10));
        $plan['max_devices'] = (int)($plan['max_devices'] ?? 100);

        if (!empty($plan['features'])) {
            $plan['features_decoded'] = json_decode($plan['features'], true) ?: [];
        } else {
            $plan['features_decoded'] = [
                'audio' => true,
                'chat' => true,
                'file_transfer' => true,
                'recording' => $slug !== 'free',
                'reverse_control' => $slug !== 'free',
                'max_quality' => $slug === 'free' ? '720p' : 'full',
                'concurrent_sessions' => $plan['concurrent_sessions']
            ];
            $plan['features'] = json_encode($plan['features_decoded']);
        }
        return $plan;
    }

    /**
     * Get all active plans from the database.
     */
    public static function getActivePlans(): array
    {
        $pdo = self::getPdo();
        $stmt = $pdo->query("SELECT * FROM `plans` WHERE `status` = 'active' ORDER BY `price` ASC, `id` ASC");
        $plans = $stmt->fetchAll(PDO::FETCH_ASSOC);
        foreach ($plans as &$plan) {
            $plan = self::normalizePlanRow($plan);
        }
        return $plans;
    }

    /**
     * Get plan by slug or id.
     */
    public static function getPlan(string|int $identifier): ?array
    {
        $pdo = self::getPdo();
        $hasSlug = self::columnExists($pdo, 'plans', 'slug');
        if (is_numeric($identifier)) {
            $stmt = $pdo->prepare("SELECT * FROM `plans` WHERE `id` = ? LIMIT 1");
            $stmt->execute([(int)$identifier]);
        } else {
            $idStr = (string)$identifier;
            if ($hasSlug) {
                $stmt = $pdo->prepare("SELECT * FROM `plans` WHERE `slug` = ? OR `name` = ? OR LOWER(`name`) = ? LIMIT 1");
                $stmt->execute([$idStr, $idStr, strtolower($idStr)]);
            } else {
                $stmt = $pdo->prepare("SELECT * FROM `plans` WHERE `name` = ? OR LOWER(`name`) LIKE ? LIMIT 1");
                $stmt->execute([$idStr, '%' . strtolower($idStr) . '%']);
            }
        }
        $plan = $stmt->fetch(PDO::FETCH_ASSOC);
        if ($plan) {
            return self::normalizePlanRow($plan);
        }
        return null;
    }

    /**
     * Get the authoritative current subscription for a user.
     * Checks expiration in real time, marks expired records, and falls back to Free Plan.
     */
    public static function getUserSubscription(int $userId): array
    {
        $pdo = self::getPdo();

        $hasSubUserId = self::columnExists($pdo, 'subscriptions', 'user_id');
        $hasSubStartsAt = self::columnExists($pdo, 'subscriptions', 'starts_at');
        $hasSubExpiresAt = self::columnExists($pdo, 'subscriptions', 'expires_at');
        $hasPlanSlug = self::columnExists($pdo, 'plans', 'slug');

        $startCol = $hasSubStartsAt ? 's.starts_at' : (self::columnExists($pdo, 'subscriptions', 'start_date') ? 's.start_date AS starts_at' : 's.created_at AS starts_at');
        $expireCol = $hasSubExpiresAt ? 's.expires_at' : (self::columnExists($pdo, 'subscriptions', 'expiry_date') ? 's.expiry_date AS expires_at' : 'NULL AS expires_at');
        $planSlugCol = $hasPlanSlug ? 'p.slug AS plan_slug' : "CASE 
            WHEN LOWER(p.name) LIKE '%welcome%' THEN 'welcome'
            WHEN LOWER(p.name) LIKE '%monthly%' THEN 'monthly'
            WHEN LOWER(p.name) LIKE '%yearly%' THEN 'yearly'
            ELSE 'free'
        END AS plan_slug";

        $joinDevice = '';
        $userWhere = '';
        if ($hasSubUserId) {
            $userWhere = 's.user_id = ?';
        } else {
            $joinDevice = 'JOIN `devices` d ON (s.device_id = d.system_id OR s.device_id = d.device_uid OR s.device_id = CAST(d.id AS CHAR))';
            $userWhere = 'd.user_id = ?';
        }

        // 1. Check for current active subscription
        $sql = "
            SELECT s.*, 
                   {$startCol},
                   {$expireCol},
                   p.name AS plan_name, 
                   {$planSlugCol}, 
                   p.price AS plan_price, 
                   p.duration_days
            FROM `subscriptions` s
            JOIN `plans` p ON s.plan_id = p.id
            {$joinDevice}
            WHERE {$userWhere} AND s.status = 'active'
            ORDER BY s.id DESC
            LIMIT 1
        ";

        $stmt = $pdo->prepare($sql);
        $stmt->execute([$userId]);
        $sub = $stmt->fetch(PDO::FETCH_ASSOC);

        if ($sub) {
            // Check real-time expiry
            if (!empty($sub['expires_at'])) {
                $expiresTime = strtotime($sub['expires_at']);
                if ($expiresTime !== false && $expiresTime <= time()) {
                    // Subscription has expired
                    $updateStmt = $pdo->prepare("UPDATE `subscriptions` SET `status` = 'expired' WHERE `id` = ?");
                    $updateStmt->execute([$sub['id']]);

                    // Check if there is another valid active subscription
                    return self::getUserSubscription($userId);
                }
            }

            // Extract snapshot if available
            $snapshot = !empty($sub['plan_snapshot']) ? json_decode((string)$sub['plan_snapshot'], true) : null;
            if (is_array($snapshot) && !empty($snapshot['slug'])) {
                $sub['plan_name'] = (string)($snapshot['name'] ?? $sub['plan_name']);
                $sub['plan_slug'] = (string)$snapshot['slug'];
                $sub['plan_price'] = (string)($snapshot['price'] ?? $sub['plan_price']);
                foreach (['duration_days', 'duration_value', 'duration_unit', 'concurrent_sessions', 'max_devices'] as $field) {
                    if (array_key_exists($field, $snapshot)) {
                        $sub[$field] = $snapshot[$field];
                    }
                }
                if (isset($snapshot['features']) && is_array($snapshot['features'])) {
                    $sub['features'] = json_encode($snapshot['features'], JSON_THROW_ON_ERROR);
                    $sub['features_decoded'] = $snapshot['features'];
                }
            }

            if (empty($sub['plan_slug'])) {
                $planNameLower = strtolower((string)($sub['plan_name'] ?? ''));
                if (str_contains($planNameLower, 'welcome')) {
                    $sub['plan_slug'] = 'welcome';
                } elseif (str_contains($planNameLower, 'monthly')) {
                    $sub['plan_slug'] = 'monthly';
                } elseif (str_contains($planNameLower, 'yearly')) {
                    $sub['plan_slug'] = 'yearly';
                } else {
                    $sub['plan_slug'] = 'free';
                }
            }

            if (!isset($sub['features_decoded'])) {
                $sub['features_decoded'] = !empty($sub['features']) ? json_decode($sub['features'], true) : [];
            }
            if (empty($sub['features_decoded'])) {
                $sub['features_decoded'] = [
                    'audio' => true,
                    'chat' => true,
                    'file_transfer' => true,
                    'recording' => $sub['plan_slug'] !== 'free',
                    'reverse_control' => $sub['plan_slug'] !== 'free',
                    'max_quality' => $sub['plan_slug'] === 'free' ? '720p' : 'full',
                    'concurrent_sessions' => (int)($sub['concurrent_sessions'] ?? ($sub['plan_slug'] === 'free' ? 1 : 10))
                ];
            }
            return $sub;
        }

        // 2. If no active subscription, check if user is eligible for Welcome Plan
        $welcomeSub = self::assignWelcomePlanIfEligible($userId);
        if ($welcomeSub !== null) {
            return $welcomeSub;
        }

        // 3. Fall back to standard Free Plan
        return self::getEffectiveFreePlan($userId);
    }

    /**
     * Resolves effective Free Plan for user when no active paid or Welcome plan exists.
     */
    public static function getEffectiveFreePlan(int $userId): array
    {
        $freePlan = self::getPlan('free') ?? self::getPlan('Free Plan');
        if (!$freePlan) {
            $freePlan = [
                'id' => 0,
                'name' => 'Free Plan',
                'slug' => 'free',
                'price' => '0.00',
                'duration_days' => 0,
                'duration_value' => 0,
                'duration_unit' => 'unlimited',
                'max_devices' => 100,
                'concurrent_sessions' => 1,
                'features' => json_encode([
                    'recording' => false,
                    'reverse_control' => false,
                    'max_quality' => '720p',
                    'concurrent_sessions' => 1
                ]),
                'features_decoded' => [
                    'recording' => false,
                    'reverse_control' => false,
                    'max_quality' => '720p',
                    'concurrent_sessions' => 1
                ]
            ];
        }

        return [
            'id' => 0,
            'user_id' => $userId,
            'plan_id' => $freePlan['id'],
            'status' => 'active',
            'starts_at' => date('Y-m-d H:i:s'),
            'expires_at' => null,
            'created_at' => date('Y-m-d H:i:s'),
            'plan_name' => $freePlan['name'],
            'plan_slug' => $freePlan['slug'] ?? 'free',
            'plan_price' => $freePlan['price'] ?? '0.00',
            'duration_days' => $freePlan['duration_days'] ?? 0,
            'duration_value' => $freePlan['duration_value'] ?? 0,
            'duration_unit' => $freePlan['duration_unit'] ?? 'unlimited',
            'concurrent_sessions' => $freePlan['concurrent_sessions'] ?? 1,
            'max_devices' => $freePlan['max_devices'] ?? 100,
            'features' => $freePlan['features'] ?? '',
            'features_decoded' => $freePlan['features_decoded'] ?? [],
            'is_fallback' => true
        ];
    }

    private static function calculatePlanExpiry(DateTimeImmutable $startAt, int $value, string $unit): ?DateTimeImmutable
    {
        if ($unit === 'unlimited' || $value <= 0) {
            return null;
        }

        if ($unit === 'months') {
            $expiry = $startAt->modify('first day of this month')->modify('+' . $value . ' months');
            $day = min((int)$startAt->format('d'), (int)$expiry->modify('last day of this month')->format('d'));
            return $expiry
                ->setDate((int)$expiry->format('Y'), (int)$expiry->format('n'), $day)
                ->setTime((int)$startAt->format('H'), (int)$startAt->format('i'), (int)$startAt->format('s'));
        }

        if ($unit === 'years') {
            $expiry = $startAt->setDate(
                (int)$startAt->format('Y') + $value,
                (int)$startAt->format('n'),
                1
            );
            $day = min((int)$startAt->format('d'), (int)$expiry->modify('last day of this month')->format('d'));
            return $expiry
                ->setDate((int)$expiry->format('Y'), (int)$expiry->format('n'), $day)
                ->setTime((int)$startAt->format('H'), (int)$startAt->format('i'), (int)$startAt->format('s'));
        }

        $expiry = $startAt->modify('+' . $value . ' days');
        return $expiry->setTime((int)$startAt->format('H'), (int)$startAt->format('i'), (int)$startAt->format('s'));
    }

    /**
     * Automatically assigns the Welcome Plan to eligible newly registered accounts.
     * Uses the currently configured Welcome duration at registration time so later plan edits do not mutate existing users.
     * Idempotent: Never grants twice if the user has ever received it before.
     */
    public static function assignWelcomePlanIfEligible(int $userId, bool $throwOnFailure = false): ?array
    {
        $pdo = self::getPdo();

        // 1. Verify user exists
        $userStmt = $pdo->prepare("SELECT id, created_at FROM `users` WHERE `id` = ? LIMIT 1");
        $userStmt->execute([$userId]);
        $user = $userStmt->fetch(PDO::FETCH_ASSOC);
        if (!$user) {
            if ($throwOnFailure) {
                throw new InvalidArgumentException("User account #{$userId} does not exist.");
            }
            return null;
        }

        $hasSubUserId = self::columnExists($pdo, 'subscriptions', 'user_id');
        $hasPlanSlug = self::columnExists($pdo, 'plans', 'slug');

        $whereClause = $hasSubUserId ? "s.user_id = ?" : "d.user_id = ?";
        $joinDev = $hasSubUserId ? "" : "JOIN `devices` d ON (s.device_id = d.system_id OR s.device_id = d.device_uid OR s.device_id = CAST(d.id AS CHAR))";
        $planFilter = $hasPlanSlug ? "(p.slug = 'welcome' OR LOWER(p.name) LIKE '%welcome%')" : "LOWER(p.name) LIKE '%welcome%'";

        // 2. Check if user has EVER had a Welcome Plan subscription in history
        $checkStmt = $pdo->prepare("
            SELECT 1 FROM `subscriptions` s
            JOIN `plans` p ON s.plan_id = p.id
            {$joinDev}
            WHERE {$whereClause} AND {$planFilter}
            LIMIT 1
        ");
        $checkStmt->execute([$userId]);
        if ($checkStmt->fetchColumn()) {
            return null;
        }

        $welcomePlan = self::getPlan('welcome') ?? self::getPlan('Welcome Plan');
        if (!$welcomePlan) {
            error_log("Welcome Plan configuration missing in database plans table.");
            if ($throwOnFailure) {
                throw new RuntimeException("Welcome Plan configuration missing in database.");
            }
            return null;
        }

        if (($welcomePlan['status'] ?? '') !== 'active') {
            error_log("Welcome Plan (ID #{$welcomePlan['id']}) is currently inactive in database.");
            if ($throwOnFailure) {
                throw new RuntimeException("Welcome Plan is currently inactive.");
            }
            return null;
        }

        $durationValue = (int)($welcomePlan['duration_value'] ?? 6);
        $durationUnit = strtolower((string)($welcomePlan['duration_unit'] ?? 'months'));
        $now = new DateTimeImmutable('now', new DateTimeZone('UTC'));
        $startsAt = $now->format('Y-m-d H:i:s');
        $expiresObject = self::calculatePlanExpiry($now, $durationValue, $durationUnit);
        $expiresAt = $expiresObject ? $expiresObject->format('Y-m-d H:i:s') : null;

        $snapshot = [
            'name' => (string)($welcomePlan['name'] ?? 'Welcome Plan offered by Friends Software Solutions'),
            'slug' => 'welcome',
            'price' => (string)($welcomePlan['price'] ?? '0'),
            'duration_value' => $durationValue,
            'duration_unit' => $durationUnit,
            'duration_days' => (int)($welcomePlan['duration_days'] ?? 0),
            'concurrent_sessions' => (int)($welcomePlan['concurrent_sessions'] ?? 10),
            'max_devices' => (int)($welcomePlan['max_devices'] ?? 100),
            'features' => !empty($welcomePlan['features_decoded']) ? $welcomePlan['features_decoded'] : [],
        ];

        try {
            $snapshotJson = json_encode($snapshot, JSON_THROW_ON_ERROR);

            $insertCols = ['plan_id', 'status'];
            $insertPlaceholders = ['?', '?'];
            $insertParams = [$welcomePlan['id'], 'active'];

            if ($hasSubUserId) {
                $insertCols[] = 'user_id';
                $insertPlaceholders[] = '?';
                $insertParams[] = $userId;
            }

            if (self::columnExists($pdo, 'subscriptions', 'device_id')) {
                // Find device for user if any
                $devStmt = $pdo->prepare("SELECT system_id, device_uid FROM `devices` WHERE `user_id` = ? ORDER BY id DESC LIMIT 1");
                $devStmt->execute([$userId]);
                $dev = $devStmt->fetch(PDO::FETCH_ASSOC);
                $devId = $dev['system_id'] ?? ($dev['device_uid'] ?? "user_$userId");
                $insertCols[] = 'device_id';
                $insertPlaceholders[] = '?';
                $insertParams[] = $devId;
            }

            if (self::columnExists($pdo, 'subscriptions', 'starts_at')) {
                $insertCols[] = 'starts_at';
                $insertPlaceholders[] = '?';
                $insertParams[] = $startsAt;
            }
            if (self::columnExists($pdo, 'subscriptions', 'start_date')) {
                $insertCols[] = 'start_date';
                $insertPlaceholders[] = '?';
                $insertParams[] = substr($startsAt, 0, 10);
            }

            if (self::columnExists($pdo, 'subscriptions', 'expires_at')) {
                $insertCols[] = 'expires_at';
                $insertPlaceholders[] = '?';
                $insertParams[] = $expiresAt;
            }
            if (self::columnExists($pdo, 'subscriptions', 'expiry_date')) {
                $insertCols[] = 'expiry_date';
                $insertPlaceholders[] = '?';
                $insertParams[] = $expiresAt ? substr($expiresAt, 0, 10) : null;
            }

            if (self::columnExists($pdo, 'subscriptions', 'plan_snapshot')) {
                $insertCols[] = 'plan_snapshot';
                $insertPlaceholders[] = '?';
                $insertParams[] = $snapshotJson;
            }
            if (self::columnExists($pdo, 'subscriptions', 'activation_source')) {
                $insertCols[] = 'activation_source';
                $insertPlaceholders[] = '?';
                $insertParams[] = 'welcome_default';
            }
            if (self::columnExists($pdo, 'subscriptions', 'created_at')) {
                $insertCols[] = 'created_at';
                $insertPlaceholders[] = 'NOW()';
            }

            $colStr = implode(', ', array_map(fn($c) => "`$c`", $insertCols));
            $valStr = implode(', ', $insertPlaceholders);
            $insertSql = "INSERT INTO `subscriptions` ($colStr) VALUES ($valStr)";

            $insertStmt = $pdo->prepare($insertSql);
            $insertStmt->execute($insertParams);
            $subId = (int)$pdo->lastInsertId();

            if (self::tableExists($pdo, 'subscription_history')) {
                try {
                    $histCols = [];
                    $histVals = [];
                    $histParams = [];
                    if (self::columnExists($pdo, 'subscription_history', 'user_id')) {
                        $histCols[] = 'user_id'; $histVals[] = '?'; $histParams[] = $userId;
                    }
                    if (self::columnExists($pdo, 'subscription_history', 'subscription_id')) {
                        $histCols[] = 'subscription_id'; $histVals[] = '?'; $histParams[] = $subId;
                    }
                    if (self::columnExists($pdo, 'subscription_history', 'plan_id')) {
                        $histCols[] = 'plan_id'; $histVals[] = '?'; $histParams[] = $welcomePlan['id'];
                    }
                    if (self::columnExists($pdo, 'subscription_history', 'action')) {
                        $histCols[] = 'action'; $histVals[] = '?'; $histParams[] = 'welcome_activation';
                    }
                    if (self::columnExists($pdo, 'subscription_history', 'details')) {
                        $histCols[] = 'details'; $histVals[] = '?'; $histParams[] = "Automatic 6-month Welcome Plan activation through " . ($expiresAt ?: 'unlimited');
                    }
                    if (self::columnExists($pdo, 'subscription_history', 'created_at')) {
                        $histCols[] = 'created_at'; $histVals[] = 'NOW()';
                    }
                    if (!empty($histCols)) {
                        $hSql = "INSERT INTO `subscription_history` (" . implode(', ', array_map(fn($c) => "`$c`", $histCols)) . ") VALUES (" . implode(', ', $histVals) . ")";
                        $pdo->prepare($hSql)->execute($histParams);
                    }
                } catch (\Throwable $e) {
                    error_log('subscription_history insert error: ' . $e->getMessage());
                }
            }

            if (self::tableExists($pdo, 'audit_logs')) {
                try {
                    $ip = $_SERVER['REMOTE_ADDR'] ?? '127.0.0.1';
                    $auditSql = "INSERT INTO `audit_logs` (`user_id`, `action`, `details`, `ip_address`, `created_at`) VALUES (?, ?, ?, ?, NOW())";
                    $pdo->prepare($auditSql)->execute([$userId, 'welcome_plan_assigned', "Welcome Plan assigned to new account #{$userId}", $ip]);
                } catch (\Throwable $e) {
                    error_log('audit_logs insert error: ' . $e->getMessage());
                }
            }

            return [
                'id' => $subId,
                'user_id' => $userId,
                'plan_id' => $welcomePlan['id'],
                'status' => 'active',
                'starts_at' => $startsAt,
                'expires_at' => $expiresAt,
                'created_at' => date('Y-m-d H:i:s'),
                'plan_name' => $welcomePlan['name'],
                'plan_slug' => 'welcome',
                'plan_price' => $welcomePlan['price'],
                'duration_days' => $welcomePlan['duration_days'] ?? 0,
                'duration_value' => $durationValue,
                'duration_unit' => $durationUnit,
                'concurrent_sessions' => $welcomePlan['concurrent_sessions'] ?? 10,
                'max_devices' => $welcomePlan['max_devices'] ?? 100,
                'features' => $welcomePlan['features'],
                'features_decoded' => $welcomePlan['features_decoded'],
                'is_welcome_introductory' => true
            ];
        } catch (Throwable $e) {
            error_log("Failed to assign Welcome Plan to user {$userId}: " . $e->getMessage());
            if ($throwOnFailure) {
                throw $e;
            }
            return null;
        }
    }

    /**
     * Resolves effective user entitlements.
     */
    public static function getUserEntitlements(int $userId): array
    {
        $sub = self::getUserSubscription($userId);
        $features = $sub['features_decoded'] ?? [];
        $slug = $sub['plan_slug'] ?? 'free';

        // Free plan has explicit restrictions
        if ($slug === 'free' || !empty($sub['is_fallback'])) {
            return [
                'user_id' => $userId,
                'plan_id' => (int)($sub['plan_id'] ?? 0),
                'plan_name' => $sub['plan_name'] ?? 'Free Plan',
                'plan_slug' => 'free',
                'can_record' => false,
                'can_reverse_control' => false,
                'max_quality' => '720p',
                'max_concurrent_sessions' => 1,
                'max_devices' => 100,
                'starts_at' => $sub['starts_at'] ?? null,
                'expires_at' => null,
                'is_unlimited' => true,
                'is_fallback' => !empty($sub['is_fallback'])
            ];
        }

        // Welcome, Monthly, Yearly have full access policy
        return [
            'user_id' => $userId,
            'plan_id' => (int)$sub['plan_id'],
            'plan_name' => $sub['plan_name'],
            'plan_slug' => $slug,
            'can_record' => $features['recording'] ?? true,
            'can_reverse_control' => $features['reverse_control'] ?? true,
            'max_quality' => $features['max_quality'] ?? 'full',
            'max_concurrent_sessions' => (int)($sub['concurrent_sessions'] ?? 10),
            'max_devices' => (int)($sub['max_devices'] ?? 10),
            'starts_at' => $sub['starts_at'] ?? null,
            'expires_at' => $sub['expires_at'] ?? null,
            'is_unlimited' => empty($sub['expires_at']),
            'is_fallback' => false
        ];
    }

    /**
     * Authorizes a feature action on the server authorization path.
     */
    public static function authorizeAction(int $userId, string $action, array $params = []): array
    {
        $entitlements = self::getUserEntitlements($userId);

        switch ($action) {
            case 'recording':
                if (!$entitlements['can_record']) {
                    return [
                        'authorized' => false,
                        'code' => 'RECORDING_DISABLED',
                        'message' => 'Session recording is disabled on the Free Plan. Please upgrade to record sessions.',
                        'plan' => $entitlements['plan_name']
                    ];
                }
                return ['authorized' => true, 'plan' => $entitlements['plan_name']];

            case 'reverse_control':
                if (!$entitlements['can_reverse_control']) {
                    return [
                        'authorized' => false,
                        'code' => 'REVERSE_CONTROL_DISABLED',
                        'message' => 'Reverse control is disabled on the Free Plan. Please upgrade to use reverse control.',
                        'plan' => $entitlements['plan_name']
                    ];
                }
                return ['authorized' => true, 'plan' => $entitlements['plan_name']];

            case 'quality':
                $requestedHeight = (int)($params['height'] ?? 0);
                if ($entitlements['max_quality'] === '720p' && $requestedHeight > 720) {
                    return [
                        'authorized' => false,
                        'code' => 'QUALITY_CAPPED',
                        'max_quality' => '720p',
                        'message' => 'Streaming quality is capped at 720p on the Free Plan.',
                        'plan' => $entitlements['plan_name']
                    ];
                }
                return ['authorized' => true, 'max_quality' => $entitlements['max_quality'], 'plan' => $entitlements['plan_name']];

            case 'session_start':
                return self::checkConcurrentSessionLimit($userId, $params['request_token'] ?? null);

            default:
                return ['authorized' => true];
        }
    }

    /**
     * Atomically enforces concurrent session limits for an account.
     * Prevents race conditions by using a database transaction with FOR UPDATE.
     */
    public static function checkConcurrentSessionLimit(int $userId, ?string $currentRequestToken = null): array
    {
        $pdo = self::getPdo();
        $ownsTransaction = !$pdo->inTransaction();
        if ($ownsTransaction) {
            $pdo->beginTransaction();
        }

        try {
            $accountLock = $pdo->prepare("SELECT id FROM `users` WHERE id = ? FOR UPDATE");
            $accountLock->execute([$userId]);
            if (!$accountLock->fetchColumn()) {
                if ($ownsTransaction) {
                    $pdo->commit();
                }
                return [
                    'authorized' => false,
                    'code' => 'ACCOUNT_NOT_FOUND',
                    'message' => 'The account could not be found.'
                ];
            }

            $entitlements = self::getUserEntitlements($userId);
            $maxLimit = (int)$entitlements['max_concurrent_sessions'];
            if ($maxLimit > 10) {
                if ($ownsTransaction) {
                    $pdo->commit();
                }
                return ['authorized' => true, 'active_count' => 0, 'limit' => $maxLimit];
            }

            $currentRequestId = null;
            if (!empty($currentRequestToken)) {
                $currentRequest = $pdo->prepare("
                    SELECT id
                    FROM `connection_requests`
                    WHERE request_token = ?
                    LIMIT 1
                    FOR UPDATE
                ");
                $currentRequest->execute([$currentRequestToken]);
                $currentRequestId = $currentRequest->fetchColumn();
                $currentRequestId = $currentRequestId === false ? null : (int)$currentRequestId;
            }

            $sessionSql = "
                SELECT s.id, s.connection_request_id
                FROM `sessions` s
                LEFT JOIN `devices` d ON s.host_device_id = d.id
                WHERE s.status = 'active'
                  AND (d.user_id = ? OR s.requester_user_id = ?)
            ";
            $sessionBinds = [$userId, $userId];
            if ($currentRequestId !== null) {
                $sessionSql .= " AND (s.connection_request_id IS NULL OR s.connection_request_id <> ?)";
                $sessionBinds[] = $currentRequestId;
            }
            $sessionSql .= " FOR UPDATE";
            $sessionStmt = $pdo->prepare($sessionSql);
            $sessionStmt->execute($sessionBinds);
            $activeSessions = $sessionStmt->fetchAll(PDO::FETCH_ASSOC);
            $activeCount = count($activeSessions);
            $activeRequestIds = [];
            foreach ($activeSessions as $activeSession) {
                if ($activeSession['connection_request_id'] !== null) {
                    $activeRequestIds[(int)$activeSession['connection_request_id']] = true;
                }
            }

            $requestSql = "
                SELECT id
                FROM `connection_requests`
                WHERE (requester_user_id = ? OR target_user_id = ?)
                  AND status IN ('pending', 'accepted')
                  AND expires_at > UTC_TIMESTAMP()
            ";
            $requestBinds = [$userId, $userId];
            if ($currentRequestToken !== null && $currentRequestToken !== '') {
                $requestSql .= " AND request_token <> ?";
                $requestBinds[] = $currentRequestToken;
            }
            $requestSql .= " FOR UPDATE";
            $requestStmt = $pdo->prepare($requestSql);
            $requestStmt->execute($requestBinds);
            $pendingRequests = 0;
            while (($requestId = $requestStmt->fetchColumn()) !== false) {
                if (!isset($activeRequestIds[(int)$requestId])) {
                    $pendingRequests++;
                }
            }

            $totalActive = $activeCount + $pendingRequests;
            if ($totalActive >= $maxLimit) {
                $result = [
                    'authorized' => false,
                    'code' => 'CONCURRENT_SESSION_LIMIT_EXCEEDED',
                    'active_count' => $totalActive,
                    'limit' => $maxLimit,
                    'message' => "Maximum concurrent session limit reached. Your Free Plan allows 1 active session at a time.",
                    'plan' => $entitlements['plan_name']
                ];
            } else {
                $result = [
                    'authorized' => true,
                    'active_count' => $totalActive,
                    'limit' => $maxLimit,
                    'plan' => $entitlements['plan_name']
                ];
            }

            if ($ownsTransaction) {
                $pdo->commit();
            }
            return $result;
        } catch (Throwable $e) {
            if ($ownsTransaction && $pdo->inTransaction()) {
                $pdo->rollBack();
            }
            throw $e;
        }
    }

    public static function activateManualSubscription(int $userId, int $planId, int $adminUserId, string $reason = ''): array
    {
        $pdo = self::getPdo();

        $isAdmin = false;
        if (self::tableExists($pdo, 'admin_users')) {
            $adminStmt = $pdo->prepare("SELECT id FROM `admin_users` WHERE `id` = ? LIMIT 1");
            $adminStmt->execute([$adminUserId]);
            if ($adminStmt->fetchColumn()) {
                $isAdmin = true;
            }
        }
        if (!$isAdmin && self::tableExists($pdo, 'users')) {
            if (self::columnExists($pdo, 'users', 'role')) {
                $adminUser = $pdo->prepare("SELECT id, role FROM `users` WHERE `id` = ? LIMIT 1");
                $adminUser->execute([$adminUserId]);
                $adminRecord = $adminUser->fetch(PDO::FETCH_ASSOC);
                if ($adminRecord && in_array(strtolower((string)($adminRecord['role'] ?? '')), ['admin', 'administrator', 'super_admin'], true)) {
                    $isAdmin = true;
                }
            } else {
                if ($adminUserId === 1 || (!empty($_SESSION['admin_logged_in']) && (int)($_SESSION['admin_user_id'] ?? 0) === $adminUserId)) {
                    $isAdmin = true;
                }
            }
        }
        if (!$isAdmin) {
            throw new InvalidArgumentException('Manual activation requires an administrator account.');
        }

        // Validate target user exists
        $userCheckStmt = $pdo->prepare("SELECT id FROM `users` WHERE `id` = ? LIMIT 1");
        $userCheckStmt->execute([$userId]);
        if (!$userCheckStmt->fetchColumn()) {
            throw new InvalidArgumentException("User account #{$userId} does not exist.");
        }

        $plan = self::getPlan($planId);
        if (!$plan || $plan['status'] !== 'active') {
            throw new InvalidArgumentException('Selected plan is not available for activation.');
        }

        $now = new DateTimeImmutable('now', new DateTimeZone('UTC'));
        $startsAt = $now->format('Y-m-d H:i:s');
        if (strtolower((string)($plan['slug'] ?? '')) === 'free' || strtolower((string)($plan['duration_unit'] ?? '')) === 'unlimited') {
            $expiresAt = null;
        } else {
            $expiresObject = self::calculatePlanExpiry($now, (int)($plan['duration_value'] ?? 1), strtolower((string)($plan['duration_unit'] ?? 'months')));
            $expiresAt = $expiresObject ? $expiresObject->format('Y-m-d H:i:s') : null;
        }

        $snapshot = [
            'name' => (string)($plan['name'] ?? ''),
            'slug' => (string)($plan['slug'] ?? ''),
            'price' => (string)($plan['price'] ?? '0'),
            'duration_value' => (int)($plan['duration_value'] ?? 0),
            'duration_unit' => strtolower((string)($plan['duration_unit'] ?? 'days')),
            'duration_days' => (int)($plan['duration_days'] ?? 0),
            'concurrent_sessions' => (int)($plan['concurrent_sessions'] ?? 10),
            'max_devices' => (int)($plan['max_devices'] ?? 10),
            'features' => !empty($plan['features_decoded']) ? $plan['features_decoded'] : [],
        ];

        $hasSubUserId = self::columnExists($pdo, 'subscriptions', 'user_id');

        $pdo->beginTransaction();
        try {
            if ($hasSubUserId) {
                $expireStmt = $pdo->prepare("UPDATE `subscriptions` SET `status` = 'expired' WHERE `user_id` = ? AND `status` = 'active'");
                $expireStmt->execute([$userId]);
            } else {
                $devStmt = $pdo->prepare("SELECT system_id, device_uid FROM `devices` WHERE `user_id` = ? ORDER BY id DESC LIMIT 1");
                $devStmt->execute([$userId]);
                $dev = $devStmt->fetch(PDO::FETCH_ASSOC);
                $devId = $dev['system_id'] ?? ($dev['device_uid'] ?? null);
                if ($devId) {
                    $expireStmt = $pdo->prepare("UPDATE `subscriptions` SET `status` = 'expired' WHERE `device_id` = ? AND `status` = 'active'");
                    $expireStmt->execute([$devId]);
                }
            }

            $snapshotJson = json_encode($snapshot, JSON_THROW_ON_ERROR);
            $insertCols = ['plan_id', 'status', 'created_at'];
            $insertPlaceholders = ['?', '?', 'NOW()'];
            $insertParams = [$planId, 'active'];

            if ($hasSubUserId) {
                $insertCols[] = 'user_id';
                $insertPlaceholders[] = '?';
                $insertParams[] = $userId;
            }
            if (self::columnExists($pdo, 'subscriptions', 'device_id')) {
                $devStmt = $pdo->prepare("SELECT system_id, device_uid FROM `devices` WHERE `user_id` = ? ORDER BY id DESC LIMIT 1");
                $devStmt->execute([$userId]);
                $dev = $devStmt->fetch(PDO::FETCH_ASSOC);
                $devId = $dev['system_id'] ?? ($dev['device_uid'] ?? "user_$userId");
                $insertCols[] = 'device_id';
                $insertPlaceholders[] = '?';
                $insertParams[] = $devId;
            }
            if (self::columnExists($pdo, 'subscriptions', 'starts_at')) {
                $insertCols[] = 'starts_at';
                $insertPlaceholders[] = '?';
                $insertParams[] = $startsAt;
            }
            if (self::columnExists($pdo, 'subscriptions', 'start_date')) {
                $insertCols[] = 'start_date';
                $insertPlaceholders[] = '?';
                $insertParams[] = substr($startsAt, 0, 10);
            }
            if (self::columnExists($pdo, 'subscriptions', 'expires_at')) {
                $insertCols[] = 'expires_at';
                $insertPlaceholders[] = '?';
                $insertParams[] = $expiresAt;
            }
            if (self::columnExists($pdo, 'subscriptions', 'expiry_date')) {
                $insertCols[] = 'expiry_date';
                $insertPlaceholders[] = '?';
                $insertParams[] = $expiresAt ? substr($expiresAt, 0, 10) : null;
            }
            if (self::columnExists($pdo, 'subscriptions', 'activation_source')) {
                $insertCols[] = 'activation_source';
                $insertPlaceholders[] = '?';
                $insertParams[] = 'manual';
            }
            if (self::columnExists($pdo, 'subscriptions', 'activated_by')) {
                $insertCols[] = 'activated_by';
                $insertPlaceholders[] = '?';
                $insertParams[] = $adminUserId;
            }
            if (self::columnExists($pdo, 'subscriptions', 'activation_reason')) {
                $insertCols[] = 'activation_reason';
                $insertPlaceholders[] = '?';
                $insertParams[] = $reason;
            }
            if (self::columnExists($pdo, 'subscriptions', 'plan_name')) {
                $insertCols[] = 'plan_name';
                $insertPlaceholders[] = '?';
                $insertParams[] = $plan['name'];
            }
            if (self::columnExists($pdo, 'subscriptions', 'amount')) {
                $insertCols[] = 'amount';
                $insertPlaceholders[] = '?';
                $insertParams[] = (float)($plan['price'] ?? 0);
            }
            if (self::columnExists($pdo, 'subscriptions', 'payment_status')) {
                $insertCols[] = 'payment_status';
                $insertPlaceholders[] = '?';
                $insertParams[] = 'completed';
            }
            if (self::columnExists($pdo, 'subscriptions', 'plan_snapshot')) {
                $insertCols[] = 'plan_snapshot';
                $insertPlaceholders[] = '?';
                $insertParams[] = $snapshotJson;
            }

            $colStr = implode(', ', array_map(fn($c) => "`$c`", $insertCols));
            $valStr = implode(', ', $insertPlaceholders);
            $insertSql = "INSERT INTO `subscriptions` ($colStr) VALUES ($valStr)";

            $insertStmt = $pdo->prepare($insertSql);
            $insertStmt->execute($insertParams);
            $subscriptionId = (int)$pdo->lastInsertId();

            if (self::tableExists($pdo, 'subscription_history')) {
                try {
                    $histCols = [];
                    $histVals = [];
                    $histParams = [];
                    if (self::columnExists($pdo, 'subscription_history', 'user_id')) {
                        $histCols[] = 'user_id'; $histVals[] = '?'; $histParams[] = $userId;
                    }
                    if (self::columnExists($pdo, 'subscription_history', 'subscription_id')) {
                        $histCols[] = 'subscription_id'; $histVals[] = '?'; $histParams[] = $subscriptionId;
                    }
                    if (self::columnExists($pdo, 'subscription_history', 'plan_id')) {
                        $histCols[] = 'plan_id'; $histVals[] = '?'; $histParams[] = $planId;
                    }
                    if (self::columnExists($pdo, 'subscription_history', 'action')) {
                        $histCols[] = 'action'; $histVals[] = '?'; $histParams[] = 'admin_activation';
                    }
                    if (self::columnExists($pdo, 'subscription_history', 'details')) {
                        $histCols[] = 'details'; $histVals[] = '?'; $histParams[] = "Activated plan '{$plan['name']}' through " . ($expiresAt ?: 'unlimited');
                    }
                    if (self::columnExists($pdo, 'subscription_history', 'created_at')) {
                        $histCols[] = 'created_at'; $histVals[] = 'NOW()';
                    }
                    if (!empty($histCols)) {
                        $hSql = "INSERT INTO `subscription_history` (" . implode(', ', array_map(fn($c) => "`$c`", $histCols)) . ") VALUES (" . implode(', ', $histVals) . ")";
                        $pdo->prepare($hSql)->execute($histParams);
                    }
                } catch (\Throwable $e) {
                    error_log('subscription_history insert error: ' . $e->getMessage());
                }
            }

            if (self::tableExists($pdo, 'audit_logs')) {
                try {
                    $ip = $_SERVER['REMOTE_ADDR'] ?? '127.0.0.1';
                    $auditSql = "INSERT INTO `audit_logs` (`user_id`, `action`, `details`, `ip_address`, `created_at`) VALUES (?, ?, ?, ?, NOW())";
                    $pdo->prepare($auditSql)->execute([$adminUserId, 'manual_activation', "Activated plan '{$plan['name']}' for User #{$userId}", $ip]);
                } catch (\Throwable $e) {
                    error_log('audit_logs insert error: ' . $e->getMessage());
                }
            }

            $pdo->commit();

            return [
                'status' => 'success',
                'subscription_id' => $subscriptionId,
                'plan_name' => $plan['name'],
                'expires_at' => $expiresAt,
                'activation_source' => 'manual',
            ];
        } catch (Throwable $e) {
            if ($pdo->inTransaction()) {
                $pdo->rollBack();
            }
            throw $e;
        }
    }

    /**
     * Activates a paid subscription only after payment verification has confirmed payment.
     * Prevents unverified activations.
     */
    public static function activatePaidSubscription(
        int $userId, 
        int $planId, 
        string $transactionId, 
        float $amount, 
        string $paymentMethod = 'card'
    ): array {
        $pdo = self::getPdo();

        if (stripos($transactionId, 'CLIENT_SUPPLIED_') === 0 || stripos($transactionId, 'TEST_') === 0) {
            throw new LogicException('Paid subscription activation requires a server-verified payment reference; client-supplied transaction IDs are not accepted.');
        }

        $plan = self::getPlan($planId);
        if (!$plan || $plan['status'] !== 'active') {
            throw new InvalidArgumentException("Selected plan is not available for subscription.");
        }

        $now = new DateTime('now', new DateTimeZone('UTC'));
        $startsAt = $now->format('Y-m-d H:i:s');
        $expires = clone $now;

        $unit = $plan['duration_unit'] ?? 'days';
        $val = (int)($plan['duration_value'] ?? 30);

        if ($unit === 'unlimited' || $val === 0) {
            $expiresAt = null;
        } elseif ($unit === 'months') {
            $expires->add(new DateInterval("P{$val}M"));
            $expiresAt = $expires->format('Y-m-d H:i:s');
        } elseif ($unit === 'years') {
            $expires->add(new DateInterval("P{$val}Y"));
            $expiresAt = $expires->format('Y-m-d H:i:s');
        } else {
            $expires->add(new DateInterval("P{$val}D"));
            $expiresAt = $expires->format('Y-m-d H:i:s');
        }

        $hasSubUserId = self::columnExists($pdo, 'subscriptions', 'user_id');

        $pdo->beginTransaction();
        try {
            // 1. Insert verified payment record
            $payCols = ['amount', 'status', 'created_at'];
            $payPlaceholders = ['?', "'completed'", 'NOW()'];
            $payParams = [$amount];

            if (self::columnExists($pdo, 'payments', 'user_id')) {
                $payCols[] = 'user_id';
                $payPlaceholders[] = '?';
                $payParams[] = $userId;
            }
            if (self::columnExists($pdo, 'payments', 'transaction_id')) {
                $payCols[] = 'transaction_id';
                $payPlaceholders[] = '?';
                $payParams[] = $transactionId;
            }
            if (self::columnExists($pdo, 'payments', 'payment_method')) {
                $payCols[] = 'payment_method';
                $payPlaceholders[] = '?';
                $payParams[] = $paymentMethod;
            }

            $paySql = "INSERT INTO `payments` (" . implode(', ', array_map(fn($c) => "`$c`", $payCols)) . ") VALUES (" . implode(', ', $payPlaceholders) . ")";
            $payStmt = $pdo->prepare($paySql);
            $payStmt->execute($payParams);
            $paymentId = (int)$pdo->lastInsertId();

            // 2. Mark any current active subscription as expired/superseded
            if ($hasSubUserId) {
                $expireStmt = $pdo->prepare("UPDATE `subscriptions` SET `status` = 'expired' WHERE `user_id` = ? AND `status` = 'active'");
                $expireStmt->execute([$userId]);
            }

            // 3. Create new active subscription
            $subCols = ['plan_id', 'status', 'created_at'];
            $subPlaceholders = ['?', "'active'", 'NOW()'];
            $subParams = [$planId];

            if ($hasSubUserId) {
                $subCols[] = 'user_id';
                $subPlaceholders[] = '?';
                $subParams[] = $userId;
            }
            if (self::columnExists($pdo, 'subscriptions', 'device_id')) {
                $devStmt = $pdo->prepare("SELECT system_id, device_uid FROM `devices` WHERE `user_id` = ? ORDER BY id DESC LIMIT 1");
                $devStmt->execute([$userId]);
                $dev = $devStmt->fetch(PDO::FETCH_ASSOC);
                $devId = $dev['system_id'] ?? ($dev['device_uid'] ?? "user_$userId");
                $subCols[] = 'device_id';
                $subPlaceholders[] = '?';
                $subParams[] = $devId;
            }
            if (self::columnExists($pdo, 'subscriptions', 'starts_at')) {
                $subCols[] = 'starts_at';
                $subPlaceholders[] = '?';
                $subParams[] = $startsAt;
            }
            if (self::columnExists($pdo, 'subscriptions', 'start_date')) {
                $subCols[] = 'start_date';
                $subPlaceholders[] = '?';
                $subParams[] = substr($startsAt, 0, 10);
            }
            if (self::columnExists($pdo, 'subscriptions', 'expires_at')) {
                $subCols[] = 'expires_at';
                $subPlaceholders[] = '?';
                $subParams[] = $expiresAt;
            }
            if (self::columnExists($pdo, 'subscriptions', 'expiry_date')) {
                $subCols[] = 'expiry_date';
                $subPlaceholders[] = '?';
                $subParams[] = $expiresAt ? substr($expiresAt, 0, 10) : null;
            }

            $subSql = "INSERT INTO `subscriptions` (" . implode(', ', array_map(fn($c) => "`$c`", $subCols)) . ") VALUES (" . implode(', ', $subPlaceholders) . ")";
            $subStmt = $pdo->prepare($subSql);
            $subStmt->execute($subParams);
            $subscriptionId = (int)$pdo->lastInsertId();

            // 4. Link subscription to payment record if column exists
            if (self::columnExists($pdo, 'payments', 'subscription_id')) {
                $pdo->prepare("UPDATE `payments` SET `subscription_id` = ? WHERE `id` = ?")->execute([$subscriptionId, $paymentId]);
            }

            $pdo->commit();

            return [
                'status' => 'success',
                'subscription_id' => $subscriptionId,
                'payment_id' => $paymentId,
                'plan_name' => $plan['name'],
                'starts_at' => $startsAt,
                'expires_at' => $expiresAt
            ];
        } catch (Throwable $e) {
            if ($pdo->inTransaction()) {
                $pdo->rollBack();
            }
            error_log("Failed to activate paid subscription for user {$userId}: " . $e->getMessage());
            throw $e;
        }
    }

    /**
     * Activates a server-verified Razorpay payment once per gateway payment reference.
     */
    public static function activateVerifiedRazorpayPayment(
        string $orderId,
        string $paymentId,
        array $gatewayPayment
    ): array {
        if ($orderId === '' || $paymentId === ''
            || ($gatewayPayment['id'] ?? '') !== $paymentId
            || ($gatewayPayment['order_id'] ?? '') !== $orderId
            || ($gatewayPayment['status'] ?? '') !== 'captured'
            || strtoupper((string)($gatewayPayment['currency'] ?? '')) !== 'INR'
            || !isset($gatewayPayment['amount'])
            || !is_numeric($gatewayPayment['amount'])
            || (int)$gatewayPayment['amount'] < 1) {
            throw new InvalidArgumentException('Razorpay payment details are invalid or incomplete.');
        }

        $pdo = self::getPdo();
        $pdo->beginTransaction();
        try {
            $paymentStmt = $pdo->prepare("
                SELECT `id`, `user_id`, `subscription_id`, `amount`, `status`,
                       `gateway_order_id`, `gateway_payment_id`, `currency`, `plan_id`, `plan_snapshot`
                FROM `payments`
                WHERE `gateway_order_id` = ?
                LIMIT 1
                FOR UPDATE
            ");
            $paymentStmt->execute([$orderId]);
            $payment = $paymentStmt->fetch(PDO::FETCH_ASSOC);
            if (!$payment) {
                throw new InvalidArgumentException('Razorpay order is not associated with a local payment record.');
            }

            $expectedAmount = (int)round((float)$payment['amount'] * 100);
            if ($expectedAmount !== (int)$gatewayPayment['amount']
                || strtoupper((string)$payment['currency']) !== 'INR'
                || (int)$payment['plan_id'] < 1) {
                throw new InvalidArgumentException('Razorpay payment does not match the recorded amount, currency, or plan.');
            }

            if ($payment['status'] === 'completed') {
                if ((string)$payment['gateway_payment_id'] !== $paymentId || empty($payment['subscription_id'])) {
                    throw new LogicException('This Razorpay order has already been applied to a different payment.');
                }
                $existingStmt = $pdo->prepare("
                    SELECT `id`, `starts_at`, `expires_at`
                    FROM `subscriptions`
                    WHERE `id` = ? AND `user_id` = ?
                    LIMIT 1
                ");
                $existingStmt->execute([(int)$payment['subscription_id'], (int)$payment['user_id']]);
                $existingSubscription = $existingStmt->fetch(PDO::FETCH_ASSOC);
                if (!$existingSubscription) {
                    throw new RuntimeException('The subscription linked to this completed payment is missing.');
                }
                $pdo->commit();
                return [
                    'status' => 'success',
                    'subscription_id' => (int)$existingSubscription['id'],
                    'payment_id' => (int)$payment['id'],
                    'starts_at' => $existingSubscription['starts_at'],
                    'expires_at' => $existingSubscription['expires_at'],
                    'idempotent' => true,
                ];
            }

            if ($payment['status'] !== 'pending'
                || (!empty($payment['gateway_payment_id']) && (string)$payment['gateway_payment_id'] !== $paymentId)) {
                throw new LogicException('This Razorpay payment record is no longer eligible for activation.');
            }

            $snapshot = json_decode((string)$payment['plan_snapshot'], true);
            if (!is_array($snapshot)
                || !in_array($snapshot['slug'] ?? '', ['monthly', 'yearly'], true)
                || (int)($snapshot['duration_value'] ?? 0) < 1
                || !in_array(strtolower((string)($snapshot['duration_unit'] ?? '')), ['months', 'years'], true)) {
                throw new RuntimeException('The payment order has no valid paid-plan snapshot.');
            }

            $startsAt = new DateTimeImmutable('now', new DateTimeZone('UTC'));
            $expiresAtObject = self::calculatePlanExpiry(
                $startsAt,
                (int)$snapshot['duration_value'],
                strtolower((string)$snapshot['duration_unit'])
            );
            if ($expiresAtObject === null) {
                throw new RuntimeException('Paid plans must have a finite subscription duration.');
            }
            $startsAtString = $startsAt->format('Y-m-d H:i:s');
            $expiresAt = $expiresAtObject->format('Y-m-d H:i:s');

            $expireStmt = $pdo->prepare("
                UPDATE `subscriptions`
                SET `status` = 'expired'
                WHERE `user_id` = ? AND `status` = 'active'
            ");
            $expireStmt->execute([(int)$payment['user_id']]);

            $subscriptionStmt = $pdo->prepare("
                INSERT INTO `subscriptions`
                    (`user_id`, `plan_id`, `status`, `starts_at`, `expires_at`, `created_at`,
                     `plan_snapshot`, `activation_source`)
                VALUES (?, ?, 'active', ?, ?, UTC_TIMESTAMP(), ?, 'razorpay')
            ");
            $subscriptionStmt->execute([
                (int)$payment['user_id'],
                (int)$payment['plan_id'],
                $startsAtString,
                $expiresAt,
                json_encode($snapshot, JSON_THROW_ON_ERROR)
            ]);
            $subscriptionId = (int)$pdo->lastInsertId();

            $completeStmt = $pdo->prepare("
                UPDATE `payments`
                SET `status` = 'completed',
                    `gateway_payment_id` = ?,
                    `transaction_id` = ?,
                    `payment_method` = 'razorpay',
                    `subscription_id` = ?,
                    `signature_verified_at` = UTC_TIMESTAMP()
                WHERE `id` = ? AND `status` = 'pending'
            ");
            $completeStmt->execute([
                $paymentId,
                $paymentId,
                $subscriptionId,
                (int)$payment['id']
            ]);
            if ($completeStmt->rowCount() !== 1) {
                throw new RuntimeException('The pending payment record changed before activation completed.');
            }

            $pdo->commit();
            return [
                'status' => 'success',
                'subscription_id' => $subscriptionId,
                'payment_id' => (int)$payment['id'],
                'plan_name' => (string)($snapshot['name'] ?? $snapshot['slug']),
                'starts_at' => $startsAtString,
                'expires_at' => $expiresAt,
                'idempotent' => false,
            ];
        } catch (Throwable $error) {
            if ($pdo->inTransaction()) {
                $pdo->rollBack();
            }
            throw $error;
        }
    }
}
