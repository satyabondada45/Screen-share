<?php
// DeskStream - Modern Remote Desktop Dashboard
// Fully Dynamic PHP implementation with live database polling and interactive signaling modals

if (session_status() === PHP_SESSION_NONE) {
    session_start();
}

// Ensure user is authenticated before accessing dashboard
if (empty($_SESSION['user_id'])) {
    header("Location: login.php");
    exit();
}

$dbPath = __DIR__ . '/../backend/config/database.php';
if (!file_exists($dbPath)) {
    $dbPath = __DIR__ . '/../../backend/config/database.php';
}

if (!isset($pdo) || $pdo === null) {
    if (file_exists($dbPath)) {
        require $dbPath;
    }
}

// Authenticated user session
$currentUserId = $_SESSION['user_id'] ?? null;
$currentUsername = $_SESSION['username'] ?? 'User';

// 1. Fetch Local Host Device ("This Device") for the authenticated user
$hostDevice = null;
$recentDevices = [];

if ($pdo) {
    try {
        // Mark stale devices offline (>30s)
        $pdo->exec("
            UPDATE devices 
            SET is_online = 0 
            WHERE (last_seen_at IS NULL OR last_seen_at < (NOW() - INTERVAL 30 SECOND)) AND is_online = 1
        ");

        if ($currentUserId) {
            // First check if user has a registered device with valid physical machine_identifier
            $stmt = $pdo->prepare("
                SELECT *, 
                       (last_seen_at >= (NOW() - INTERVAL 30 SECOND) AND is_online = 1) AS is_live_online 
                FROM devices 
                WHERE user_id = ? AND machine_identifier IS NOT NULL
                ORDER BY is_live_online DESC, last_seen_at DESC, id ASC 
                LIMIT 1
            ");
            $stmt->execute([$currentUserId]);
            $hostDevice = $stmt->fetch(PDO::FETCH_ASSOC);
        }

        // If no user-specific device found, look for active computer on local machine or most recent active device
        if (!$hostDevice) {
            $clientIp = $_SERVER['HTTP_X_FORWARDED_FOR'] ?? $_SERVER['REMOTE_ADDR'] ?? '127.0.0.1';
            $stmt = $pdo->prepare("
                SELECT *, 
                       (last_seen_at >= (NOW() - INTERVAL 30 SECOND) AND is_online = 1) AS is_live_online 
                FROM devices 
                WHERE machine_identifier IS NOT NULL
                  AND (ip_address = ? OR ip_address = '127.0.0.1' OR ip_address = '::1')
                ORDER BY is_live_online DESC, last_seen_at DESC, id DESC 
                LIMIT 1
            ");
            $stmt->execute([$clientIp]);
            $hostDevice = $stmt->fetch(PDO::FETCH_ASSOC);

            // Associate user to device if found without changing device's system_id
            if ($hostDevice && $currentUserId && empty($hostDevice['user_id'])) {
                $pdo->prepare("UPDATE devices SET user_id = ? WHERE id = ?")
                    ->execute([$currentUserId, $hostDevice['id']]);
                $hostDevice['user_id'] = $currentUserId;
            }
        }

        // Fetch other workstations (excluding this computer) for Recent Sessions grid
        if ($hostDevice && !empty($hostDevice['id'])) {
            $gridStmt = $pdo->prepare("
                SELECT id, user_id, COALESCE(system_id, device_uid) AS system_id, device_uid, name, os_type, ip_address, 
                       (last_seen_at >= (NOW() - INTERVAL 30 SECOND) AND is_online = 1) AS is_online, 
                       last_seen_at, created_at 
                FROM devices 
                WHERE id != ?
                ORDER BY is_online DESC, last_seen_at DESC 
                LIMIT 12
            ");
            $gridStmt->execute([$hostDevice['id']]);
        } else {
            $gridStmt = $pdo->query("
                SELECT id, user_id, COALESCE(system_id, device_uid) AS system_id, device_uid, name, os_type, ip_address, 
                       (last_seen_at >= (NOW() - INTERVAL 30 SECOND) AND is_online = 1) AS is_online, 
                       last_seen_at, created_at 
                FROM devices 
                ORDER BY is_online DESC, last_seen_at DESC 
                LIMIT 12
            ");
        }
        $recentDevices = $gridStmt->fetchAll(PDO::FETCH_ASSOC);

    } catch (\PDOException $e) {
        error_log("Database Error in dashboard.php: " . $e->getMessage());
    }
}

// Device ID strictly from physical device registration
$hostUid = $hostDevice['system_id'] ?? $hostDevice['device_uid'] ?? 'Connecting...';
$hostAlias = $hostDevice['name'] ?? ($currentUsername . '-PC');
$hostIp = $hostDevice['ip_address'] ?? '127.0.0.1';
$isHostOnline = !empty($hostDevice['is_online']);

function formatDeviceId($id)
{
    $clean = preg_replace('/[^0-9]/', '', (string) $id);
    if (strlen($clean) === 9) {
        return substr($clean, 0, 3) . ' ' . substr($clean, 3, 3) . ' ' . substr($clean, 6, 3);
    }
    if (strlen($clean) === 6) {
        return substr($clean, 0, 3) . ' ' . substr($clean, 3, 3);
    }
    return chunk_split($clean, 3, ' ');
}

function getRelativeTime($timestamp)
{
    if (!$timestamp)
        return 'Recently active';
    $time = strtotime($timestamp);
    if (!$time)
        return 'Recently active';

    $diff = time() - $time;
    if ($diff < 60)
        return 'Just now';
    if ($diff < 3600)
        return floor($diff / 60) . ' mins ago';

    $todayStart = strtotime('today midnight');
    $yesterdayStart = strtotime('yesterday midnight');

    if ($time >= $todayStart) {
        return 'Today, ' . date('h:i A', $time);
    } elseif ($time >= $yesterdayStart) {
        return 'Yesterday, ' . date('h:i A', $time);
    } else {
        return date('d M Y', $time);
    }
}
?>
<!DOCTYPE html>
<html lang="en">

<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>DeskStream - Remote Desktop Control Dashboard</title>
    <link rel="preconnect" href="https://fonts.googleapis.com">
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
    <link
        href="https://fonts.googleapis.com/css2?family=Plus+Jakarta+Sans:wght@400;500;600;700;800&family=JetBrains+Mono:wght@500;700&display=swap"
        rel="stylesheet">
    <style>
        :root {
            --bg-body: #f1f5f9;
            --bg-card: #ffffff;
            --border-color: #e2e8f0;
            --border-focus: #cbd5e1;
            --primary: #ef4444;
            --primary-hover: #dc2626;
            --primary-light: #fef2f2;
            --primary-border: #fca5a5;
            --text-dark: #0f172a;
            --text-muted: #64748b;
            --text-subtle: #94a3b8;
            --success: #16a34a;
            --success-light: #f0fdf4;
            --success-border: #bbf7d0;
            --radius-lg: 16px;
            --radius-md: 12px;
            --radius-sm: 8px;
            --shadow-subtle: 0 1px 3px 0 rgba(0, 0, 0, 0.05), 0 1px 2px -1px rgba(0, 0, 0, 0.05);
            --shadow-card: 0 4px 6px -1px rgba(0, 0, 0, 0.03), 0 2px 4px -2px rgba(0, 0, 0, 0.03);
            --shadow-modal: 0 25px 50px -12px rgba(15, 23, 42, 0.25);
        }

        * {
            box-sizing: border-box;
            margin: 0;
            padding: 0;
            font-family: 'Plus Jakarta Sans', -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
        }

        body {
            background-color: var(--bg-body);
            color: var(--text-dark);
            height: 100vh;
            display: flex;
            flex-direction: column;
            overflow: hidden;
            -webkit-font-smoothing: antialiased;
        }

        /* Top Title Bar / Header */
        .topbar {
            height: 56px;
            background: var(--bg-card);
            border-bottom: 1px solid var(--border-color);
            display: flex;
            justify-content: space-between;
            align-items: center;
            padding: 0 20px;
            flex-shrink: 0;
            z-index: 10;
        }

        .brand-container {
            display: flex;
            align-items: center;
            gap: 10px;
            text-decoration: none;
            cursor: pointer;
        }

        .brand-logo {
            width: 28px;
            height: 28px;
            display: flex;
            align-items: center;
            justify-content: center;
        }

        .brand-name {
            font-size: 1.18rem;
            font-weight: 800;
            color: var(--text-dark);
            letter-spacing: -0.02em;
        }

        .topbar-center {
            display: flex;
            align-items: center;
            gap: 8px;
        }

        .topbar-tab-btn {
            display: inline-flex;
            align-items: center;
            gap: 8px;
            padding: 7px 16px;
            background: #ffffff;
            border: 1px solid var(--border-color);
            border-radius: var(--radius-sm);
            font-weight: 600;
            font-size: 0.85rem;
            color: var(--primary);
            box-shadow: var(--shadow-subtle);
            cursor: pointer;
            transition: all 0.2s ease;
        }

        .topbar-tab-btn:hover {
            border-color: var(--primary-border);
            background: var(--primary-light);
        }

        .topbar-plus-btn {
            background: #ffffff;
            border: 1px solid var(--border-color);
            border-radius: var(--radius-sm);
            width: 32px;
            height: 32px;
            display: flex;
            align-items: center;
            justify-content: center;
            cursor: pointer;
            color: var(--text-muted);
            font-size: 1.1rem;
            font-weight: 500;
            transition: all 0.2s ease;
        }

        .topbar-plus-btn:hover {
            background: #f8fafc;
            color: var(--text-dark);
            border-color: #cbd5e1;
        }

        .window-controls {
            display: flex;
            align-items: center;
            gap: 16px;
            color: var(--text-muted);
        }

        .win-btn {
            background: transparent;
            border: none;
            cursor: pointer;
            color: var(--text-muted);
            display: flex;
            align-items: center;
            justify-content: center;
            padding: 4px;
            border-radius: 4px;
            transition: color 0.15s ease;
        }

        .win-btn:hover {
            color: var(--text-dark);
        }

        .win-btn.close-btn:hover {
            color: var(--primary);
        }

        /* Workspace Main Layout */
        .workspace {
            display: flex;
            flex: 1;
            overflow: hidden;
        }

        /* Left Sidebar */
        .sidebar {
            width: 250px;
            background: var(--bg-card);
            border-right: 1px solid var(--border-color);
            display: flex;
            flex-direction: column;
            justify-content: space-between;
            padding: 16px 14px;
            flex-shrink: 0;
            overflow-y: auto;
        }

        .nav-menu {
            display: flex;
            flex-direction: column;
            gap: 6px;
        }

        .nav-item {
            display: flex;
            align-items: center;
            gap: 12px;
            padding: 10px 14px;
            border-radius: 10px;
            text-decoration: none;
            color: var(--text-muted);
            font-weight: 500;
            font-size: 0.9rem;
            transition: all 0.15s ease;
            cursor: pointer;
            border: 1px solid transparent;
            background: transparent;
            text-align: left;
            width: 100%;
        }

        .nav-item svg {
            flex-shrink: 0;
            stroke-width: 2;
        }

        .nav-item:hover {
            background: #f8fafc;
            color: var(--text-dark);
        }

        .nav-item.active {
            background: var(--primary-light);
            color: var(--primary);
            font-weight: 600;
            border-color: #fee2e2;
        }

        .nav-item.active svg {
            color: var(--primary);
            stroke: var(--primary);
        }

        .sidebar-divider {
            height: 1px;
            background: var(--border-color);
            margin: 8px 0;
        }

        /* Sidebar Bottom Mini Status Box */
        .sidebar-device-box {
            background: #ffffff;
            border: 1px solid var(--border-color);
            border-radius: var(--radius-md);
            padding: 14px;
            box-shadow: var(--shadow-subtle);
        }

        .readiness-tag {
            display: inline-flex;
            align-items: center;
            gap: 6px;
            font-size: 0.8rem;
            color: var(--success);
            font-weight: 600;
            margin-bottom: 8px;
        }

        .sidebar-id-row {
            display: flex;
            align-items: center;
            justify-content: space-between;
            margin: 2px 0 6px 0;
        }

        .sidebar-id-val {
            font-family: 'JetBrains Mono', monospace;
            font-size: 1.15rem;
            font-weight: 800;
            color: var(--text-dark);
            letter-spacing: 0.5px;
        }

        .sidebar-copy-btn {
            background: transparent;
            border: none;
            cursor: pointer;
            color: var(--text-muted);
            padding: 4px;
            display: flex;
            align-items: center;
            border-radius: 4px;
            transition: all 0.15s ease;
        }

        .sidebar-copy-btn:hover {
            color: var(--primary);
            background: var(--primary-light);
        }

        .sidebar-alias-val {
            font-size: 0.82rem;
            color: var(--text-muted);
            display: flex;
            align-items: center;
            gap: 6px;
        }

        .sidebar-alias-val strong {
            color: var(--text-dark);
            font-weight: 600;
        }

        .mini-edit-btn {
            background: transparent;
            border: none;
            color: var(--text-subtle);
            cursor: pointer;
            font-size: 0.8rem;
            transition: color 0.15s;
        }

        .mini-edit-btn:hover {
            color: var(--primary);
        }

        /* Main Content Container */
        .content {
            flex: 1;
            padding: 22px 24px;
            overflow-y: auto;
            display: flex;
            flex-direction: column;
            gap: 20px;
        }

        /* Top Grid - Two Panels */
        .top-panels-grid {
            display: grid;
            grid-template-columns: 1fr 1fr;
            gap: 20px;
        }

        .panel-card {
            background: var(--bg-card);
            border: 1px solid var(--border-color);
            border-radius: var(--radius-lg);
            padding: 22px 24px;
            box-shadow: var(--shadow-card);
            position: relative;
            display: flex;
            flex-direction: column;
            justify-content: space-between;
        }

        .panel-header {
            display: flex;
            align-items: flex-start;
            justify-content: space-between;
            margin-bottom: 4px;
        }

        .panel-title {
            font-size: 1.18rem;
            font-weight: 700;
            color: var(--text-dark);
        }

        .panel-subtitle {
            color: var(--text-muted);
            font-size: 0.85rem;
            margin-bottom: 14px;
            font-weight: 400;
        }

        .panel-menu-btn {
            background: transparent;
            border: none;
            color: var(--text-muted);
            cursor: pointer;
            padding: 4px;
            font-size: 1.2rem;
            line-height: 1;
            border-radius: 4px;
            transition: color 0.15s;
        }

        .panel-menu-btn:hover {
            color: var(--text-dark);
        }

        .field-label {
            font-size: 0.78rem;
            color: var(--text-muted);
            font-weight: 600;
            text-transform: capitalize;
            margin-bottom: 4px;
        }

        /* ID Display Row */
        .id-row-wrapper {
            display: flex;
            align-items: center;
            gap: 16px;
            margin-bottom: 14px;
        }

        .large-id-display {
            font-family: 'JetBrains Mono', monospace;
            font-size: 2.1rem;
            font-weight: 800;
            color: var(--primary);
            letter-spacing: 1px;
            line-height: 1.1;
        }

        .copy-box-btn {
            background: #ffffff;
            border: 1px solid var(--border-color);
            border-radius: 10px;
            width: 42px;
            height: 42px;
            display: inline-flex;
            align-items: center;
            justify-content: center;
            cursor: pointer;
            color: var(--text-muted);
            transition: all 0.2s ease;
            box-shadow: var(--shadow-subtle);
        }

        .copy-box-btn:hover {
            background: #f8fafc;
            color: var(--primary);
            border-color: var(--primary-border);
            transform: translateY(-1px);
        }

        .alias-display-row {
            display: flex;
            align-items: center;
            gap: 8px;
            font-size: 1.05rem;
            font-weight: 700;
            color: var(--text-dark);
            margin-bottom: 16px;
        }

        .alias-input-inline {
            font-size: 1rem;
            font-weight: 600;
            padding: 4px 8px;
            border: 1px solid var(--primary);
            border-radius: 6px;
            outline: none;
            display: none;
        }

        .edit-alias-btn {
            background: transparent;
            border: none;
            color: var(--text-muted);
            cursor: pointer;
            display: flex;
            align-items: center;
            padding: 2px 4px;
            transition: color 0.15s;
        }

        .edit-alias-btn:hover {
            color: var(--primary);
        }

        .connection-ready-badge {
            display: inline-flex;
            align-items: center;
            gap: 6px;
            font-size: 0.84rem;
            color: var(--success);
            font-weight: 600;
            margin-top: auto;
        }

        /* Connect to Remote Panel */
        .connect-input-row {
            display: flex;
            gap: 10px;
            margin-bottom: 14px;
            position: relative;
        }

        .input-with-dropdown {
            position: relative;
            flex: 1;
            display: flex;
            align-items: center;
        }

        .remote-id-input {
            width: 100%;
            height: 46px;
            padding: 0 38px 0 14px;
            background: #ffffff;
            border: 1px solid var(--border-color);
            border-radius: 10px;
            font-size: 0.95rem;
            font-weight: 500;
            color: var(--text-dark);
            outline: none;
            transition: all 0.2s ease;
        }

        .remote-id-input:focus {
            border-color: var(--primary);
            box-shadow: 0 0 0 3px rgba(239, 68, 68, 0.12);
        }

        .dropdown-chevron-btn {
            position: absolute;
            right: 12px;
            background: transparent;
            border: none;
            color: var(--text-muted);
            cursor: pointer;
            display: flex;
            align-items: center;
            justify-content: center;
        }

        .recent-dropdown-list {
            position: absolute;
            top: 50px;
            left: 0;
            right: 0;
            background: #ffffff;
            border: 1px solid var(--border-color);
            border-radius: 10px;
            box-shadow: var(--shadow-modal);
            z-index: 40;
            display: none;
            max-height: 180px;
            overflow-y: auto;
        }

        .recent-dropdown-list.open {
            display: block;
        }

        .dropdown-item {
            padding: 10px 14px;
            font-size: 0.85rem;
            color: var(--text-dark);
            display: flex;
            justify-content: space-between;
            align-items: center;
            cursor: pointer;
            border-bottom: 1px solid #f1f5f9;
        }

        .dropdown-item:hover {
            background: #f8fafc;
            color: var(--primary);
        }

        .btn-connect-primary {
            background: var(--primary);
            color: #ffffff;
            border: none;
            border-radius: 10px;
            padding: 0 22px;
            height: 46px;
            font-weight: 700;
            font-size: 0.92rem;
            cursor: pointer;
            display: inline-flex;
            align-items: center;
            gap: 8px;
            transition: all 0.2s ease;
            box-shadow: 0 4px 10px rgba(239, 68, 68, 0.25);
            flex-shrink: 0;
        }

        .btn-connect-primary:hover {
            background: var(--primary-hover);
            transform: translateY(-1px);
            box-shadow: 0 6px 14px rgba(239, 68, 68, 0.35);
        }

        /* Mode Selection Cards */
        .mode-selection-grid {
            display: grid;
            grid-template-columns: 1fr 1fr;
            gap: 12px;
        }

        .mode-option-card {
            border: 1px solid var(--border-color);
            background: #ffffff;
            border-radius: 12px;
            padding: 12px 14px;
            display: flex;
            align-items: center;
            gap: 12px;
            cursor: pointer;
            transition: all 0.2s ease;
            user-select: none;
        }

        .mode-option-card.active {
            background: var(--primary-light);
            border-color: var(--primary-border);
        }

        .mode-icon-box {
            color: var(--text-muted);
            display: flex;
            align-items: center;
            justify-content: center;
        }

        .mode-option-card.active .mode-icon-box {
            color: var(--primary);
        }

        .mode-text-box h5 {
            font-size: 0.9rem;
            font-weight: 700;
            color: var(--text-dark);
            margin-bottom: 2px;
        }

        .mode-text-box p {
            font-size: 0.75rem;
            color: var(--text-muted);
            font-weight: 400;
        }

        /* Recent Sessions Section */
        .section-header-row {
            display: flex;
            justify-content: space-between;
            align-items: center;
            margin-top: 4px;
        }

        .section-heading {
            font-size: 1.15rem;
            font-weight: 700;
            color: var(--text-dark);
        }

        .view-all-link {
            color: #2563eb;
            font-size: 0.85rem;
            font-weight: 600;
            text-decoration: none;
            transition: color 0.15s;
            cursor: pointer;
        }

        .view-all-link:hover {
            color: #1d4ed8;
            text-decoration: underline;
        }

        /* Recent Workstation Cards Grid */
        .workstations-grid {
            display: grid;
            grid-template-columns: repeat(3, 1fr);
            gap: 16px;
        }

        .workstation-card {
            background: var(--bg-card);
            border: 1px solid var(--border-color);
            border-radius: 14px;
            padding: 16px;
            box-shadow: var(--shadow-subtle);
            display: flex;
            align-items: center;
            gap: 14px;
            cursor: pointer;
            transition: all 0.2s ease;
            position: relative;
        }

        .workstation-card:hover {
            border-color: #cbd5e1;
            box-shadow: 0 6px 16px rgba(0, 0, 0, 0.05);
            transform: translateY(-2px);
        }

        .device-avatar-circle {
            width: 44px;
            height: 44px;
            border-radius: 50%;
            display: flex;
            align-items: center;
            justify-content: center;
            color: #ffffff;
            flex-shrink: 0;
        }

        .card-main-info {
            flex: 1;
            overflow: hidden;
        }

        .card-top-row {
            display: flex;
            align-items: center;
            justify-content: space-between;
            margin-bottom: 3px;
        }

        .device-name-title {
            font-size: 0.95rem;
            font-weight: 700;
            color: var(--text-dark);
            white-space: nowrap;
            overflow: hidden;
            text-overflow: ellipsis;
            display: flex;
            align-items: center;
            gap: 6px;
        }

        .online-dot {
            width: 7px;
            height: 7px;
            border-radius: 50%;
            display: inline-block;
            background: var(--text-subtle);
        }

        .online-dot.active {
            background: var(--success);
            box-shadow: 0 0 6px rgba(22, 163, 74, 0.6);
        }

        .card-top-actions {
            display: flex;
            align-items: center;
            gap: 6px;
            color: var(--text-subtle);
        }

        .star-fav-btn,
        .more-opt-btn {
            background: transparent;
            border: none;
            cursor: pointer;
            color: var(--text-subtle);
            padding: 2px;
            display: flex;
            align-items: center;
            transition: color 0.15s;
        }

        .star-fav-btn:hover {
            color: #f59e0b;
        }

        .more-opt-btn:hover {
            color: var(--text-dark);
        }

        .device-ip-text {
            font-family: 'JetBrains Mono', monospace;
            font-size: 0.8rem;
            color: var(--text-muted);
            margin-bottom: 5px;
        }

        .last-active-row {
            display: flex;
            align-items: center;
            gap: 5px;
            font-size: 0.75rem;
            color: var(--text-muted);
        }

        .empty-grid-box {
            grid-column: 1 / -1;
            background: #ffffff;
            border: 1px dashed var(--border-color);
            border-radius: 14px;
            padding: 40px 20px;
            text-align: center;
            color: var(--text-muted);
            font-size: 0.9rem;
        }

        /* Footer Status Bar */
        .footer-status-bar {
            height: 34px;
            background: #f8fafc;
            border-top: 1px solid var(--border-color);
            display: flex;
            justify-content: space-between;
            align-items: center;
            padding: 0 24px;
            font-size: 0.78rem;
            color: var(--text-muted);
            flex-shrink: 0;
        }

        .footer-left {
            display: flex;
            align-items: center;
            gap: 6px;
        }

        .footer-right {
            display: flex;
            align-items: center;
            gap: 6px;
            color: var(--text-dark);
            font-weight: 500;
        }

        .signal-icon-green {
            color: var(--success);
            display: inline-flex;
            align-items: center;
        }

        /* =========================================================
           INCOMING CONNECTION REQUEST MODAL OVERLAY (Reference Image 2)
           ========================================================= */
        .modal-backdrop-overlay {
            position: fixed;
            inset: 0;
            background: rgba(15, 23, 42, 0.65);
            backdrop-filter: blur(5px);
            -webkit-backdrop-filter: blur(5px);
            z-index: 1000;
            display: none;
            align-items: center;
            justify-content: center;
            padding: 20px;
            opacity: 0;
            transition: opacity 0.25s cubic-bezier(0.16, 1, 0.3, 1);
        }

        .modal-backdrop-overlay.open {
            display: flex;
            opacity: 1;
        }

        .incoming-modal-card {
            background: #ffffff;
            border-radius: 20px;
            width: 100%;
            max-width: 520px;
            box-shadow: var(--shadow-modal);
            padding: 32px 28px;
            display: flex;
            flex-direction: column;
            align-items: center;
            position: relative;
            transform: scale(0.94) translateY(10px);
            transition: transform 0.25s cubic-bezier(0.16, 1, 0.3, 1);
        }

        .modal-backdrop-overlay.open .incoming-modal-card {
            transform: scale(1) translateY(0);
        }

        .modal-red-icon-badge {
            width: 72px;
            height: 72px;
            border-radius: 50%;
            background: #fee2e2;
            display: flex;
            align-items: center;
            justify-content: center;
            color: var(--primary);
            margin-bottom: 16px;
        }

        .modal-title {
            font-size: 1.35rem;
            font-weight: 800;
            color: var(--text-dark);
            text-align: center;
            margin-bottom: 4px;
        }

        .modal-subtitle {
            font-size: 0.88rem;
            color: var(--text-muted);
            text-align: center;
            margin-bottom: 20px;
        }

        /* Modal Info Table Card */
        .modal-info-card {
            width: 100%;
            background: #ffffff;
            border: 1px solid var(--border-color);
            border-radius: var(--radius-md);
            overflow: hidden;
            margin-bottom: 18px;
        }

        .info-row {
            display: flex;
            align-items: center;
            justify-content: space-between;
            padding: 12px 16px;
            border-bottom: 1px solid #f1f5f9;
        }

        .info-row:last-child {
            border-bottom: none;
        }

        .info-label-group {
            display: flex;
            align-items: center;
            gap: 10px;
            color: var(--text-muted);
            font-size: 0.85rem;
            font-weight: 500;
        }

        .info-value-group {
            display: flex;
            align-items: center;
            gap: 6px;
            font-size: 0.9rem;
            font-weight: 700;
            color: var(--text-dark);
        }

        .verified-shield-icon {
            color: var(--success);
            display: inline-flex;
            align-items: center;
        }

        .red-id-highlight {
            font-family: 'JetBrains Mono', monospace;
            color: var(--primary);
            font-size: 1.15rem;
            font-weight: 800;
        }

        /* Permissions Section */
        .permissions-section {
            width: 100%;
            margin-bottom: 20px;
        }

        .permissions-label {
            font-size: 0.84rem;
            font-weight: 700;
            color: var(--text-dark);
            margin-bottom: 10px;
        }

        .permissions-grid {
            display: grid;
            grid-template-columns: repeat(3, 1fr);
            gap: 10px;
        }

        .permission-card {
            border: 1px solid var(--border-color);
            border-radius: 10px;
            padding: 10px 10px;
            background: #ffffff;
            display: flex;
            flex-direction: column;
            justify-content: space-between;
            min-height: 80px;
        }

        .perm-top-row {
            display: flex;
            justify-content: space-between;
            align-items: flex-start;
        }

        .perm-icon {
            color: var(--text-dark);
            display: flex;
            align-items: center;
        }

        /* iOS style toggle switch */
        .switch {
            position: relative;
            display: inline-block;
            width: 32px;
            height: 18px;
        }

        .switch input {
            opacity: 0;
            width: 0;
            height: 0;
        }

        .slider {
            position: absolute;
            cursor: pointer;
            inset: 0;
            background-color: #cbd5e1;
            transition: .2s;
            border-radius: 20px;
        }

        .slider:before {
            position: absolute;
            content: "";
            height: 14px;
            width: 14px;
            left: 2px;
            bottom: 2px;
            background-color: white;
            transition: .2s;
            border-radius: 50%;
            box-shadow: 0 1px 3px rgba(0, 0, 0, 0.2);
        }

        input:checked+.slider {
            background-color: var(--success);
        }

        input:checked+.slider:before {
            transform: translateX(14px);
        }

        .perm-title {
            font-size: 0.8rem;
            font-weight: 700;
            color: var(--text-dark);
            margin-top: 6px;
        }

        .perm-desc {
            font-size: 0.68rem;
            color: var(--text-muted);
            margin-top: 1px;
        }

        /* Modal Action Buttons */
        .modal-action-row {
            width: 100%;
            display: grid;
            grid-template-columns: 1fr 1fr;
            gap: 12px;
            margin-bottom: 16px;
        }

        .btn-modal-deny {
            background: #fee2e2;
            border: 1px solid #fca5a5;
            color: var(--primary);
            border-radius: var(--radius-md);
            padding: 12px 18px;
            font-size: 0.95rem;
            font-weight: 700;
            display: inline-flex;
            align-items: center;
            justify-content: center;
            gap: 8px;
            cursor: pointer;
            transition: all 0.2s ease;
        }

        .btn-modal-deny:hover {
            background: #fecaca;
        }

        .btn-modal-accept {
            background: #15803d;
            border: 1px solid #166534;
            color: #ffffff;
            border-radius: var(--radius-md);
            padding: 12px 18px;
            font-size: 0.95rem;
            font-weight: 700;
            display: inline-flex;
            align-items: center;
            justify-content: center;
            gap: 8px;
            cursor: pointer;
            transition: all 0.2s ease;
            box-shadow: 0 4px 12px rgba(22, 163, 74, 0.3);
        }

        .btn-modal-accept:hover {
            background: #166534;
            box-shadow: 0 6px 16px rgba(22, 163, 74, 0.4);
            transform: translateY(-1px);
        }

        /* Remember Choice Checkbox */
        .remember-choice-wrapper {
            display: flex;
            flex-direction: column;
            align-items: center;
            gap: 2px;
            font-size: 0.8rem;
            color: var(--text-dark);
        }

        .remember-checkbox-label {
            display: flex;
            align-items: center;
            gap: 8px;
            cursor: pointer;
            font-weight: 600;
        }

        .remember-checkbox-label input {
            accent-color: var(--primary);
            cursor: pointer;
        }

        .remember-subtext {
            font-size: 0.72rem;
            color: var(--text-muted);
        }

        /* Toast notification */
        .toast-popup {
            position: fixed;
            bottom: 24px;
            right: 24px;
            background: var(--text-dark);
            color: #ffffff;
            padding: 10px 18px;
            border-radius: 8px;
            font-size: 0.85rem;
            font-weight: 600;
            box-shadow: var(--shadow-modal);
            z-index: 2000;
            opacity: 0;
            transform: translateY(20px);
            transition: all 0.25s ease;
            pointer-events: none;
            display: flex;
            align-items: center;
            gap: 8px;
        }

        .toast-popup.show {
            opacity: 1;
            transform: translateY(0);
        }

        /* Responsive Breakpoints */
        @media (max-width: 1080px) {
            .workstations-grid {
                grid-template-columns: repeat(2, 1fr);
            }
        }

        @media (max-width: 820px) {
            .top-panels-grid {
                grid-template-columns: 1fr;
            }

            .sidebar {
                width: 220px;
            }

            .workstations-grid {
                grid-template-columns: 1fr;
            }
        }

        @media (max-width: 640px) {
            .workspace {
                flex-direction: column;
            }

            .sidebar {
                width: 100%;
                height: auto;
                border-right: none;
                border-bottom: 1px solid var(--border-color);
            }

            .topbar-center {
                display: none;
            }

            .modal-action-row {
                grid-template-columns: 1fr;
            }

            .permissions-grid {
                grid-template-columns: 1fr;
            }
        }
    </style>
</head>

<body>

    <!-- 1. Top Window Titlebar -->
    <header class="topbar">
        <div class="brand-container" onclick="location.reload()">
            <div class="brand-logo">
                <svg width="26" height="26" viewBox="0 0 32 32" fill="none">
                    <path d="M6 16L16 6L20 10L12 18L6 16Z" fill="#ef4444" />
                    <path d="M12 22L22 12L26 16L18 24L12 22Z" fill="#dc2626" />
                    <path d="M16 28L26 18L30 22L20 32L16 28Z" fill="#b91c1c" opacity="0.8" />
                </svg>
            </div>
            <div class="brand-name">DeskStream</div>
        </div>

        <div class="topbar-center">
            <button class="topbar-tab-btn" type="button">
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <rect x="2" y="3" width="20" height="14" rx="2" />
                    <line x1="8" y1="21" x2="16" y2="21" />
                    <line x1="12" y1="17" x2="12" y2="21" />
                </svg>
                New Session
            </button>
            <button class="topbar-plus-btn" title="Open new tab"
                onclick="showToast('Opened new session tab')">+</button>
        </div>

        <div style="display: flex; align-items: center; gap: 14px;">
            <?php if (!empty($currentUserId)): ?>
                <div
                    style="display: inline-flex; align-items: center; gap: 8px; font-size: 0.84rem; font-weight: 600; color: var(--text-dark); background: #f8fafc; border: 1px solid var(--border-color); padding: 5px 12px; border-radius: 20px;">
                    <span>👤 <?= htmlspecialchars($currentUsername) ?></span>
                    <a href="logout.php"
                        style="color: var(--primary); text-decoration: none; font-size: 0.78rem; font-weight: 700; margin-left: 4px;"
                        title="Sign out">Logout</a>
                </div>
            <?php else: ?>
                <a href="login.php"
                    style="font-size: 0.84rem; font-weight: 600; color: #2563eb; text-decoration: none;">Sign In</a>
            <?php endif; ?>

            <div class="window-controls">
                <button class="win-btn" title="Menu">
                    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                        <line x1="3" y1="12" x2="21" y2="12" />
                        <line x1="3" y1="6" x2="21" y2="6" />
                        <line x1="3" y1="18" x2="21" y2="18" />
                    </svg>
                </button>
                <button class="win-btn" title="Minimize">
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                        <line x1="5" y1="12" x2="19" y2="12" />
                    </svg>
                </button>
                <button class="win-btn" title="Maximize">
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                        <rect x="4" y="4" width="16" height="16" rx="2" />
                    </svg>
                </button>
                <button class="win-btn close-btn" title="Close">
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                        <line x1="18" y1="6" x2="6" y2="18" />
                        <line x1="6" y1="6" x2="18" y2="18" />
                    </svg>
                </button>
            </div>
        </div>
    </header>

    <!-- 2. Main Workspace Layout -->
    <div class="workspace">
        <!-- Sidebar Navigation -->
        <aside class="sidebar">
            <div class="nav-menu">
                <button class="nav-item active" onclick="setSidebarActive(this)">
                    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor">
                        <rect x="2" y="3" width="20" height="14" rx="2" />
                        <line x1="8" y1="21" x2="16" y2="21" />
                        <line x1="12" y1="17" x2="12" y2="21" />
                    </svg>
                    New Session
                </button>
                <button class="nav-item" onclick="setSidebarActive(this); scrollToSection('recent-section');">
                    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor">
                        <circle cx="12" cy="12" r="10" />
                        <polyline points="12 6 12 12 16 14" />
                    </svg>
                    Recent Sessions
                </button>
                <button class="nav-item" onclick="setSidebarActive(this); showToast('Address Book synced');">
                    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor">
                        <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2" />
                        <circle cx="12" cy="7" r="4" />
                    </svg>
                    Address Book
                </button>

                <div class="sidebar-divider"></div>

                <button class="nav-item" onclick="setSidebarActive(this); showToast('Settings opened');">
                    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor">
                        <circle cx="12" cy="12" r="3" />
                        <path
                            d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" />
                    </svg>
                    Settings
                </button>
                <button class="nav-item" onclick="setSidebarActive(this); showToast('DeskStream v2.4.0 (Build 8891)');">
                    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor">
                        <circle cx="12" cy="12" r="10" />
                        <line x1="12" y1="16" x2="12" y2="12" />
                        <line x1="12" y1="8" x2="12.01" y2="8" />
                    </svg>
                    About
                </button>
            </div>

            <!-- Bottom Mini Status Box -->
            <div class="sidebar-device-box">
                <div class="readiness-tag">
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor"
                        stroke-width="2.5">
                        <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
                        <polyline points="9 12 11 14 15 10" />
                    </svg>
                    Ready for connections
                </div>
                <div class="field-label">Your ID</div>
                <div class="sidebar-id-row">
                    <div class="sidebar-id-val" id="sidebarIdDisplay"><?= htmlspecialchars(formatDeviceId($hostUid)) ?>
                    </div>
                    <button class="sidebar-copy-btn" onclick="copyHostId()" title="Copy ID">
                        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor"
                            stroke-width="2">
                            <rect x="9" y="9" width="13" height="13" rx="2" />
                            <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
                        </svg>
                    </button>
                </div>
                <div class="sidebar-alias-val">
                    Alias: <strong id="sidebarAliasDisplay"><?= htmlspecialchars($hostAlias) ?></strong>
                    <button class="mini-edit-btn" onclick="triggerAliasEdit()" title="Edit Alias">
                        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor"
                            stroke-width="2">
                            <path d="M17 3a2.828 2.828 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5L17 3z" />
                        </svg>
                    </button>
                </div>
            </div>
        </aside>

        <!-- Main Content Area -->
        <main class="content">
            <!-- Top Section - Two Primary Panels -->
            <div class="top-panels-grid">
                <!-- Panel 1: This Device -->
                <div class="panel-card">
                    <div>
                        <div class="panel-header">
                            <h2 class="panel-title">This Device</h2>
                            <button class="panel-menu-btn" title="Options"
                                onclick="showToast('Host options: Listening on port 8080')">⋮</button>
                        </div>
                        <p class="panel-subtitle">Your device can be accessed with this address.</p>

                        <div class="field-label">Your ID</div>
                        <div class="id-row-wrapper">
                            <div class="large-id-display" id="mainHostId">
                                <?= htmlspecialchars(formatDeviceId($hostUid)) ?>
                            </div>
                            <button class="copy-box-btn" onclick="copyHostId()" title="Copy to clipboard">
                                <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor"
                                    stroke-width="2">
                                    <rect x="9" y="9" width="13" height="13" rx="2" />
                                    <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
                                </svg>
                            </button>
                        </div>

                        <div class="field-label">Alias</div>
                        <div class="alias-display-row">
                            <span id="mainHostAlias"><?= htmlspecialchars($hostAlias) ?></span>
                            <input type="text" id="hostAliasInput" class="alias-input-inline"
                                value="<?= htmlspecialchars($hostAlias) ?>" onblur="saveAliasInline()"
                                onkeydown="handleAliasKey(event)">
                            <button class="edit-alias-btn" onclick="triggerAliasEdit()" title="Edit Alias">
                                <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor"
                                    stroke-width="2">
                                    <path d="M17 3a2.828 2.828 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5L17 3z" />
                                </svg>
                            </button>
                        </div>
                    </div>

                    <div class="connection-ready-badge" id="hostStatusBadge" style="cursor:pointer;" onclick="if(!isHostOnlineGlobal) startDesktopAgent();">
                        <?php if ($isHostOnline): ?>
                            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor"
                                stroke-width="2.5">
                                <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
                                <polyline points="9 12 11 14 15 10" />
                            </svg>
                            <span>Ready for connections</span>
                        <?php else: ?>
                            <span style="color:#64748b; display:inline-flex; align-items:center; gap:6px;">
                                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                                    <circle cx="12" cy="12" r="10"/>
                                    <line x1="12" y1="8" x2="12" y2="12"/>
                                    <line x1="12" y1="16" x2="12.01" y2="16"/>
                                </svg>
                                Agent Offline (Start Desktop Agent)
                            </span>
                        <?php endif; ?>
                    </div>
                </div>

                <!-- Panel 2: Connect to Remote Device -->
                <div class="panel-card">
                    <div>
                        <div class="panel-header">
                            <h2 class="panel-title">Connect to Remote Device</h2>
                        </div>
                        <p class="panel-subtitle">Enter the remote device ID to start a session.</p>

                        <div class="connect-input-row">
                            <div class="input-with-dropdown">
                                <input type="text" id="remoteIdInput" class="remote-id-input"
                                    placeholder="Enter Remote ID" autocomplete="off"
                                    onkeydown="if(event.key==='Enter') startConnection()">
                                <button class="dropdown-chevron-btn" type="button" onclick="toggleRecentDropdown()"
                                    title="Recent IDs">
                                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor"
                                        stroke-width="2">
                                        <polyline points="6 9 12 15 18 9" />
                                    </svg>
                                </button>
                                <div class="recent-dropdown-list" id="recentDropdown">
                                    <!-- Populated dynamically -->
                                </div>
                            </div>
                            <button class="btn-connect-primary" type="button" onclick="startConnection()">
                                Connect
                                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor"
                                    stroke-width="2.5">
                                    <line x1="5" y1="12" x2="19" y2="12" />
                                    <polyline points="12 5 19 12 12 19" />
                                </svg>
                            </button>
                        </div>
                    </div>
                </div>
            </div>

            <!-- Bottom Section - Recent Sessions Grid -->
            <div id="recent-section">
                <div class="section-header-row">
                    <h3 class="section-heading">Recent Sessions</h3>
                    <a class="view-all-link" href="#" onclick="fetchAllDevices(true); return false;">View All</a>
                </div>

                <div class="workstations-grid" id="workstationsContainer">
                    <?php if (!empty($recentDevices)): ?>
                        <?php foreach ($recentDevices as $idx => $dev):
                            $colors = ['#0284c7', '#ef4444', '#16a34a', '#f59e0b', '#8b5cf6', '#0d9488'];
                            $color = $colors[$idx % count($colors)];
                            $isOnline = !empty($dev['is_online']);
                            $devUidFormatted = formatDeviceId($dev['device_uid']);
                            $relativeTime = getRelativeTime($dev['last_seen_at'] ?? $dev['created_at']);
                            $osType = strtolower($dev['os_type'] ?? 'windows');
                            ?>
                            <div class="workstation-card"
                                onclick="selectDropdownId('<?= htmlspecialchars($dev['system_id'] ?? $dev['device_uid']) ?>')">
                                <div class="device-avatar-circle" style="background: <?= $color ?>;">
                                    <?php if (strpos($osType, 'mac') !== false || strpos($osType, 'apple') !== false): ?>
                                        <svg width="22" height="22" viewBox="0 0 24 24" fill="currentColor">
                                            <path
                                                d="M18.71 19.5c-.83 1.24-1.71 2.45-3.05 2.47-1.34.03-1.77-.79-3.29-.79-1.53 0-2 .77-3.27.82-1.31.05-2.3-1.32-3.14-2.53C4.25 17 2.94 12.45 4.7 9.39c.87-1.52 2.43-2.48 4.12-2.51 1.28-.02 2.5.87 3.29.87.78 0 2.26-1.07 3.81-.91.65.03 2.47.26 3.64 1.98-.09.06-2.17 1.28-2.15 3.81.03 3.02 2.65 4.03 2.68 4.04-.03.07-.42 1.44-1.38 2.83M15.97 6.37c.63-.77 1.06-1.84.94-2.91-.91.04-2.02.61-2.67 1.37-.58.67-1.09 1.76-.95 2.8.01 0 .03 0 .05 0 1.02 0 2-.49 2.63-1.26z" />
                                        </svg>
                                    <?php elseif (strpos($osType, 'linux') !== false): ?>
                                        <svg width="22" height="22" viewBox="0 0 24 24" fill="currentColor">
                                            <path d="M12 2a5 5 0 0 0-5 5v3a5 5 0 0 0 10 0V7a5 5 0 0 0-5-5z" />
                                        </svg>
                                    <?php else: ?>
                                        <!-- Windows / PC Icon -->
                                        <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor">
                                            <path
                                                d="M3 5.557L10.375 4.5v6.924H3V5.557zm0 12.886l7.375 1.057v-6.924H3v5.867zm8.344 1.218L21 21.2V11.424H11.344v8.237zM21 2.8L11.344 4.361v7.063H21V2.8z" />
                                        </svg>
                                    <?php endif; ?>
                                </div>
                                <div class="card-main-info">
                                    <div class="card-top-row">
                                        <div class="device-name-title">
                                            <?= htmlspecialchars($dev['name'] ?? 'Workstation') ?>
                                            <span class="online-dot <?= $isOnline ? 'active' : '' ?>"
                                                title="<?= $isOnline ? 'Online' : 'Offline' ?>"></span>
                                        </div>
                                        <div class="card-top-actions" onclick="event.stopPropagation()">
                                            <button class="star-fav-btn" title="Favorite"
                                                onclick="toggleFavorite(this)">☆</button>
                                            <button class="more-opt-btn" title="Options"
                                                onclick="showToast('Device UID: <?= htmlspecialchars($dev['device_uid']) ?>')">⋮</button>
                                        </div>
                                    </div>
                                    <div class="device-ip-text">
                                        <?= htmlspecialchars($dev['ip_address'] ?? '192.168.1.100') ?>
                                    </div>
                                    <div class="last-active-row">
                                        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor"
                                            stroke-width="2">
                                            <circle cx="12" cy="12" r="10" />
                                            <polyline points="12 6 12 12 16 14" />
                                        </svg>
                                        <?= htmlspecialchars($relativeTime) ?>
                                    </div>
                                </div>
                            </div>
                        <?php endforeach; ?>
                    <?php else: ?>
                        <div class="empty-grid-box">
                            No remote desktop agents connected yet. Launch your desktop agent to connect
                            automatically!
                        </div>
                    <?php endif; ?>
                </div>
            </div>
        </main>
    </div>

    <!-- 3. Footer Status Bar -->
    <footer class="footer-status-bar">
        <div class="footer-left">
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <rect x="3" y="11" width="18" height="11" rx="2" ry="2" />
                <path d="M7 11V7a5 5 0 0 1 10 0v4" />
            </svg>
            <span>Secure connection established.</span>
        </div>
        <div class="footer-right">
            <span class="signal-icon-green">
                <svg width="15" height="15" viewBox="0 0 24 24" fill="currentColor">
                    <path d="M2 17h2v4H2v-4zm4-5h2v9H6v-9zm4-4h2v13h-2V8zm4-4h2v17h-2V4zm4-4h2v21h-2V0z" />
                </svg>
            </span>
            <span>Connection quality: Good</span>
        </div>
    </footer>

    <!-- 4. Incoming Connection Request Modal Overlay (Exact Design Match for Image 2) -->
    <div class="modal-backdrop-overlay" id="incomingModalOverlay" onclick="handleBackdropClick(event)">
        <div class="incoming-modal-card" id="incomingModalCard">
            <!-- Red Screen Share Avatar Badge -->
            <div class="modal-red-icon-badge">
                <svg width="36" height="36" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <rect x="2" y="3" width="20" height="14" rx="2" />
                    <line x1="8" y1="21" x2="16" y2="21" />
                    <line x1="12" y1="17" x2="12" y2="21" />
                    <circle cx="12" cy="10" r="2.5" />
                    <path d="M8 15c0-1.5 1.5-2 4-2s4 .5 4 2" />
                </svg>
            </div>

            <h3 class="modal-title">Incoming Connection Request</h3>
            <p class="modal-subtitle">Someone is trying to connect to your device</p>

            <!-- Information Card -->
            <div class="modal-info-card">
                <div class="info-row">
                    <div class="info-label-group">
                        <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor"
                            stroke-width="2">
                            <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2" />
                            <circle cx="12" cy="7" r="4" />
                        </svg>
                        <span>Remote Device</span>
                    </div>
                    <div class="info-value-group">
                        <span id="modalDeviceName"><?= htmlspecialchars($hostAlias) ?></span>
                        <span class="verified-shield-icon" title="Verified device">
                            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor"
                                stroke-width="2.5">
                                <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
                                <polyline points="9 12 11 14 15 10" />
                            </svg>
                        </span>
                    </div>
                </div>

                <div class="info-row">
                    <div class="info-label-group">
                        <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor"
                            stroke-width="2">
                            <rect x="2" y="3" width="20" height="14" rx="2" />
                            <line x1="8" y1="21" x2="16" y2="21" />
                            <line x1="12" y1="17" x2="12" y2="21" />
                        </svg>
                        <span>Remote ID</span>
                    </div>
                    <div class="info-value-group">
                        <span class="red-id-highlight"
                            id="modalRemoteId"><?= htmlspecialchars(formatDeviceId($hostUid)) ?></span>
                    </div>
                </div>

                <div class="info-row">
                    <div class="info-label-group">
                        <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor"
                            stroke-width="2">
                            <path d="M21 10c0 7-9 13-9 13s-9-6-9-13a9 9 0 0 1 18 0z" />
                            <circle cx="12" cy="10" r="3" />
                        </svg>
                        <span>IP Address</span>
                    </div>
                    <div class="info-value-group">
                        <span id="modalIpAddress">103.247.12.45</span>
                    </div>
                </div>

                <div class="info-row">
                    <div class="info-label-group">
                        <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor"
                            stroke-width="2">
                            <circle cx="12" cy="12" r="10" />
                            <polyline points="12 6 12 12 16 14" />
                        </svg>
                        <span>Request Time</span>
                    </div>
                    <div class="info-value-group">
                        <span id="modalRequestTime">Today, 10:32 AM</span>
                    </div>
                </div>
            </div>

            <!-- Permissions / Allowed Actions -->
            <div class="permissions-section">
                <div class="permissions-label">Allow the remote device to:</div>
                <div class="permissions-grid">
                    <div class="permission-card">
                        <div class="perm-top-row">
                            <div class="perm-icon">
                                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor"
                                    stroke-width="2">
                                    <rect x="2" y="3" width="20" height="14" rx="2" />
                                    <line x1="8" y1="21" x2="16" y2="21" />
                                    <line x1="12" y1="17" x2="12" y2="21" />
                                </svg>
                            </div>
                            <label class="switch">
                                <input type="checkbox" id="permViewScreen" checked>
                                <span class="slider"></span>
                            </label>
                        </div>
                        <div>
                            <div class="perm-title">View Screen</div>
                            <div class="perm-desc">Allow remote view</div>
                        </div>
                    </div>

                    <div class="permission-card">
                        <div class="perm-top-row">
                            <div class="perm-icon">
                                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor"
                                    stroke-width="2">
                                    <rect x="5" y="2" width="14" height="20" rx="7" />
                                    <line x1="12" y1="6" x2="12" y2="10" />
                                </svg>
                            </div>
                            <label class="switch">
                                <input type="checkbox" id="permControlInput" checked>
                                <span class="slider"></span>
                            </label>
                        </div>
                        <div>
                            <div class="perm-title">Control Input</div>
                            <div class="perm-desc">Allow mouse and keyboard</div>
                        </div>
                    </div>

                    <div class="permission-card">
                        <div class="perm-top-row">
                            <div class="perm-icon">
                                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor"
                                    stroke-width="2">
                                    <path
                                        d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
                                </svg>
                            </div>
                            <label class="switch">
                                <input type="checkbox" id="permFileTransfer" checked>
                                <span class="slider"></span>
                            </label>
                        </div>
                        <div>
                            <div class="perm-title">File Transfer</div>
                            <div class="perm-desc">Allow file transfer</div>
                        </div>
                    </div>
                </div>
            </div>

            <!-- Action Buttons: Deny vs Accept -->
            <div class="modal-action-row">
                <button class="btn-modal-deny" type="button" onclick="denyConnection()">
                    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                        <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
                    </svg>
                    Deny
                </button>
                <button class="btn-modal-accept" type="button" onclick="acceptConnection()">
                    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor"
                        stroke-width="2.5">
                        <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
                        <polyline points="9 12 11 14 15 10" />
                    </svg>
                    Accept and connect
                </button>
            </div>

            <!-- Remember My Choice Checkbox -->
            <div class="remember-choice-wrapper">
                <label class="remember-checkbox-label">
                    <input type="checkbox" id="rememberChoiceCheck">
                    Remember my choice for this device
                </label>
                <div class="remember-subtext">You can change this later in Settings &gt; Security</div>
            </div>
        </div>
    </div>

    <!-- Toast Notification Element -->
    <div class="toast-popup" id="toastBox">
        <span id="toastMsg">Action completed</span>
    </div>

    <!-- Client-side Realtime Engine & Modal Controller -->
    <script>
        // State variables
        let selectedMode = 'desktop';
        let currentTargetUid = '<?= htmlspecialchars($hostUid) ?>';
        let currentHostUid = '<?= htmlspecialchars($hostUid) ?>';
        let currentRequestToken = null;
        let pendingPollInterval = null;
        let registeredDevicesList = [];

        // 1. Connection Mode Selector
        function selectMode(mode) {
            selectedMode = mode;
            document.getElementById('modeDesktop').classList.toggle('active', mode === 'desktop');
            document.getElementById('modeViewOnly').classList.toggle('active', mode === 'view');
        }

        // 2. Format Device ID into neat 3-3-3 chunks
        function formatId(raw) {
            if (!raw) return '------';
            const clean = raw.toString().replace(/[^0-9]/g, '');
            if (clean.length === 9) {
                return clean.slice(0, 3) + ' ' + clean.slice(3, 6) + ' ' + clean.slice(6);
            }
            if (clean.length === 6) {
                return clean.slice(0, 3) + ' ' + clean.slice(3);
            }
            return clean.replace(/(\d{3})(?=\d)/g, '$1 ');
        }

        // 3. Copy Host ID to clipboard with toast
        function copyHostId() {
            const rawId = currentHostUid;
            if (navigator.clipboard) {
                navigator.clipboard.writeText(rawId.replace(/\s/g, ''));
            }
            showToast('System ID copied to clipboard: ' + formatId(rawId));
        }

        // 4. Toast notification helper
        function showToast(msg) {
            const toast = document.getElementById('toastBox');
            document.getElementById('toastMsg').innerText = msg;
            toast.classList.add('show');
            setTimeout(() => {
                toast.classList.remove('show');
            }, 3000);
        }

        // 5. Trigger Inline Alias Editing
        function triggerAliasEdit() {
            const aliasSpan = document.getElementById('mainHostAlias');
            const aliasInput = document.getElementById('hostAliasInput');
            aliasSpan.style.display = 'none';
            aliasInput.style.display = 'inline-block';
            aliasInput.focus();
            aliasInput.select();
        }

        function handleAliasKey(e) {
            if (e.key === 'Enter') {
                saveAliasInline();
            } else if (e.key === 'Escape') {
                document.getElementById('mainHostAlias').style.display = 'inline-block';
                document.getElementById('hostAliasInput').style.display = 'none';
            }
        }

        async function saveAliasInline() {
            const aliasInput = document.getElementById('hostAliasInput');
            const newAlias = aliasInput.value.trim() || 'Workstation';

            document.getElementById('mainHostAlias').innerText = newAlias;
            document.getElementById('sidebarAliasDisplay').innerText = newAlias;
            document.getElementById('mainHostAlias').style.display = 'inline-block';
            aliasInput.style.display = 'none';

            try {
                await fetch('../backend/api/devices/update.php', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ device_uid: currentHostUid, alias: newAlias })
                });
                showToast('Device alias updated to "' + newAlias + '"');
            } catch (err) {
                showToast('Saved locally');
            }
        }

        // 6. Connect Button Flow (State Machine Request Creation & Status Polling)
        async function startConnection() {
            const inputVal = document.getElementById('remoteIdInput').value.trim();
            const cleanId = inputVal.replace(/[^0-9]/g, '');

            if (!cleanId) {
                showToast('Please enter a valid remote System ID');
                document.getElementById('remoteIdInput').focus();
                return;
            }

            if (cleanId === currentHostUid.replace(/[^0-9]/g, '')) {
                showToast('Cannot connect to your own System ID');
                return;
            }

            showToast('Sending connection request to ' + formatId(cleanId) + '...');

            try {
                const res = await fetch('../backend/api/connections/request.php', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({
                        target_system_id: cleanId,
                        requester_system_id: currentHostUid,
                        requester_name: '<?= htmlspecialchars($hostAlias) ?>'
                    })
                });

                const data = await res.json();

                if (data.status !== 'success') {
                    showToast(data.message || 'Failed to initiate connection');
                    return;
                }

                const token = data.request.request_token;
                showToast('Request sent. Waiting for remote approval (up to 60s)...');

                // Clear any existing polling loop
                if (pendingPollInterval) clearInterval(pendingPollInterval);

                let pollCount = 0;
                pendingPollInterval = setInterval(async () => {
                    pollCount++;
                    if (pollCount > 65) {
                        clearInterval(pendingPollInterval);
                        showToast('Connection request expired.');
                        return;
                    }

                    try {
                        const statusRes = await fetch(`../backend/api/connections/status.php?token=${encodeURIComponent(token)}`);
                        const statusData = await statusRes.json();

                        if (statusData.request_status === 'accepted') {
                            clearInterval(pendingPollInterval);
                            showToast('Connection APPROVED! Launching session...');
                            setTimeout(() => {
                                window.location.href = `remote/session.php?id=${encodeURIComponent(cleanId)}&mode=${selectedMode}&token=${encodeURIComponent(token)}`;
                            }, 500);
                        } else if (statusData.request_status === 'rejected') {
                            clearInterval(pendingPollInterval);
                            showToast('Connection was REJECTED by the remote computer.');
                        } else if (statusData.request_status === 'expired') {
                            clearInterval(pendingPollInterval);
                            showToast('Connection request expired (timeout).');
                        }
                    } catch (e) {
                        // ignore network blips
                    }
                }, 1000);

            } catch (err) {
                showToast('Server error initiating connection request');
            }
        }

        // 7. Incoming Connection Request Modal Controller (Image 2)
        let handledTokens = new Set();

        function promptIncomingModal(name, uid, ip, time, token = null) {
            if (token && handledTokens.has(token)) {
                return;
            }
            console.log("[REQUEST] Incoming remote request");
            console.log("[REQUEST] Request ID:", token);
            console.log("[REQUEST] Target User ID:", currentHostUid);
            console.log("[REQUEST] Requester User ID:", uid);

            currentTargetUid = uid;
            currentRequestToken = token;
            document.getElementById('modalDeviceName').innerText = name;
            document.getElementById('modalRemoteId').innerText = formatId(uid);
            document.getElementById('modalIpAddress').innerText = ip;
            document.getElementById('modalRequestTime').innerText = time || 'Today, ' + new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });

            const overlay = document.getElementById('incomingModalOverlay');
            overlay.classList.add('open');
        }

        async function denyConnection() {
            const overlay = document.getElementById('incomingModalOverlay');
            overlay.classList.remove('open');

            if (currentRequestToken) {
                handledTokens.add(currentRequestToken);
                console.log("[REQUEST] REJECT clicked");
                console.log("[REQUEST] Sending REJECT for Request ID:", currentRequestToken);
                try {
                    await fetch('../backend/api/connections/reject.php', {
                        method: 'POST',
                        headers: { 'Content-Type': 'application/json' },
                        body: JSON.stringify({
                            request_token: currentRequestToken,
                            target_system_id: currentHostUid
                        })
                    });
                } catch (e) { }
            }

            showToast('Connection request rejected');
        }

        async function acceptConnection() {
            const overlay = document.getElementById('incomingModalOverlay');
            overlay.classList.remove('open');

            if (currentRequestToken) {
                handledTokens.add(currentRequestToken);
                console.log("[REQUEST] ACCEPT clicked");
                console.log("[REQUEST] Sending ACCEPT for Request ID:", currentRequestToken);
                try {
                    await fetch('../backend/api/connections/accept.php', {
                        method: 'POST',
                        headers: { 'Content-Type': 'application/json' },
                        body: JSON.stringify({
                            request_token: currentRequestToken,
                            target_system_id: currentHostUid
                        })
                    });
                } catch (e) { }
            }

            showToast('Connection accepted. Remote screen sharing active.');
        }

        function handleBackdropClick(e) {
            if (e.target.id === 'incomingModalOverlay') {
                denyConnection();
            }
        }

        // 8. Sidebar tab active style
        function setSidebarActive(btn) {
            document.querySelectorAll('.nav-item').forEach(el => el.classList.remove('active'));
            btn.classList.add('active');
        }

        function scrollToSection(id) {
            const el = document.getElementById(id);
            if (el) el.scrollIntoView({ behavior: 'smooth' });
        }

        function toggleFavorite(btn) {
            if (btn.innerText === '☆') {
                btn.innerText = '★';
                btn.style.color = '#f59e0b';
                showToast('Added to Favorites');
            } else {
                btn.innerText = '☆';
                btn.style.color = 'var(--text-subtle)';
                showToast('Removed from Favorites');
            }
        }

        // 9. Quick Select Dropdown for Recent Devices
        function toggleRecentDropdown() {
            const dd = document.getElementById('recentDropdown');
            dd.classList.toggle('open');
        }

        function selectDropdownId(uid) {
            document.getElementById('remoteIdInput').value = uid;
            document.getElementById('recentDropdown').classList.remove('open');
            startConnection();
        }

        document.addEventListener('click', (e) => {
            if (!e.target.closest('.input-with-dropdown')) {
                document.getElementById('recentDropdown').classList.remove('open');
            }
        });

        let isHostOnlineGlobal = <?php echo $isHostOnline ? 'true' : 'false'; ?>;

        function cleanId(id) {
            return String(id || '').replace(/[^0-9]/g, '');
        }

        async function startDesktopAgent() {
            console.log("[AGENT UI] Starting Desktop Agent");
            console.log("[AGENT UI] Agent executable: dist/DeskStream.exe");
            showToast("Starting Desktop Agent in background...");

            try {
                const res = await fetch('../backend/api/agent/launch.php', { method: 'POST' });
                const data = await res.json();
                if (data.status === 'success') {
                    if (data.already_running) {
                        console.log("[AGENT UI] Agent process already running (PID: " + data.pid + ")");
                        showToast("DeskStream Agent is already running (PID: " + data.pid + ")");
                    } else {
                        console.log("[AGENT UI] Agent process started");
                        console.log("[AGENT UI] PID: " + data.pid);
                        showToast("Desktop Agent started (PID: " + data.pid + ")");
                    }
                    setTimeout(() => fetchAllDevices(true), 1000);
                } else {
                    console.error("[AGENT UI] Failed to start agent:", data.message);
                    showToast("Failed to start Desktop Agent: " + data.message);
                }
            } catch (err) {
                console.warn("[AGENT UI] Launch API failed, trying deskstream:// custom protocol...", err);
                window.location.href = 'deskstream://open';
            }
        }

        // 10. Live Polling Engine (Queries Database via API every 3 seconds)
        async function fetchAllDevices(isManual = false) {
            try {
                const res = await fetch('../backend/api/devices/list.php');
                if (!res.ok) throw new Error('API offline');
                const data = await res.json();

                if (data.status === 'success' && Array.isArray(data.devices)) {
                    registeredDevicesList = data.devices;

                    // If host UID is not set or was connecting, check if our machine has registered
                    if (!currentHostUid || currentHostUid === 'Connecting...') {
                        const localDev = data.devices.find(d => d.ip_address === '127.0.0.1' || d.ip_address === '::1');
                        if (localDev) {
                            currentHostUid = localDev.system_id || localDev.device_uid;
                            document.getElementById('mainHostId').innerText = formatId(currentHostUid);
                            document.getElementById('sidebarIdDisplay').innerText = formatId(currentHostUid);
                            if (localDev.name) {
                                document.getElementById('mainHostAlias').innerText = localDev.name;
                                document.getElementById('sidebarAliasDisplay').innerText = localDev.name;
                            }
                        }
                    }

                    // Dynamically update "This Device" readiness badge
                    if (currentHostUid && currentHostUid !== 'Connecting...') {
                        const myDev = data.devices.find(d => cleanId(d.system_id) === cleanId(currentHostUid) || cleanId(d.device_uid) === cleanId(currentHostUid));
                        const isOnline = myDev && (myDev.is_online == 1 || myDev.is_online === true);
                        isHostOnlineGlobal = isOnline;

                        if (isOnline) {
                            console.log("[STATUS] Backend returned agent = ONLINE");
                            console.log("[STATUS] Device ID = " + myDev.system_id);
                            console.log("[UI] Updating agent status = ONLINE");
                        }

                        const badge = document.querySelector('.connection-ready-badge');
                        if (badge) {
                            badge.innerHTML = isOnline
                                ? '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/><polyline points="9 12 11 14 15 10"/></svg> <span>Agent Online (Ready for connections)</span>'
                                : '<span style="color:#64748b; display:inline-flex; align-items:center; gap:6px;"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/></svg> Agent Offline (Start Desktop Agent)</span>';
                        }
                    }

                    // Filter other workstations for the grid
                    const otherDevices = data.devices.filter(d => (cleanId(d.system_id) !== cleanId(currentHostUid) && cleanId(d.device_uid) !== cleanId(currentHostUid)));
                    renderDevicesGrid(otherDevices);
                    populateRecentDropdown(otherDevices);
                    if (isManual) showToast('Workstation inventory synchronized');
                }
            } catch (err) {
                if (isManual) showToast('Database sync in progress...');
            }

            // Check for incoming connection requests targeted to this computer
            if (currentHostUid && currentHostUid !== 'Connecting...') {
                try {
                    const incRes = await fetch(`../backend/api/connections/incoming.php?system_id=${encodeURIComponent(currentHostUid)}`);
                    const incData = await incRes.json();
                    if (incData.has_request && incData.request) {
                        const req = incData.request;
                        // If modal is not already open, display incoming request
                        const modalOverlay = document.getElementById('incomingModalOverlay');
                        if (!modalOverlay.classList.contains('open')) {
                            promptIncomingModal(
                                req.requester_name || 'Remote User',
                                req.requester_system_id,
                                '127.0.0.1',
                                'Just now',
                                req.request_token
                            );
                        }
                    }
                } catch (e) { }
            }
        }

        function getOsSvg(osType) {
            osType = (osType || 'windows').toLowerCase();
            if (osType.includes('mac') || osType.includes('apple')) {
                return '<svg width="22" height="22" viewBox="0 0 24 24" fill="currentColor"><path d="M18.71 19.5c-.83 1.24-1.71 2.45-3.05 2.47-1.34.03-1.77-.79-3.29-.79-1.53 0-2 .77-3.27.82-1.31.05-2.3-1.32-3.14-2.53C4.25 17 2.94 12.45 4.7 9.39c.87-1.52 2.43-2.48 4.12-2.51 1.28-.02 2.5.87 3.29.87.78 0 2.26-1.07 3.81-.91.65.03 2.47.26 3.64 1.98-.09.06-2.17 1.28-2.15 3.81.03 3.02 2.65 4.03 2.68 4.04-.03.07-.42 1.44-1.38 2.83M15.97 6.37c.63-.77 1.06-1.84.94-2.91-.91.04-2.02.61-2.67 1.37-.58.67-1.09 1.76-.95 2.8.01 0 .03 0 .05 0 1.02 0 2-.49 2.63-1.26z"/></svg>';
            }
            if (osType.includes('linux')) {
                return '<svg width="22" height="22" viewBox="0 0 24 24" fill="currentColor"><path d="M12 2a5 5 0 0 0-5 5v3a5 5 0 0 0 10 0V7a5 5 0 0 0-5-5z"/></svg>';
            }
            // Windows
            return '<svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor"><path d="M3 5.557L10.375 4.5v6.924H3V5.557zm0 12.886l7.375 1.057v-6.924H3v5.867zm8.344 1.218L21 21.2V11.424H11.344v8.237zM21 2.8L11.344 4.361v7.063H21V2.8z"/></svg>';
        }

        function formatRelativeTime(dateStr) {
            if (!dateStr) return 'Recently active';
            const d = new Date(dateStr);
            if (isNaN(d.getTime())) return 'Recently active';
            const now = new Date();
            const diffSec = Math.floor((now - d) / 1000);

            if (diffSec < 60) return 'Just now';
            if (diffSec < 3600) return Math.floor(diffSec / 60) + ' mins ago';

            const isToday = d.toDateString() === now.toDateString();
            const timeFormatted = d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });

            if (isToday) return 'Today, ' + timeFormatted;

            const yesterday = new Date(now);
            yesterday.setDate(yesterday.getDate() - 1);
            if (d.toDateString() === yesterday.toDateString()) return 'Yesterday, ' + timeFormatted;

            return d.toLocaleDateString([], { day: '2-digit', month: 'short', year: 'numeric' });
        }

        function renderDevicesGrid(devices) {
            const container = document.getElementById('workstationsContainer');
            if (!devices || devices.length === 0) {
                container.innerHTML = '<div class="empty-grid-box">No remote desktop agents connected yet. Launch your desktop agent to connect automatically!</div>';
                return;
            }

            const colors = ['#0284c7', '#ef4444', '#16a34a', '#f59e0b', '#8b5cf6', '#0d9488'];

            container.innerHTML = devices.map((dev, idx) => {
                const color = colors[idx % colors.length];
                const isOnline = dev.is_online == 1;
                const timeStr = formatRelativeTime(dev.last_seen_at || dev.created_at);
                const safeName = (dev.name || 'Workstation').replace(/'/g, "\\'");
                const safeUid = (dev.device_uid || '').replace(/'/g, "\\'");
                const safeIp = (dev.ip_address || '192.168.1.100').replace(/'/g, "\\'");

                return `
                    <div class="workstation-card" onclick="selectDropdownId('${safeUid}')">
                        <div class="device-avatar-circle" style="background: ${color};">
                            ${getOsSvg(dev.os_type)}
                        </div>
                        <div class="card-main-info">
                            <div class="card-top-row">
                                <div class="device-name-title">
                                    ${dev.name || 'Workstation'}
                                    <span class="online-dot ${isOnline ? 'active' : ''}" title="${isOnline ? 'Online' : 'Offline'}"></span>
                                </div>
                                <div class="card-top-actions" onclick="event.stopPropagation()">
                                    <button class="star-fav-btn" title="Favorite" onclick="toggleFavorite(this)">☆</button>
                                    <button class="more-opt-btn" title="Options" onclick="showToast('Device UID: ${dev.device_uid}')">⋮</button>
                                </div>
                            </div>
                            <div class="device-ip-text">${dev.ip_address || '192.168.1.100'}</div>
                            <div class="last-active-row">
                                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></svg>
                                ${timeStr}
                            </div>
                        </div>
                    </div>
                `;
            }).join('');
        }

        function populateRecentDropdown(devices) {
            const dd = document.getElementById('recentDropdown');
            if (!devices || devices.length === 0) {
                dd.innerHTML = '<div class="dropdown-item" style="color:var(--text-muted);">No recent devices</div>';
                return;
            }

            dd.innerHTML = devices.slice(0, 5).map(dev => `
                <div class="dropdown-item" onclick="selectDropdownId('${dev.device_uid}')">
                    <span><strong>${dev.name || 'Workstation'}</strong> (${formatId(dev.device_uid)})</span>
                    <span style="font-size:0.75rem; color:var(--text-muted);">${dev.ip_address || ''}</span>
                </div>
            `).join('');
        }

        // Initialize: Fetch on start and poll every 3 seconds
        fetchAllDevices();
        setInterval(() => fetchAllDevices(false), 3000);
    </script>
</body>

</html>