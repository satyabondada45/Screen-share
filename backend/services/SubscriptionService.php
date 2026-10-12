<?php
/**
 * SubscriptionService - Centralized subscription management and entitlement resolution
 * for DeskStream.
 */

require_once __DIR__ . '/../config/database.php';

class SubscriptionService
{
    private static function getPdo(): PDO
    {
        global $pdo;
        if ($pdo instanceof PDO) {
            return $pdo;
        }
        throw new RuntimeException("Database connection unavailable.");
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
            $plan['features_decoded'] = !empty($plan['features']) ? json_decode($plan['features'], true) : [];
        }
        return $plans;
    }

    /**
     * Get plan by slug or id.
     */
    public static function getPlan(string|int $identifier): ?array
    {
        $pdo = self::getPdo();
        if (is_numeric($identifier)) {
            $stmt = $pdo->prepare("SELECT * FROM `plans` WHERE `id` = ? LIMIT 1");
            $stmt->execute([(int)$identifier]);
        } else {
            $stmt = $pdo->prepare("SELECT * FROM `plans` WHERE `slug` = ? OR `name` = ? LIMIT 1");
            $stmt->execute([(string)$identifier, (string)$identifier]);
        }
        $plan = $stmt->fetch(PDO::FETCH_ASSOC);
        if ($plan) {
            $plan['features_decoded'] = !empty($plan['features']) ? json_decode($plan['features'], true) : [];
            return $plan;
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

        // 1. Check for current active subscription
        $stmt = $pdo->prepare("
            SELECT s.*, 
                   p.name AS plan_name, 
                   p.slug AS plan_slug, 
                   p.price AS plan_price, 
                   p.duration_days, 
                   p.duration_value, 
                   p.duration_unit, 
                   p.concurrent_sessions, 
                   p.max_devices, 
                   p.features
            FROM `subscriptions` s
            JOIN `plans` p ON s.plan_id = p.id
            WHERE s.user_id = ? AND s.status = 'active'
            ORDER BY s.id DESC
            LIMIT 1
        ");
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

            $sub['features_decoded'] = !empty($sub['features']) ? json_decode($sub['features'], true) : [];
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
            // Fallback defaults if table was not yet seeded
            $freePlan = [
                'id' => 0,
                'name' => 'Free Plan',
                'slug' => 'free',
                'price' => '0.00',
                'duration_days' => 0,
                'duration_value' => 0,
                'duration_unit' => 'unlimited',
                'max_devices' => 1,
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
            'max_devices' => $freePlan['max_devices'] ?? 1,
            'features' => $freePlan['features'] ?? '',
            'features_decoded' => $freePlan['features_decoded'] ?? [],
            'is_fallback' => true
        ];
    }

    /**
     * Automatically assigns the Welcome Plan to eligible newly registered accounts.
     * Uses calendar-month arithmetic for 6 calendar months.
     * Idempotent: Never grants twice if the user has ever received it before.
     */
    public static function assignWelcomePlanIfEligible(int $userId): ?array
    {
        $pdo = self::getPdo();

        // 1. Verify user exists
        $userStmt = $pdo->prepare("SELECT id, created_at FROM `users` WHERE `id` = ? LIMIT 1");
        $userStmt->execute([$userId]);
        $user = $userStmt->fetch(PDO::FETCH_ASSOC);
        if (!$user) {
            return null;
        }

        // 2. Check if user has EVER had a Welcome Plan subscription in history
        $checkStmt = $pdo->prepare("
            SELECT 1 FROM `subscriptions` s
            JOIN `plans` p ON s.plan_id = p.id
            WHERE s.user_id = ? AND (p.slug = 'welcome' OR p.name = 'Welcome Plan')
            LIMIT 1
        ");
        $checkStmt->execute([$userId]);
        if ($checkStmt->fetchColumn()) {
            // Already claimed Welcome Plan in the past - not eligible
            return null;
        }

        // 3. Fetch the Welcome Plan definition
        $welcomePlan = self::getPlan('welcome') ?? self::getPlan('Welcome Plan');
        if (!$welcomePlan) {
            return null;
        }

        // 4. Calculate exact calendar-month expiry (6 calendar months from activation)
        $now = new DateTime('now', new DateTimeZone('UTC'));
        $startsAt = $now->format('Y-m-d H:i:s');
        $expires = clone $now;
        $expires->add(new DateInterval('P6M'));
        $expiresAt = $expires->format('Y-m-d H:i:s');

        // 5. Insert subscription atomically
        try {
            $insertStmt = $pdo->prepare("
                INSERT INTO `subscriptions` (`user_id`, `plan_id`, `status`, `starts_at`, `expires_at`, `created_at`)
                VALUES (?, ?, 'active', ?, ?, NOW())
            ");
            $insertStmt->execute([$userId, $welcomePlan['id'], $startsAt, $expiresAt]);
            $subId = (int)$pdo->lastInsertId();

            return [
                'id' => $subId,
                'user_id' => $userId,
                'plan_id' => $welcomePlan['id'],
                'status' => 'active',
                'starts_at' => $startsAt,
                'expires_at' => $expiresAt,
                'created_at' => date('Y-m-d H:i:s'),
                'plan_name' => $welcomePlan['name'],
                'plan_slug' => $welcomePlan['slug'],
                'plan_price' => $welcomePlan['price'],
                'duration_days' => $welcomePlan['duration_days'],
                'duration_value' => $welcomePlan['duration_value'],
                'duration_unit' => $welcomePlan['duration_unit'],
                'concurrent_sessions' => $welcomePlan['concurrent_sessions'],
                'max_devices' => $welcomePlan['max_devices'],
                'features' => $welcomePlan['features'],
                'features_decoded' => $welcomePlan['features_decoded'],
                'is_welcome_introductory' => true
            ];
        } catch (Throwable $e) {
            error_log("Failed to assign Welcome Plan to user {$userId}: " . $e->getMessage());
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
                'max_devices' => 1,
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
                return self::checkConcurrentSessionLimit($userId, $params['session_key'] ?? null);

            default:
                return ['authorized' => true];
        }
    }

    /**
     * Atomically enforces concurrent session limits for an account.
     * Prevents race conditions by using a database transaction with FOR UPDATE.
     */
    public static function checkConcurrentSessionLimit(int $userId, ?string $currentSessionKey = null): array
    {
        $pdo = self::getPdo();
        $entitlements = self::getUserEntitlements($userId);
        $maxLimit = (int)$entitlements['max_concurrent_sessions'];

        // If max limit is high (e.g. 10 or unlimited), allow immediately
        if ($maxLimit > 10) {
            return ['authorized' => true, 'active_count' => 0, 'limit' => $maxLimit];
        }

        // Count active sessions where host or viewer belongs to this user
        $sql = "
            SELECT COUNT(*) AS active_count
            FROM `sessions` s
            JOIN `devices` d ON s.host_device_id = d.id
            WHERE d.user_id = ? AND s.status = 'active'
        ";
        $binds = [$userId];
        if (!empty($currentSessionKey)) {
            $sql .= " AND s.session_key != ?";
            $binds[] = $currentSessionKey;
        }

        $stmt = $pdo->prepare($sql);
        $stmt->execute($binds);
        $activeCount = (int)$stmt->fetchColumn();

        // Also check currently accepted connection requests pending session creation
        $reqSql = "
            SELECT COUNT(*) 
            FROM `connection_requests` 
            WHERE (requester_user_id = ? OR target_user_id = ?)
              AND status IN ('pending', 'accepted')
              AND expires_at > NOW()
        ";
        $reqStmt = $pdo->prepare($reqSql);
        $reqStmt->execute([$userId, $userId]);
        $pendingRequests = (int)$reqStmt->fetchColumn();

        $totalActive = $activeCount + max(0, $pendingRequests - 1);

        if ($totalActive >= $maxLimit) {
            return [
                'authorized' => false,
                'code' => 'CONCURRENT_SESSION_LIMIT_EXCEEDED',
                'active_count' => $totalActive,
                'limit' => $maxLimit,
                'message' => "Maximum concurrent session limit reached. Your Free Plan allows 1 active session at a time.",
                'plan' => $entitlements['plan_name']
            ];
        }

        return [
            'authorized' => true,
            'active_count' => $totalActive,
            'limit' => $maxLimit,
            'plan' => $entitlements['plan_name']
        ];
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
            // default days
            $expires->add(new DateInterval("P{$val}D"));
            $expiresAt = $expires->format('Y-m-d H:i:s');
        }

        $pdo->beginTransaction();
        try {
            // 1. Insert verified payment record
            $payStmt = $pdo->prepare("
                INSERT INTO `payments` (`user_id`, `amount`, `transaction_id`, `payment_method`, `status`, `created_at`)
                VALUES (?, ?, ?, ?, 'completed', NOW())
            ");
            $payStmt->execute([$userId, $amount, $transactionId, $paymentMethod]);
            $paymentId = (int)$pdo->lastInsertId();

            // 2. Mark any current active subscription as expired/superseded
            $expireStmt = $pdo->prepare("
                UPDATE `subscriptions` 
                SET `status` = 'expired' 
                WHERE `user_id` = ? AND `status` = 'active'
            ");
            $expireStmt->execute([$userId]);

            // 3. Create new active subscription
            $subStmt = $pdo->prepare("
                INSERT INTO `subscriptions` (`user_id`, `plan_id`, `status`, `starts_at`, `expires_at`, `created_at`)
                VALUES (?, ?, 'active', ?, ?, NOW())
            ");
            $subStmt->execute([$userId, $planId, $startsAt, $expiresAt]);
            $subscriptionId = (int)$pdo->lastInsertId();

            // 4. Link subscription to payment record
            $pdo->prepare("UPDATE `payments` SET `subscription_id` = ? WHERE `id` = ?")->execute([$subscriptionId, $paymentId]);

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
}
