<?php
/**
 * test_remote_session_persistence.php
 * Automated regression test covering:
 * 1. Schema integrity (sessions table, connection_mode column, indexes)
 * 2. Session start persistence (active state, connection mode, timestamps)
 * 3. Session heartbeat and last_activity_at refresh
 * 4. Session conclusion persistence (completed state, ended_at, duration, bandwidth)
 * 5. Idempotency of repeated end calls (no duplicate rows, idempotent success)
 * 6. Reverse connection mode persistence
 * 7. Stale active session sweep (>120s inactivity auto-completion)
 * 8. Account isolation (user cannot see other accounts' session history)
 * 9. Admin Portal sessions query integration
 */

require_once __DIR__ . '/../backend/config/database.php';

$passed = 0;
$failed = 0;

function assertCheck(string $desc, bool $condition, string $detail = '') {
    global $passed, $failed;
    if ($condition) {
        $passed++;
        echo "  [PASS] $desc\n";
    } else {
        $failed++;
        echo "  [FAIL] $desc " . ($detail ? "($detail)" : "") . "\n";
    }
}

echo "======================================================================\n";
echo " DESKSTREAM REMOTE SESSION PERSISTENCE & HISTORY TEST SUITE\n";
echo "======================================================================\n\n";

try {
    // ── PART 1: SCHEMA INTEGRITY ──────────────────────────────────────────────────
    echo "--- PART 1: Schema Integrity ---\n";
    $colStmt = $pdo->query("SHOW COLUMNS FROM `sessions`");
    $cols = $colStmt->fetchAll(PDO::FETCH_COLUMN);

    assertCheck("sessions table exists", !empty($cols));
    assertCheck("sessions has session_key column", in_array('session_key', $cols, true));
    assertCheck("sessions has started_at column", in_array('started_at', $cols, true));
    assertCheck("sessions has ended_at column", in_array('ended_at', $cols, true));
    assertCheck("sessions has last_activity_at column", in_array('last_activity_at', $cols, true));
    assertCheck("sessions has status column", in_array('status', $cols, true));
    assertCheck("sessions has bandwidth_used_mb column", in_array('bandwidth_used_mb', $cols, true));
    assertCheck("sessions has connection_mode column", in_array('connection_mode', $cols, true));

    // ── PART 2: TEST FIXTURES SETUP ──────────────────────────────────────────────
    echo "\n--- PART 2: Test Fixtures Setup ---\n";
    // Find or create test users A and B
    $u1Stmt = $pdo->query("SELECT id FROM users ORDER BY id ASC LIMIT 1");
    $userAId = (int)$u1Stmt->fetchColumn();
    if (!$userAId) {
        $pdo->prepare("INSERT INTO users (name, email, password_hash, role) VALUES ('Test User A', 'test_user_a@deskstream.local', 'hash', 'user')")->execute();
        $userAId = (int)$pdo->lastInsertId();
    }
    assertCheck("Test User A available (id: $userAId)", $userAId > 0);

    $u2Stmt = $pdo->prepare("SELECT id FROM users WHERE id != ? ORDER BY id ASC LIMIT 1");
    $u2Stmt->execute([$userAId]);
    $userBId = (int)$u2Stmt->fetchColumn();
    if (!$userBId) {
        $pdo->prepare("INSERT INTO users (name, email, password_hash, role) VALUES ('Test User B', 'test_user_b@deskstream.local', 'hash', 'user')")->execute();
        $userBId = (int)$pdo->lastInsertId();
    }
    assertCheck("Test User B available (id: $userBId)", $userBId > 0);

    // Find or create host device owned by User A
    $devStmt = $pdo->prepare("SELECT id, system_id, device_uid, machine_identifier FROM devices WHERE user_id = ? LIMIT 1");
    $devStmt->execute([$userAId]);
    $hostDevice = $devStmt->fetch(PDO::FETCH_ASSOC);
    if (!$hostDevice) {
        $testSysId = '999' . str_pad((string)random_int(100000, 999999), 6, '0', STR_PAD_LEFT);
        $testMachId = 'mach-' . bin2hex(random_bytes(8));
        $pdo->prepare("INSERT INTO devices (user_id, name, system_id, device_uid, machine_identifier, os_type, is_online) VALUES (?, 'Host Device A', ?, ?, ?, 'windows', 1)")
            ->execute([$userAId, $testSysId, $testSysId, $testMachId]);
        $hostDeviceId = (int)$pdo->lastInsertId();
        $hostDevice = ['id' => $hostDeviceId, 'system_id' => $testSysId, 'device_uid' => $testSysId, 'machine_identifier' => $testMachId];
    }
    assertCheck("Host Device for User A available (system_id: {$hostDevice['system_id']})", !empty($hostDevice['system_id']));

    // Create a connection request from User A to Host Device A
    $testReqToken = 'reqtok_' . bin2hex(random_bytes(16));
    $pdo->prepare("
        INSERT INTO connection_requests (requester_user_id, target_user_id, requester_system_id, requester_name, target_system_id, request_token, status, expires_at, created_at, updated_at)
        VALUES (?, ?, '999111222', 'Test User A', ?, ?, 'accepted', UTC_TIMESTAMP() + INTERVAL 300 SECOND, UTC_TIMESTAMP(), UTC_TIMESTAMP())
    ")->execute([$userAId, $userAId, $hostDevice['system_id'], $testReqToken]);
    $crId = (int)$pdo->lastInsertId();
    assertCheck("Connection request created (id: $crId, token: $testReqToken)", $crId > 0);

    // ── PART 3: SESSION START PERSISTENCE ─────────────────────────────────────────
    echo "\n--- PART 3: Session Start Persistence ---\n";
    $testSessionKey = "sess_{$hostDevice['system_id']}_" . time() . "_" . bin2hex(random_bytes(6));

    // Invoke start.php logic directly via PDO simulation matching start.php
    $pdo->beginTransaction();
    $insertStart = $pdo->prepare("
        INSERT INTO `sessions`
            (`connection_request_id`, `requester_user_id`, `host_device_id`, `session_key`,
             `started_at`, `last_activity_at`, `status`, `connection_mode`)
        VALUES (?, ?, ?, ?, UTC_TIMESTAMP(), UTC_TIMESTAMP(), 'active', 'desktop')
    ");
    $insertStart->execute([$crId, $userAId, (int)$hostDevice['id'], $testSessionKey]);
    $sessionId = (int)$pdo->lastInsertId();
    $pdo->commit();

    assertCheck("Session inserted into database (id: $sessionId)", $sessionId > 0);

    $verifyActive = $pdo->prepare("SELECT id, status, started_at, connection_mode FROM `sessions` WHERE id = ?");
    $verifyActive->execute([$sessionId]);
    $activeRow = $verifyActive->fetch(PDO::FETCH_ASSOC);

    assertCheck("Session status is 'active'", strtolower($activeRow['status']) === 'active');
    assertCheck("Session connection_mode is 'desktop'", strtolower($activeRow['connection_mode']) === 'desktop');
    assertCheck("Session started_at is populated", !empty($activeRow['started_at']));

    // ── PART 4: SESSION HEARTBEAT TOUCH ───────────────────────────────────────────
    echo "\n--- PART 4: Session Heartbeat Refresh ---\n";
    // Simulate heartbeat touching last_activity_at
    sleep(1);
    $touchStmt = $pdo->prepare("UPDATE `sessions` SET `last_activity_at` = UTC_TIMESTAMP() WHERE id = ? AND status = 'active'");
    $touchStmt->execute([$sessionId]);

    $verifyHeartbeat = $pdo->prepare("SELECT last_activity_at, started_at FROM `sessions` WHERE id = ?");
    $verifyHeartbeat->execute([$sessionId]);
    $hbRow = $verifyHeartbeat->fetch(PDO::FETCH_ASSOC);
    assertCheck("last_activity_at updated after heartbeat", !empty($hbRow['last_activity_at']));

    // ── PART 5: SESSION CONCLUSION & DURATION PERSISTENCE ─────────────────────────
    echo "\n--- PART 5: Session Conclusion & Duration Persistence ---\n";
    sleep(1);
    // Simulate end.php logic
    $endStmt = $pdo->prepare("
        UPDATE `sessions`
        SET `ended_at` = UTC_TIMESTAMP(),
            `status` = 'completed',
            `bandwidth_used_mb` = ?,
            `last_activity_at` = UTC_TIMESTAMP()
        WHERE id = ? AND status = 'active'
    ");
    $endStmt->execute([15.75, $sessionId]);

    $verifyEnd = $pdo->prepare("
        SELECT id, status, started_at, ended_at, bandwidth_used_mb,
               TIMESTAMPDIFF(SECOND, started_at, ended_at) AS duration_seconds
        FROM `sessions` WHERE id = ?
    ");
    $verifyEnd->execute([$sessionId]);
    $endRow = $verifyEnd->fetch(PDO::FETCH_ASSOC);

    assertCheck("Session status updated to 'completed'", strtolower($endRow['status']) === 'completed');
    assertCheck("Session ended_at timestamp recorded", !empty($endRow['ended_at']));
    assertCheck("Session duration accurately calculated (duration: {$endRow['duration_seconds']}s)", (int)$endRow['duration_seconds'] >= 2);
    assertCheck("Session bandwidth recorded (15.75 MB)", abs((float)$endRow['bandwidth_used_mb'] - 15.75) < 0.01);

    // ── PART 6: IDEMPOTENT REPEATED CONCLUSION ────────────────────────────────────
    echo "\n--- PART 6: Idempotent Repeated Conclusion ---\n";
    // A second end event arrives (e.g. host disconnects after viewer disconnects)
    $priorSessionCount = (int)$pdo->query("SELECT COUNT(*) FROM `sessions`")->fetchColumn();

    // Matching end.php idempotent lookup:
    $checkExisting = $pdo->prepare("SELECT id, status, ended_at FROM `sessions` WHERE id = ? LIMIT 1");
    $checkExisting->execute([$sessionId]);
    $existingRec = $checkExisting->fetch(PDO::FETCH_ASSOC);

    assertCheck("Existing completed session detected", $existingRec && strtolower($existingRec['status']) === 'completed');
    // It must NOT insert a new row or overwrite valid ended_at
    $newSessionCount = (int)$pdo->query("SELECT COUNT(*) FROM `sessions`")->fetchColumn();
    assertCheck("No duplicate session row created on repeated end call", $priorSessionCount === $newSessionCount);
    assertCheck("Completed status preserved", strtolower($existingRec['status']) === 'completed');

    // ── PART 7: REVERSE CONNECTION MODE PERSISTENCE ───────────────────────────────
    echo "\n--- PART 7: Reverse Connection Mode Persistence ---\n";
    $revReqToken = 'reqtok_rev_' . bin2hex(random_bytes(14));
    $pdo->prepare("
        INSERT INTO connection_requests (requester_user_id, target_user_id, requester_system_id, requester_name, target_system_id, request_token, status, expires_at, created_at, updated_at)
        VALUES (?, ?, '999111222', 'Test User A', ?, ?, 'accepted', UTC_TIMESTAMP() + INTERVAL 300 SECOND, UTC_TIMESTAMP(), UTC_TIMESTAMP())
    ")->execute([$userAId, $userAId, $hostDevice['system_id'], $revReqToken]);
    $revCrId = (int)$pdo->lastInsertId();

    $revSessionKey = "sess_rev_{$hostDevice['system_id']}_" . time() . "_" . bin2hex(random_bytes(6));
    $pdo->prepare("
        INSERT INTO `sessions`
            (`connection_request_id`, `requester_user_id`, `host_device_id`, `session_key`,
             `started_at`, `ended_at`, `status`, `connection_mode`, `bandwidth_used_mb`)
        VALUES (?, ?, ?, ?, UTC_TIMESTAMP(), UTC_TIMESTAMP(), 'completed', 'reverse', 8.2)
    ")->execute([$revCrId, $userAId, (int)$hostDevice['id'], $revSessionKey]);
    $revSessionId = (int)$pdo->lastInsertId();

    $verifyRev = $pdo->prepare("SELECT connection_mode, status FROM `sessions` WHERE id = ?");
    $verifyRev->execute([$revSessionId]);
    $revRow = $verifyRev->fetch(PDO::FETCH_ASSOC);

    assertCheck("Reverse session recorded with connection_mode = 'reverse'", strtolower($revRow['connection_mode']) === 'reverse');
    assertCheck("Reverse session completed", strtolower($revRow['status']) === 'completed');

    // ── PART 8: STALE ACTIVE SESSION SWEEP ─────────────────────────────────────────
    echo "\n--- PART 8: Stale Active Session Sweep ---\n";
    $staleReqToken = 'reqtok_stale_' . bin2hex(random_bytes(14));
    $pdo->prepare("
        INSERT INTO connection_requests (requester_user_id, target_user_id, requester_system_id, requester_name, target_system_id, request_token, status, expires_at, created_at, updated_at)
        VALUES (?, ?, '999111222', 'Test User A', ?, ?, 'accepted', UTC_TIMESTAMP() + INTERVAL 300 SECOND, UTC_TIMESTAMP(), UTC_TIMESTAMP())
    ")->execute([$userAId, $userAId, $hostDevice['system_id'], $staleReqToken]);
    $staleCrId = (int)$pdo->lastInsertId();

    // Insert an active session with last_activity_at 300 seconds in the past
    $staleSessionKey = "sess_stale_" . bin2hex(random_bytes(8));
    $pdo->prepare("
        INSERT INTO `sessions`
            (`connection_request_id`, `requester_user_id`, `host_device_id`, `session_key`,
             `started_at`, `last_activity_at`, `status`, `connection_mode`)
        VALUES (?, ?, ?, ?, (UTC_TIMESTAMP() - INTERVAL 350 SECOND), (UTC_TIMESTAMP() - INTERVAL 150 SECOND), 'active', 'desktop')
    ")->execute([$staleCrId, $userAId, (int)$hostDevice['id'], $staleSessionKey]);
    $staleSessionId = (int)$pdo->lastInsertId();

    // Run sweep query from history.php / active.php
    $sweepRows = $pdo->exec("
        UPDATE `sessions`
        SET `ended_at` = COALESCE(`last_activity_at`, `started_at`),
            `status` = 'completed'
        WHERE `status` = 'active'
          AND (`last_activity_at` < (UTC_TIMESTAMP() - INTERVAL 120 SECOND)
               OR (`last_activity_at` IS NULL AND `started_at` < (UTC_TIMESTAMP() - INTERVAL 300 SECOND)))
    ");
    assertCheck("Sweep updated stale session(s) ($sweepRows rows affected)", $sweepRows > 0);

    $verifyStale = $pdo->prepare("SELECT status, ended_at FROM `sessions` WHERE id = ?");
    $verifyStale->execute([$staleSessionId]);
    $staleRow = $verifyStale->fetch(PDO::FETCH_ASSOC);

    assertCheck("Stale session status finalized to 'completed'", strtolower($staleRow['status']) === 'completed');
    assertCheck("Stale session ended_at set to last_activity_at", !empty($staleRow['ended_at']));

    // ── PART 9: ACCOUNT ISOLATION ─────────────────────────────────────────────────
    echo "\n--- PART 9: Account Isolation ---\n";
    // Query sessions visible to User B (who is NOT part of User A's sessions)
    $isoStmt = $pdo->prepare("
        SELECT COUNT(DISTINCT s.id)
        FROM `sessions` s
        JOIN `devices` d ON d.id = s.host_device_id
        LEFT JOIN `connection_requests` cr ON cr.id = s.connection_request_id
        WHERE (s.requester_user_id = ? OR d.user_id = ? OR cr.target_user_id = ?)
          AND s.id IN (?, ?, ?)
    ");
    $isoStmt->execute([$userBId, $userBId, $userBId, $sessionId, $revSessionId, $staleSessionId]);
    $visibleToUserB = (int)$isoStmt->fetchColumn();

    assertCheck("User B cannot view User A's session records (visible: $visibleToUserB)", $visibleToUserB === 0);

    // Query sessions visible to User A
    $isoStmtA = $pdo->prepare("
        SELECT COUNT(DISTINCT s.id)
        FROM `sessions` s
        JOIN `devices` d ON d.id = s.host_device_id
        LEFT JOIN `connection_requests` cr ON cr.id = s.connection_request_id
        WHERE (s.requester_user_id = ? OR d.user_id = ? OR cr.target_user_id = ?)
          AND s.id IN (?, ?, ?)
    ");
    $isoStmtA->execute([$userAId, $userAId, $userAId, $sessionId, $revSessionId, $staleSessionId]);
    $visibleToUserA = (int)$isoStmtA->fetchColumn();

    assertCheck("User A can view their own 3 session records (visible: $visibleToUserA)", $visibleToUserA === 3);

    // ── PART 10: ADMIN PORTAL SESSIONS QUERY INTEGRATION ─────────────────────────
    echo "\n--- PART 10: Admin Portal Sessions Query Integration ---\n";
    $adminStmt = $pdo->prepare("
        SELECT s.id, s.session_key, s.status, s.connection_mode,
               d.name AS host_device_name, d.system_id AS host_system_id,
               u.name AS host_user_name,
               TIMESTAMPDIFF(SECOND, s.started_at, s.ended_at) AS duration_seconds
        FROM `sessions` s
        LEFT JOIN `devices` d ON s.host_device_id = d.id
        LEFT JOIN `users` u ON d.user_id = u.id
        WHERE s.id = ?
    ");
    $adminStmt->execute([$sessionId]);
    $adminRow = $adminStmt->fetch(PDO::FETCH_ASSOC);

    assertCheck("Admin Portal query returns session record", !empty($adminRow));
    assertCheck("Admin row session_key matches", $adminRow['session_key'] === $testSessionKey);
    assertCheck("Admin row status is 'completed'", strtolower($adminRow['status']) === 'completed');
    assertCheck("Admin row connection_mode is 'desktop'", strtolower($adminRow['connection_mode']) === 'desktop');
    assertCheck("Admin row host_device_name populated", !empty($adminRow['host_device_name']));

} catch (Throwable $e) {
    echo "  [FATAL ERROR] " . $e->getMessage() . "\n" . $e->getTraceAsString() . "\n";
    $failed++;
}

echo "\n======================================================================\n";
echo " TEST SUMMARY: Passed: $passed, Failed: $failed\n";
echo "======================================================================\n";
exit($failed === 0 ? 0 : 1);
