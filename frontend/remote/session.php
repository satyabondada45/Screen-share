<?php
if (session_status() === PHP_SESSION_NONE) {
    session_start();
}

// Require authenticated user session
if (empty($_SESSION['user_id'])) {
    header("Location: ../index.php");
    exit();
}

$dbPath = __DIR__ . '/../config/database.php';

if (!isset($pdo) || !($pdo instanceof PDO)) {
    if (!file_exists($dbPath)) {
        http_response_code(500);
        exit('Database configuration file not found.');
    }

    require_once $dbPath;
}

if (!isset($pdo) || !($pdo instanceof PDO)) {
    http_response_code(500);
    exit('Database connection unavailable.');
}

$deviceUid = $_GET['id'] ?? null;
$sessionToken = $_GET['token'] ?? null;
$isIntegrated = isset($_GET['integrated']) && $_GET['integrated'] == '1';
$layer = $_GET['layer'] ?? ($isIntegrated ? 'A' : 'B');


if (!$deviceUid) {
    header("Location: ../dashboard.php");
    exit();
}

// Clean alphanumeric device identifier / system ID
$cleanId = preg_replace('/[^0-9a-zA-Z_\-]/', '', (string) $deviceUid);

$stmt = $pdo->prepare("
    SELECT *
    FROM devices
    WHERE device_uid = :device_uid OR system_id = :system_id
    LIMIT 1
");

$stmt->execute([
    ':device_uid' => $cleanId,
    ':system_id' => $cleanId
]);

$device = $stmt->fetch(PDO::FETCH_ASSOC);

if (!$device) {
    die("Error: Target device not found in database.");
}

$deviceName = $device['name'] ?? 'Unknown Device';
$isOnline = !empty($device['is_online']);

// Requested Step 2: Log both identifiers to verify routing
$accountId = $_SESSION['user_id'];
$requestedId = $cleanId;
$systemId = $device['system_id'] ?: $device['device_uid'];
?>
<!-- DIAGNOSTIC LOGS -->
<script>
    console.log("[SESSION DEBUG]");
    console.log("Account ID = <?= htmlspecialchars($accountId) ?>");
    console.log("Requested Device ID = <?= htmlspecialchars($requestedId) ?>");
    console.log("Registered System ID = <?= htmlspecialchars($systemId) ?>");
</script>
<?php
$sessionCode = strlen($cleanId) === 9
    ? substr($cleanId, 0, 3) . ' ' . substr($cleanId, 3, 3) . ' ' . substr($cleanId, 6, 3)
    : (strlen($cleanId) > 3 ? substr($cleanId, 0, 3) . '-' . substr($cleanId, 3) : $cleanId);

// The relay WebSocket URL is configurable via the RELAY_WS_URL environment variable.
// In production, this is set to wss://your-domain.com:9001 (or ws://server-ip:9001 for LAN).
// In development/local testing, it defaults to ws://localhost:9001.
$relayWsUrl = getenv('RELAY_WS_URL') ?: 'wss://admin.friendssoftwaresolutions.in';
// Allow override via query parameter for testing (dev only)
if (isset($_GET['relay']) && getenv('APP_ENV') !== 'production') {
    $relayWsUrl = $_GET['relay'];
}
?>
<!DOCTYPE html>

<html lang="en">

<head>

    <meta charset="UTF-8">

    <meta name="viewport" content="width=device-width, initial-scale=1.0">

    <title>
        Active Remote Session - <?= htmlspecialchars($deviceName, ENT_QUOTES, 'UTF-8') ?>
    </title>

    <style>
        #reversePermissionDialog {
            position: fixed;
            inset: auto;
            left: 20px;
            bottom: 20px;
            z-index: 11000;
            width: min(360px, calc(100vw - 40px));
        }

        .reverse-permission-card {
            padding: 16px;
            border: 1px solid #e2e8f0;
            border-radius: 14px;
            background: #fff;
            color: #111827;
            box-shadow: 0 12px 32px rgba(15, 23, 42, 0.22);
        }

        .reverse-permission-card h2 {
            margin: 0 0 8px;
            font-size: 1rem;
        }

        .reverse-permission-card p {
            margin: 0 0 14px;
            color: #4b5563;
            font-size: 0.9rem;
            line-height: 1.45;
        }

        .reverse-permission-actions {
            display: flex;
            justify-content: flex-end;
            gap: 8px;
        }

        #fileTransferChoiceDialog {
            position: fixed;
            inset: 0;
            z-index: 10001;
            display: none;
            align-items: center;
            justify-content: center;
            padding: 20px;
            background: rgba(15, 23, 42, 0.35);
        }

        .file-transfer-choice-card {
            width: min(360px, 100%);
            padding: 20px;
            border-radius: 14px;
            background: #fff;
            box-shadow: 0 16px 40px rgba(15, 23, 42, 0.22);
        }

        .file-transfer-choice-actions {
            display: flex;
            flex-wrap: wrap;
            justify-content: flex-end;
            gap: 8px;
            margin-top: 16px;
        }

        #folderDestinationDialog {
            position: fixed;
            inset: 0;
            z-index: 10002;
            display: none;
            align-items: center;
            justify-content: center;
            padding: 20px;
            background: rgba(15, 23, 42, 0.35);
        }

        :root {
            --bg-dark: #000;
            --header-bg: #fff;
            --border: #e5e5e5;
            --accent: #ef4444;
            --text-dark: #111827;
            --text-muted: #6b7280;
            --online: #22c55e;
            --btn-bg: #fff;
            --btn-border: #d1d5db;
        }

        * {
            box-sizing: border-box;
            margin: 0;
            padding: 0;
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
        }

        body {
            background: var(--bg-dark);
            color: var(--text-dark);
            height: 100vh;
            display: flex;
            flex-direction: column;
            overflow: hidden;
        }

        /* HEADER */
        .viewer-header {
            height: 54px;
            background: var(--header-bg);
            border-bottom: 1px solid var(--border);
            display: flex;
            justify-content: space-between;
            align-items: center;
            padding: 0 16px;
            flex-shrink: 0;
            z-index: 100;
        }

        .header-left {
            display: flex;
            align-items: center;
            gap: 24px;
        }

        .logo {
            display: flex;
            align-items: center;
            gap: 8px;
            text-decoration: none;
            color: var(--text-dark);
            font-weight: 800;
            font-size: 1.1rem;
            letter-spacing: -0.02em;
        }

        .device-info {
            display: flex;
            flex-direction: column;
            border-left: 1px solid var(--border);
            padding-left: 20px;
        }

        .device-name {
            font-weight: 600;
            font-size: 0.9rem;
            display: flex;
            align-items: center;
            gap: 8px;
        }

        .device-meta {
            font-size: 0.75rem;
            color: var(--text-muted);
            display: flex;
            align-items: center;
            gap: 6px;
            margin-top: 2px;
        }

        .dot {
            width: 8px;
            height: 8px;
            border-radius: 50%;
            background: var(--online);
        }

        .dot-text {
            color: var(--online);
            font-weight: 500;
        }

        /* HEADER ACTIONS */
        .header-right {
            display: flex;
            align-items: center;
            gap: 12px;
        }

        .action-group {
            display: flex;
            gap: 8px;
            margin-right: 16px;
            border-right: 1px solid var(--border);
            padding-right: 16px;
        }

        .btn {
            display: inline-flex;
            flex-direction: column;
            align-items: center;
            justify-content: center;
            background: var(--btn-bg);
            border: 1px solid var(--btn-border);
            border-radius: 6px;
            padding: 4px 12px;
            font-size: 0.7rem;
            font-weight: 500;
            color: var(--text-dark);
            cursor: pointer;
            transition: 0.15s;
            text-decoration: none;
            min-width: 60px;
        }

        .btn:hover {
            background: #f9fafb;
            border-color: #9ca3af;
        }

        .btn svg {
            margin-bottom: 2px;
        }

        .btn-end {
            border-color: var(--accent);
            color: var(--accent);
            flex-direction: row;
            gap: 6px;
            padding: 6px 16px;
            font-size: 0.85rem;
            font-weight: 600;
        }

        .btn-end:hover {
            background: #fef2f2;
            border-color: #dc2626;
            color: #dc2626;
        }

        .btn-end svg {
            margin-bottom: 0;
        }

        .window-controls {
            display: flex;
            gap: 16px;
            color: var(--text-muted);
            font-size: 1rem;
            user-select: none;
        }

        .window-controls span {
            cursor: pointer;
        }

        .window-controls span:hover {
            color: var(--text-dark);
        }

        /* MAIN STREAM AREA */
        .stream-container {
            flex: 1;
            position: relative;
            display: flex;
            background: #000;
            overflow: hidden;
        }

        canvas {
            width: 100%;
            height: 100%;
            object-fit: contain;
            display: block;
            outline: none;
            background: #000;
            cursor: default;
        }

        /* FLOATING CONTROLS */
        .floating-controls {
            position: absolute;
            bottom: 24px;
            right: 24px;
            background: white;
            border-radius: 8px;
            box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
            display: flex;
            gap: 4px;
            padding: 6px;
            z-index: 150;
            transition: opacity 0.2s;
        }

        .toolbar-drag-handle {
            display: flex;
            align-items: center;
            justify-content: center;
            padding: 0 6px;
            cursor: grab;
            color: #94a3b8;
            border-right: 1px solid var(--border);
            margin-right: 4px;
            user-select: none;
        }
        
        .toolbar-drag-handle:active {
            cursor: grabbing;
        }

        .floating-btn {
            display: flex;
            flex-direction: column;
            align-items: center;
            justify-content: center;
            background: transparent;
            border: none;
            padding: 4px 10px;
            font-size: 0.7rem;
            font-weight: 500;
            color: var(--text-dark);
            cursor: pointer;
            border-radius: 6px;
        }

        .floating-btn:hover {
            background: #f3f4f6;
        }

        .floating-btn svg {
            margin-bottom: 2px;
        }

        /* SESSION PANEL */
        .session-panel {
            position: absolute;
            right: 24px;
            top: 24px;
            width: 320px;
            background: white;
            border-radius: 12px;
            box-shadow: 0 8px 24px rgba(0, 0, 0, 0.15);
            z-index: 60;
            display: none;
            flex-direction: column;
            overflow: hidden;
        }

        .session-panel.open {
            display: flex;
        }

        .panel-tabs {
            display: flex;
            border-bottom: 1px solid var(--border);
        }

        .panel-tab {
            flex: 1;
            text-align: center;
            padding: 12px;
            font-size: 0.85rem;
            font-weight: 600;
            cursor: pointer;
            color: var(--text-muted);
        }

        .panel-tab.active {
            color: var(--accent);
            border-bottom: 2px solid var(--accent);
        }

        .panel-content {
            padding: 16px;
            font-size: 0.8rem;
        }

        .info-row {
            display: flex;
            justify-content: space-between;
            margin-bottom: 12px;
        }

        .info-label {
            color: var(--text-muted);
        }

        .info-val {
            font-weight: 500;
            text-align: right;
        }

        .info-val.highlight {
            color: var(--online);
        }

        .info-val.red {
            color: var(--accent);
        }

        .panel-section-title {
            font-weight: 700;
            margin: 16px 0 12px;
            font-size: 0.85rem;
        }

        /* CHAT PANEL */
        .chat-panel {
            position: absolute;
            top: 24px;
            right: 360px;
            width: 300px;
            background: white;
            border-radius: 10px;
            box-shadow: 0 8px 24px rgba(0, 0, 0, 0.15);
            z-index: 60;
            display: none;
            flex-direction: column;
            overflow: hidden;
        }

        .chat-panel.open {
            display: flex;
        }

        .chat-header {
            background: #f9fafb;
            padding: 12px;
            font-weight: bold;
            border-bottom: 1px solid var(--border);
            display: flex;
            justify-content: space-between;
            font-size: 0.85rem;
        }

        .chat-close {
            cursor: pointer;
            color: var(--text-muted);
        }

        .chat-messages {
            height: 250px;
            overflow-y: auto;
            padding: 12px;
            font-size: 0.8rem;
            display: flex;
            flex-direction: column;
            gap: 8px;
            background: white;
        }

        .chat-typing {
            display: none;
            padding: 0 12px 8px;
            color: var(--text-muted);
            font-size: 0.75rem;
            font-style: italic;
        }

        .chat-typing.visible {
            display: block;
        }

        .msg {
            padding: 8px 12px;
            border-radius: 8px;
            max-width: 85%;
            word-wrap: break-word;
        }

        .msg.sent {
            background: #eff6ff;
            align-self: flex-end;
            color: #1e3a8a;
        }

        .msg.recv {
            background: #f3f4f6;
            align-self: flex-start;
            color: #111827;
        }

        .chat-input {
            display: flex;
            border-top: 1px solid var(--border);
        }

        .chat-input input {
            flex: 1;
            padding: 10px;
            border: none;
            outline: none;
            font-size: 0.8rem;
        }

        .chat-input button {
            background: var(--accent);
            border: none;
            color: white;
            padding: 0 12px;
            cursor: pointer;
            font-weight: 600;
            font-size: 0.8rem;
        }

        /* OTHERS */
        .hud-badge {
            position: absolute;
            top: 12px;
            right: 12px;
            background: rgba(0, 0, 0, 0.6);
            padding: 4px 8px;
            border-radius: 4px;
            font-family: monospace;
            font-size: 0.75rem;
            color: #fff;
            z-index: 10;
            pointer-events: none;
        }

        .drop-overlay {
            position: absolute;
            inset: 0;
            background: rgba(59, 130, 246, 0.8);
            display: flex;
            flex-direction: column;
            justify-content: center;
            align-items: center;
            color: white;
            font-size: 1.2rem;
            font-weight: bold;
            z-index: 20;
            opacity: 0;
            pointer-events: none;
            transition: 0.2s;
        }

        .drop-overlay.active {
            opacity: 1;
            pointer-events: auto;
        }
    </style>

</head>

<body>

    <?php if ($layer === 'A'): ?>
    <div class="viewer-header">
        <div class="header-left">
            <div class="logo" aria-label="DeskStream">
                <svg width="24" height="24" viewBox="0 0 32 32" fill="none">
                    <path d="M6 16L16 6L20 10L12 18L6 16Z" fill="#ef4444" />
                    <path d="M12 22L22 12L26 16L18 24L12 22Z" fill="#dc2626" />
                    <path d="M16 28L26 18L30 22L20 32L16 28Z" fill="#b91c1c" />
                </svg>
                DeskStream
            </div>
            <div class="device-info">
                <div class="device-name" style="display: flex; align-items: center; gap: 8px;">
                    <span style="color: #94a3b8; font-weight: normal;"><?= htmlspecialchars($_SESSION['username'] ?? 'Local Computer', ENT_QUOTES, 'UTF-8') ?></span>
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#ef4444" stroke-width="2">
                        <path d="M5 12h14M12 5l7 7-7 7"/>
                    </svg>
                    <span style="color: #ef4444; font-size: 0.85em; letter-spacing: 1px;">CONTROLLING</span>
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#ef4444" stroke-width="2">
                        <path d="M5 12h14M12 5l7 7-7 7"/>
                    </svg>
                    <span><?= htmlspecialchars($deviceName, ENT_QUOTES, 'UTF-8') ?></span>
                </div>
                <div class="device-meta">
                    Full Control <span style="color:#d1d5db;">|</span>
                    <span id="headerDeviceIP"><?= htmlspecialchars($device['ip_address'] ?? '127.0.0.1') ?></span> <span
                        style="color:#d1d5db;">|</span>
                    <span class="dot" id="headerConnDot"></span> <span class="dot-text"
                        id="headerConnText">Connecting...</span>
                </div>
            </div>
        </div>

        <div class="header-right">
            <div class="action-group">
                <button class="btn" id="chatBtn">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                        <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"></path>
                    </svg>
                    Chat
                </button>
                <button class="btn" id="audioBtn" type="button">Listen Audio</button>
                <button class="btn" id="micBtn" type="button">Mic Off</button>
                <button class="btn" id="fileBtn" type="button" onclick="showFileTransferChoice()">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                        <path d="M13 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z"></path>
                        <polyline points="13 2 13 9 20 9"></polyline>
                    </svg>
                    File Transfer
                </button>
                <button class="btn" id="settingsBtn"
                    onclick="document.getElementById('sessionPanel').classList.toggle('open')">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                        <circle cx="12" cy="12" r="3"></circle>
                        <path
                            d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z">
                        </path>
                    </svg>
                    Settings
                </button>
            </div>
            <a href="#" class="btn-end" id="endSessionBtn" onclick="endSessionAndRedirect(event)">
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3">
                    <rect x="3" y="3" width="18" height="18" rx="2" ry="2"></rect>
                </svg>
                End Session
            </a>
            <?php if (empty($_GET['integrated'])): ?>
            <div class="window-controls">
                <span>—</span>
                <span>□</span>
                <span>×</span>
            </div>
            <?php endif; ?>
        </div>
    </div>
    <?php endif; ?>

    <div class="stream-container" id="stream-box">
        <div id="hud" class="hud-badge">CONNECTING...</div>

        <div id="dropOverlay" class="drop-overlay">
            📁 Drop files here to send to Remote Host<br>
            <small style="font-size:0.9rem;">Files will be placed in RemoteDrop/</small>
        </div>

        <canvas id="remoteCanvas" tabindex="0"></canvas>

        <?php if ($layer === 'B'): ?>
        <div class="floating-controls" id="floatingControls">
            <div class="toolbar-drag-handle" id="toolbarHandle" title="Drag to move">
                <svg width="12" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <circle cx="8" cy="4" r="1.5"></circle>
                    <circle cx="8" cy="12" r="1.5"></circle>
                    <circle cx="8" cy="20" r="1.5"></circle>
                    <circle cx="16" cy="4" r="1.5"></circle>
                    <circle cx="16" cy="12" r="1.5"></circle>
                    <circle cx="16" cy="20" r="1.5"></circle>
                </svg>
            </div>
            
            <button class="floating-btn" id="fullscreenBtn" onclick="toggleFullscreen()">
                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <path
                        d="M8 3H5a2 2 0 0 0-2 2v3m18 0V5a2 2 0 0 0-2-2h-3m0 18h3a2 2 0 0 0 2-2v-3M3 16v3a2 2 0 0 0 2 2h3">
                    </path>
                </svg>
                Fullscreen
            </button>
            <button class="floating-btn" onclick="document.getElementById('sessionPanel').classList.toggle('open')">
                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <circle cx="12" cy="12" r="1"></circle>
                    <circle cx="12" cy="5" r="1"></circle>
                    <circle cx="12" cy="19" r="1"></circle>
                </svg>
                Options
            </button>
            <button class="floating-btn" style="color: var(--primary);" onclick="endSessionAndRedirect(event)">
                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <rect x="3" y="3" width="18" height="18" rx="2" ry="2"></rect>
                </svg>
                End Session
            </button>
        </div>
        
        <script>
            // Drag logic for floating controls
            const floatingControls = document.getElementById('floatingControls');
            const toolbarHandle = document.getElementById('toolbarHandle');
            
            if (floatingControls && toolbarHandle) {
                let isDragging = false;
                let startX, startY, initialX, initialY;

                toolbarHandle.addEventListener('mousedown', (e) => {
                    isDragging = true;
                    startX = e.clientX;
                    startY = e.clientY;
                    const rect = floatingControls.getBoundingClientRect();
                    initialX = rect.left;
                    initialY = rect.top;
                    
                    // Switch to fixed positioning for precise dragging relative to viewport
                    floatingControls.style.position = 'fixed';
                    floatingControls.style.bottom = 'auto';
                    floatingControls.style.right = 'auto';
                    floatingControls.style.left = initialX + 'px';
                    floatingControls.style.top = initialY + 'px';
                    floatingControls.style.transition = 'none'; // Disable transition during drag
                    
                    e.preventDefault();
                });

                window.addEventListener('mousemove', (e) => {
                    if (!isDragging) return;
                    
                    const dx = e.clientX - startX;
                    const dy = e.clientY - startY;
                    
                    // Calculate bounds
                    let newLeft = initialX + dx;
                    let newTop = initialY + dy;
                    
                    // Keep within window bounds
                    const maxX = window.innerWidth - floatingControls.offsetWidth;
                    const maxY = window.innerHeight - floatingControls.offsetHeight;
                    
                    newLeft = Math.max(0, Math.min(newLeft, maxX));
                    newTop = Math.max(0, Math.min(newTop, maxY));
                    
                    floatingControls.style.left = newLeft + 'px';
                    floatingControls.style.top = newTop + 'px';
                });

                window.addEventListener('mouseup', () => {
                    if (isDragging) {
                        isDragging = false;
                        floatingControls.style.transition = ''; // Restore transition
                    }
                });
                
                // Prevent drag from propagating to canvas
                floatingControls.addEventListener('mousedown', (e) => e.stopPropagation());
            }
        </script>
        <?php endif; ?>

        <!-- Session / Settings Panel -->
        <div class="session-panel" id="sessionPanel">
            <div class="panel-tabs">
                <div class="panel-tab active" id="tabSession" onclick="switchPanelTab('session')">Session</div>
                <div class="panel-tab" id="tabActivity" onclick="switchPanelTab('activity')">Activity</div>
                <div class="panel-tab" id="tabTransfers" onclick="switchPanelTab('transfers')">Transfers</div>
            </div>
            <div class="panel-content" id="panelContentSession">
                <div class="info-row">
                    <span class="info-label">Connection Time</span>
                    <span class="info-val" id="connTimeVal">00:00:00</span>
                </div>
                <div class="info-row">
                    <span class="info-label">Connected Since</span>
                    <span class="info-val">Today, <?= date('h:i A') ?></span>
                </div>
                <div class="info-row">
                    <span class="info-label">Connection Mode</span>
                    <span class="info-val">Full control</span>
                </div>
                <div class="info-row">
                    <span class="info-label">Quality</span>
                    <span class="info-val">High (Auto)</span>
                </div>
                <div class="info-row">
                    <span class="info-label">Encryption</span>
                    <span class="info-val highlight">AES-256</span>
                </div>

                <div class="panel-section-title">Remote Device</div>

                <div class="info-row">
                    <span class="info-label">Device Name</span>
                    <span class="info-val"><?= htmlspecialchars($deviceName, ENT_QUOTES, 'UTF-8') ?></span>
                </div>
                <div class="info-row">
                    <span class="info-label">Remote ID</span>
                    <span class="info-val red"><?= htmlspecialchars($sessionCode) ?></span>
                </div>
                <div class="info-row">
                    <span class="info-label">IP Address</span>
                    <span class="info-val"><?= htmlspecialchars($device['ip_address'] ?? 'Unknown') ?></span>
                </div>
                <div class="info-row">
                    <span class="info-label">Operating System</span>
                    <span class="info-val"><?= htmlspecialchars($device['os_type'] ?? 'Windows') ?></span>
                </div>
                <div class="info-row">
                    <span class="info-label">Resolution</span>
                    <span class="info-val" id="resDisplay">Waiting...</span>
                </div>
                <div class="info-row">
                    <span class="info-label">User</span>
                    <span class="info-val"><?= htmlspecialchars($device['username'] ?? 'User') ?></span>
                </div>
            </div>
            <div class="panel-content" id="panelContentActivity" style="display:none; height: 350px; overflow-y: auto;">
                <div id="activityFeed" style="display:flex; flex-direction:column; gap:8px; font-size:0.75rem;">
                </div>
            </div>

            <div class="panel-content" id="panelContentTransfers"
                style="display:none; height: 350px; overflow-y: auto;">
                <div style="margin-bottom: 15px;">
                    <button onclick="showFileTransferChoice()"
                        style="width: 100%; padding: 8px; background: #007bff; color: white; border: none; border-radius: 4px; cursor: pointer;">
                        Send File to Host
                    </button>
                </div>
                <div id="transferFeed" style="display:flex; flex-direction:column; gap:8px; font-size:0.75rem;">
                </div>
            </div>

            <!-- Hidden File Input -->
            <input type="file" id="fileUploadInput" multiple style="display: none;" onchange="handleFileInput(event)">
            <input type="file" id="folderUploadInput" webkitdirectory directory multiple style="display: none;" onchange="handleFileInput(event)">

        </div>

        <div id="fileTransferChoiceDialog" role="dialog" aria-modal="true" aria-labelledby="fileTransferChoiceTitle">
            <div class="file-transfer-choice-card">
                <h2 id="fileTransferChoiceTitle">Choose what to send</h2>
                <div class="file-transfer-choice-actions">
                    <button type="button" class="btn" onclick="chooseTransferSource('file')">Select File</button>
                    <button type="button" class="btn btn-primary" onclick="chooseTransferSource('folder')">Select Folder</button>
                    <button type="button" class="btn" onclick="closeFileTransferChoice()">Cancel</button>
                </div>
            </div>
        </div>

        <div id="folderDestinationDialog" role="dialog" aria-modal="true" aria-labelledby="folderDestinationTitle">
            <div class="file-transfer-choice-card">
                <h2 id="folderDestinationTitle">Choose a folder for the received files</h2>
                <p>Folder paths will be created only inside the destination you choose.</p>
                <div class="file-transfer-choice-actions">
                    <button type="button" class="btn btn-primary" onclick="selectFolderDestination()">Choose Destination</button>
                    <button type="button" class="btn" onclick="cancelFolderDestination()">Cancel</button>
                </div>
            </div>
        </div>

        <!-- Chat Panel -->
        <div class="chat-panel" id="chatPanel">
            <div class="chat-header">
                <span>Session Chat</span>
                <span class="chat-close" id="chatClose">&times;</span>
            </div>
            <div class="chat-messages" id="chatMessages"></div>
            <div class="chat-typing" id="chatTypingIndicator" aria-live="polite">Remote user is typing…</div>
            <div class="chat-input">
                <input type="text" id="chatInput" placeholder="Type a message..." autocomplete="off">
                <button type="button" id="sendChatBtn">Send</button>
            </div>
        </div>
    </div>

    <div id="reversePermissionDialog" role="alertdialog" aria-modal="false" aria-labelledby="reversePermissionTitle"
        style="display:none;">
        <div class="reverse-permission-card">
            <h2 id="reversePermissionTitle">Reverse control requested</h2>
            <p>Allow the remote device to take control of this computer?</p>
            <div class="reverse-permission-actions">
                <button type="button" id="reverseRejectBtn" class="btn">Decline</button>
                <button type="button" id="reverseAcceptBtn" class="btn btn-primary">Accept</button>
            </div>
        </div>
    </div>
    <div id="sessionEndedDialog" role="dialog" aria-modal="true" aria-labelledby="sessionEndedTitle"
        style="display:none;position:fixed;inset:0;z-index:1100;background:rgba(15,23,42,.45);align-items:center;justify-content:center;">
        <div style="width:min(380px,calc(100vw - 32px));padding:24px;background:#fff;border-radius:12px;box-shadow:0 20px 50px rgba(15,23,42,.25);color:#111827;">
            <h2 id="sessionEndedTitle" style="margin:0 0 10px;font-size:1.1rem;">Session Ended</h2>
            <p style="margin:0 0 20px;color:#4b5563;">This session has ended.</p>
            <div style="display:flex;justify-content:flex-end;">
                <button type="button" class="btn btn-primary" onclick="dismissSessionEnded()">OK</button>
            </div>
        </div>
    </div>


    <script>

        /* ============================================================
           CONFIG
        ============================================================ */

        console.log("[SECURITY] location:", location.href);
        console.log("[SECURITY] origin:", location.origin);
        console.log("[SECURITY] secure:", window.isSecureContext);
        console.log("[SECURITY] VideoDecoder:", ("VideoDecoder" in window));

        const DEVICE_ID =
            <?= json_encode(
                $device['system_id'] ?: $device['device_uid'],
                JSON_UNESCAPED_SLASHES
            ) ?>;
        const REQUESTER_SYSTEM_ID =
            <?= json_encode($_GET['requester_system_id'] ?? '', JSON_UNESCAPED_SLASHES) ?>;

        const WS_URL = <?= json_encode($relayWsUrl) ?>;


        /* ============================================================
           STATE
        ============================================================ */

        let ws = null;
        let wsRxBuffer = new Uint8Array(0);
        let wsRxProcessing = false;
        let directConnection = false;
        let directFallbackAttempted = false;
        let websocketAuthenticated = false;
        let reverseTransitionPending = false;
        let sessionEndedDialogShown = false;
        let sessionWasEstablished = false;
        let webrtcPeerConnection = null;
        let webrtcDataChannel = null;
        let activeTransport = "WS"; // "WS" or "WEBRTC"

        let isStreaming = false;

        let canvas = null;

        let ctx = null;

        let animationFrameId = null;

        let latestImage = null;

        let renderWidth = 0;

        let renderHeight = 0;

        let videoFrameCount = 0;

        let videoDecodeBusy = false;

        let videoDecoder = null;
        const MAX_DECODER_QUEUE = 12;
        const MAX_RX_BUFFER_BYTES = 12 * 1024 * 1024;
        const videoTimingByTimestamp = new Map();
        let lastLatencyLogAt = 0;
        let lastRenderedLatencyLogAt = 0;


        /* AUDIO */

        let audioCtx = null;

        let nextAudioTime = 0;

        let isAudioEnabled = false;
        let audioReceiverDiagnosticCount = 0;
        let audioSenderDiagnosticCount = 0;
        let activeAudioNodes = [];
        let microphoneStream = null;
        let microphoneContext = null;
        let microphoneSource = null;
        let microphoneProcessor = null;


        /* RECORDING */

        let mediaRecorder = null;

        let recordedChunks = [];


        /* ============================================================
           DOM
        ============================================================ */

        const streamBox =
            document.getElementById("stream-box");

        const advToolbar =
            document.getElementById("advToolbar");

        const hud =
            document.getElementById("hud");

        const audioBtn =
            document.getElementById("audioBtn");

        const micBtn =
            document.getElementById("micBtn");

        const chatBtn =
            document.getElementById("chatBtn");

        const recordBtn =
            document.getElementById("recordBtn");

        const monitorSelect =
            document.getElementById("monitorSelect");

        const chatPanel =
            document.getElementById("chatPanel");

        const chatClose =
            document.getElementById("chatClose");

        const chatInput =
            document.getElementById("chatInput");

        const sendChatBtn =
            document.getElementById("sendChatBtn");

        const chatMessages =
            document.getElementById("chatMessages");
        const chatHistory = [];
        let nextChatMessageId = 0;
        let chatPacketDiagnosticLogged = false;
        let localChatTyping = false;
        let remoteChatTyping = false;
        let chatTypingIdleTimer = null;

        const dropOverlay =
            document.getElementById("dropOverlay");


        /* ============================================================
           HUD
        ============================================================ */

        function setHud(text) {

            hud.textContent = text;

        }


        /* ============================================================
           SOCKET
        ============================================================ */

        function isSocketOpen() {

            return (
                ws &&
                ws.readyState === WebSocket.OPEN
            );

        }


        /* ============================================================
           IMAGE CLEANUP
        ============================================================ */

        function safeCloseImage() {

            if (!latestImage) {

                return;

            }

            try {

                if (
                    typeof latestImage.close ===
                    "function"
                ) {

                    latestImage.close();

                }

            } catch (e) {

                console.warn(
                    "[VIDEO] Image cleanup failed",
                    e
                );

            }

            latestImage = null;

        }


        /* ============================================================
           CHAT
        ============================================================ */

        function toggleChat() {

            chatPanel.classList.toggle("open");

            if (
                chatPanel.classList.contains("open")
            ) {

                renderChatHistory();
                renderRemoteChatTyping();
                setTimeout(
                    () => chatInput.focus(),
                    100
                );
            } else {
                stopLocalChatTyping();
            }

        }

        function renderChatMessage(message) {
            const div = document.createElement("div");
            div.className = "msg " + message.type;
            div.dataset.messageId = String(message.id);
            div.textContent = message.text;
            chatMessages.appendChild(div);
            if (message.type === "sent") {
                logAChatDiagnostic("A_CHAT_RENDER_MESSAGE", {
                    message_id: String(message.id),
                    sender: "local",
                    dom_count: String(chatMessages.children.length),
                    rendered: String(div.isConnected)
                });
            }
        }

        function logAChatDiagnostic(event, details = {}) {
            console.info(`[A CHAT DIAG] ${event}`, details);
            if (typeof sessionDebug === "function") {
                sessionDebug(event, details);
            }
        }

        function renderChatHistory() {
            chatMessages.replaceChildren();
            for (const message of chatHistory) {
                renderChatMessage(message);
            }
            chatMessages.scrollTop = chatMessages.scrollHeight;
        }

        function appendMessage(type, text) {
            const message = {
                id: ++nextChatMessageId,
                type,
                text
            };
            const isLocalSend = type === "sent";
            if (isLocalSend) {
                logAChatDiagnostic("A_CHAT_LOCAL_APPEND_START", {
                    message_id: String(message.id),
                    sender: "local",
                    state_count_before: String(chatHistory.length),
                    dom_count_before: String(chatMessages.children.length),
                    panel_open: String(chatPanel.classList.contains("open"))
                });
            }
            try {
                chatHistory.push(message);
                if (chatHistory.length > 500) {
                    chatHistory.shift();
                    chatMessages.firstElementChild?.remove();
                }
                if (chatPanel.classList.contains("open")) {
                    renderChatMessage(message);
                    chatMessages.scrollTop = chatMessages.scrollHeight;
                }
                if (isLocalSend) {
                    const renderedMessage = chatMessages.querySelector(
                        `[data-message-id="${message.id}"]`
                    );
                    logAChatDiagnostic("A_CHAT_LOCAL_APPEND_SUCCESS", {
                        message_id: String(message.id),
                        sender: "local",
                        state_count_after: String(chatHistory.length),
                        dom_count_after: String(chatMessages.children.length),
                        rendered: String(Boolean(renderedMessage)),
                        panel_open: String(chatPanel.classList.contains("open"))
                    });
                }
            } catch (error) {
                if (isLocalSend) {
                    logAChatDiagnostic("A_CHAT_LOCAL_APPEND_FAILED", {
                        message_id: String(message.id),
                        sender: "local",
                        state_count_after: String(chatHistory.length),
                        dom_count_after: String(chatMessages.children.length),
                        error: error.name || "Error"
                    });
                }
                throw error;
            }
        }

        window.addEventListener("chat_message_received", event => {
            console.info(`[CHAT UI] direction=B->A event_received=true panel_open=${chatPanel.classList.contains("open")} characters=${event.detail.text.length}`);
            chatPanel.classList.add("open");
            appendMessage("recv", event.detail.text);
        });

        function renderRemoteChatTyping() {
            document.getElementById("chatTypingIndicator")
                ?.classList.toggle("visible", remoteChatTyping && chatPanel.classList.contains("open"));
        }

        function sendChatTyping(typing) {
            if (localChatTyping === typing) return;
            if (!isSocketOpen()) {
                localChatTyping = false;
                return;
            }
            try {
                ws.send(new Uint8Array([18, typing ? 1 : 0]));
                localChatTyping = typing;
            } catch (error) {
                localChatTyping = false;
                console.error("[CHAT TYPING] Send failed:", error);
            }
        }

        function stopLocalChatTyping() {
            if (chatTypingIdleTimer) {
                clearTimeout(chatTypingIdleTimer);
                chatTypingIdleTimer = null;
            }
            sendChatTyping(false);
        }

        function updateLocalChatTyping() {
            if (!chatInput.value.trim()) {
                stopLocalChatTyping();
                return;
            }
            sendChatTyping(true);
            if (chatTypingIdleTimer) clearTimeout(chatTypingIdleTimer);
            chatTypingIdleTimer = setTimeout(stopLocalChatTyping, 900);
        }

        function setRemoteChatTyping(typing) {
            remoteChatTyping = typing;
            renderRemoteChatTyping();
        }


        function sendChat() {

            const msg =
                chatInput.value.trim();

            logAChatDiagnostic("A_CHAT_SEND_START", {
                message_length: String(msg.length),
                panel_open: String(chatPanel.classList.contains("open"))
            });

            if (!msg) {

                return;

            }

            stopLocalChatTyping();

            if (!isSocketOpen()) {

                appendMessage(
                    "recv",
                    "Not connected to remote host."
                );

                return;

            }

            const msgBytes =
                new TextEncoder().encode(msg);
            logAChatDiagnostic("A_CHAT_SEND_TEXT_CREATED", {
                message_length: String(msgBytes.length)
            });

            if (msgBytes.length > 65535) {

                alert("Message is too long.");

                return;

            }

            const pkt =
                new Uint8Array(
                    4 + msgBytes.length
                );

            pkt[0] = 16;
            pkt[1] = 0;
            pkt[2] =
                (msgBytes.length >> 8) & 0xff;

            pkt[3] =
                msgBytes.length & 0xff;

            pkt.set(
                msgBytes,
                4
            );

            try {
                console.info(
                    `[CHAT SEND START] time=${Date.now()} bytes=${msgBytes.length} ` +
                    `videoPackets=${streamStats.received_packets} decodedFrames=${browserPerf.decodedFrames} ` +
                    `renderedFrames=${browserPerf.renderedFrames} lastVideoAt=${videoDiagnostics.lastPacketAt || "none"} ` +
                    `lastDecodedAt=${videoDiagnostics.lastDecodedAt || "none"} ` +
                    `lastRenderedAt=${videoDiagnostics.lastRenderedAt || "none"} ` +
                    `receiveLoopAlive=${ws.readyState === WebSocket.OPEN} ` +
                    `parserBufferBytes=${wsRxBuffer.length}`
                );
                try {
                    ws.send(pkt);
                    logAChatDiagnostic("A_CHAT_WS_SEND_SUCCESS", {
                        packet_type: "16",
                        message_length: String(msgBytes.length)
                    });
                } catch (error) {
                    logAChatDiagnostic("A_CHAT_WS_SEND_FAILED", {
                        packet_type: "16",
                        message_length: String(msgBytes.length),
                        error: error.name || "Error"
                    });
                    throw error;
                }
                chatVideoSnapshot = {
                    sentAt: Date.now(),
                    packetCountBefore: streamStats.received_packets
                };
                console.info(`[CHAT TYPE 16 SENT] bytes=${pkt.length} payloadBytes=${msgBytes.length}`);

                chatPanel.classList.add("open");
                appendMessage(
                    "sent",
                    msg
                );

                chatInput.value = "";

            } catch (error) {
                logAChatDiagnostic("A_CHAT_SEND_FAILED", {
                    error: error.name || "Error"
                });

                console.error(
                    "[CHAT] Send failed:",
                    error
                );

            }

        }


        /* ============================================================
           AUDIO
        ============================================================ */

        async function toggleAudio() {
            isAudioEnabled = !isAudioEnabled;
            if (isAudioEnabled) {
                if (audioBtn) {
                    audioBtn.textContent = "🔊 Mute Audio";
                    audioBtn.classList.remove("btn-secondary");
                    audioBtn.classList.add("btn-warning");
                }
                try {
                    if (!audioCtx) {
                        audioCtx = new (window.AudioContext || window.webkitAudioContext)({
                            latencyHint: "interactive",
                            sampleRate: 48000
                        });
                    }
                    if (audioCtx.state === "suspended") {
                        await audioCtx.resume();
                    }
                } catch (error) {
                    console.error("[AUDIO] Init error:", error);
                }
            } else {
                if (audioBtn) {
                    audioBtn.textContent = "🔈 Listen Audio";
                    audioBtn.classList.remove("btn-warning");
                    audioBtn.classList.add("btn-secondary");
                }
                nextAudioTime = 0;
            }
        }

        async function toggleMicrophone() {
            if (microphoneStream) {
                stopMicrophone();
                return;
            }
            if (!isSocketOpen()) {
                console.error("[MIC] Cannot enable microphone without an active session.");
                return;
            }
            try {
                // Low-latency voice capture with automatic echo cancellation, noise suppression, and gain control
                microphoneStream = await navigator.mediaDevices.getUserMedia({
                    audio: {
                        channelCount: 1,
                        sampleRate: 48000,
                        echoCancellation: true,
                        noiseSuppression: true,
                        autoGainControl: true
                    },
                    video: false
                });

                microphoneContext = new (window.AudioContext || window.webkitAudioContext)({
                    latencyHint: "interactive",
                    sampleRate: 48000
                });
                await microphoneContext.resume();

                microphoneSource = microphoneContext.createMediaStreamSource(microphoneStream);
                // 1024 buffer size (~21.3ms @ 48kHz) optimizes voice latency without CPU thrashing
                microphoneProcessor = microphoneContext.createScriptProcessor(1024, 1, 1);
                const silentOutput = microphoneContext.createGain();
                silentOutput.gain.value = 0;

                microphoneProcessor.onaudioprocess = event => {
                    if (!microphoneStream || !isSocketOpen()) return;
                    const tCapture = performance.now();
                    const input = event.inputBuffer.getChannelData(0);
                    const bytes = input.length * 4;
                    const packet = new Uint8Array(11 + bytes);
                    const view = new DataView(packet.buffer);

                    packet[0] = 17; // TYPE 17 = AUDIO
                    view.setUint32(1, bytes, false);
                    view.setUint32(5, microphoneContext.sampleRate, false);
                    view.setUint16(9, 1, false);

                    // High performance direct Float32 LE buffer copy (eliminating 1024-iteration element loop)
                    const floatBytes = new Uint8Array(input.buffer, input.byteOffset, input.byteLength);
                    packet.set(floatBytes, 11);

                    const tCreated = performance.now();
                    ws.send(packet);
                    const tSent = performance.now();

                    audioSenderDiagnosticCount = (audioSenderDiagnosticCount || 0) + 1;
                    if (audioSenderDiagnosticCount % 100 === 1) {
                        console.log(`[A-AUDIO] capture: ${tCapture.toFixed(2)}ms | packet created: +${(tCreated - tCapture).toFixed(2)}ms | packet sent: +${(tSent - tCreated).toFixed(2)}ms | chunk_samples: ${input.length} | sampleRate: ${microphoneContext.sampleRate}Hz`);
                    }
                };

                microphoneSource.connect(microphoneProcessor);
                microphoneProcessor.connect(silentOutput);
                silentOutput.connect(microphoneContext.destination);

                if (micBtn) micBtn.textContent = "Mic On";
                console.log("[MIC] TYPE 17 microphone transmission enabled (1024-sample low-latency stream).");
            } catch (error) {
                stopMicrophone();
                console.error("[MIC] Failed to enable microphone:", error);
            }
        }

        function stopMicrophone() {
            if (microphoneProcessor) {
                microphoneProcessor.onaudioprocess = null;
                microphoneProcessor.disconnect();
                microphoneProcessor = null;
            }
            if (microphoneSource) {
                microphoneSource.disconnect();
                microphoneSource = null;
            }
            if (microphoneStream) {
                microphoneStream.getTracks().forEach(track => track.stop());
                microphoneStream = null;
            }
            if (microphoneContext) {
                void microphoneContext.close();
                microphoneContext = null;
            }
            if (micBtn) micBtn.textContent = "Mic Off";
            console.log("[MIC] Microphone stopped.");
        }

        let reverseRequestPending = false;

        function receiveReverseRequest() {
            if (reverseRequestPending || !isSocketOpen() || !websocketAuthenticated) return;
            reverseRequestPending = true;
            document.getElementById("reversePermissionDialog").style.display = "flex";
        }

        function answerReverseRequest(accepted) {
            if (!reverseRequestPending || !isSocketOpen()) return;
            try {
                ws.send(new Uint8Array([31, accepted ? 1 : 0]));
            } catch (error) {
                console.error("[REVERSE] Failed to send permission decision:", error);
                return;
            }
            reverseRequestPending = false;
            document.getElementById("reversePermissionDialog").style.display = "none";
            if (accepted) {
                reverseTransitionPending = true;
                window.parent.postMessage({
                    type: "reverse_remote_approved",
                    requester_system_id: DEVICE_ID
                }, "*");
            }
        }

        function showSessionEnded() {
            if (sessionEndedDialogShown) return;
            sessionEndedDialogShown = true;
            document.getElementById("sessionEndedDialog").style.display = "flex";
        }

        function dismissSessionEnded() {
            const params = new URLSearchParams(window.location.search);
            if (params.get("integrated") === "1") {
                window.parent.postMessage("end_integrated_session", "*");
                return;
            }
            window.location.href = "../dashboard.php";
        }

        /* ============================================================
           MONITOR
        ============================================================ */

        function switchMonitor(index) {

            if (!isSocketOpen()) {

                return;

            }

            const monitorIndex =
                parseInt(index, 10);

            if (
                Number.isNaN(monitorIndex) ||
                monitorIndex < 0 ||
                monitorIndex > 255
            ) {

                return;

            }

            const pkt =
                new Uint8Array(9);

            pkt[0] = 7;

            pkt[1] =
                monitorIndex;

            ws.send(pkt);

        }


        /* ============================================================
           RECORDING
        ============================================================ */

        function toggleRecording() {

            if (!canvas) {

                alert(
                    "Start the remote stream first."
                );

                return;

            }

            if (
                mediaRecorder &&
                mediaRecorder.state ===
                "recording"
            ) {

                mediaRecorder.stop();

                return;

            }

            if (
                !window.MediaRecorder ||
                !canvas.captureStream
            ) {

                alert(
                    "Session recording is not supported."
                );

                return;

            }

            const stream =
                canvas.captureStream(30);

            let mimeType =
                "video/webm;codecs=vp9";

            if (
                !MediaRecorder.isTypeSupported(
                    mimeType
                )
            ) {

                mimeType =
                    "video/webm;codecs=vp8";

            }

            if (
                !MediaRecorder.isTypeSupported(
                    mimeType
                )
            ) {

                mimeType =
                    "video/webm";

            }

            try {

                mediaRecorder =
                    new MediaRecorder(
                        stream,
                        {
                            mimeType
                        }
                    );

            } catch (error) {

                console.error(
                    "[RECORDER]",
                    error
                );

                alert(
                    "Unable to start recording."
                );

                return;

            }

            recordedChunks = [];

            mediaRecorder.ondataavailable =
                event => {

                    if (
                        event.data &&
                        event.data.size > 0
                    ) {

                        recordedChunks.push(
                            event.data
                        );

                    }

                };

            mediaRecorder.onstop =
                () => {

                    if (
                        recordedChunks.length === 0
                    ) {

                        resetRecordButton();

                        return;

                    }

                    const blob =
                        new Blob(
                            recordedChunks,
                            {
                                type: mimeType
                            }
                        );

                    const url =
                        URL.createObjectURL(blob);

                    const a =
                        document.createElement("a");

                    a.href = url;

                    a.download =
                        "session-" +
                        DEVICE_ID +
                        "-" +
                        Date.now() +
                        ".webm";

                    document.body.appendChild(a);

                    a.click();

                    a.remove();

                    setTimeout(
                        () =>
                            URL.revokeObjectURL(url),
                        1000
                    );

                    recordedChunks = [];

                    resetRecordButton();

                };

            mediaRecorder.start(1000);

            recordBtn.textContent =
                "⏹ Stop Recording";

            recordBtn.classList.remove(
                "btn-secondary"
            );

            recordBtn.classList.add(
                "btn-danger"
            );

        }


        function resetRecordButton() {

            recordBtn.textContent =
                "🔴 Record Session";

            recordBtn.classList.remove(
                "btn-danger"
            );

            recordBtn.classList.add(
                "btn-secondary"
            );

        }


        /* ============================================================
           RENDER LOOP
        ============================================================ */

        function startRenderLoop() {

            stopRenderLoop();

            function render() {

                if (!isStreaming) {

                    return;

                }

                if (
                    latestImage &&
                    ctx &&
                    renderWidth > 0 &&
                    renderHeight > 0
                ) {

                    if (
                        canvas.width !==
                        renderWidth ||
                        canvas.height !==
                        renderHeight
                    ) {

                        canvas.width =
                            renderWidth;

                        canvas.height =
                            renderHeight;

                    }

                    try {

                        ctx.drawImage(
                            latestImage,
                            0,
                            0,
                            renderWidth,
                            renderHeight
                        );

                    } catch (error) {

                        console.warn(
                            "[VIDEO] drawImage error:",
                            error
                        );

                    }

                }

                animationFrameId =
                    requestAnimationFrame(render);

            }

            animationFrameId =
                requestAnimationFrame(render);

        }


        function stopRenderLoop() {

            if (animationFrameId) {

                cancelAnimationFrame(
                    animationFrameId
                );

                animationFrameId = null;

            }

        }


        /* ============================================================
           VIDEO PACKET PARSER (FIXED FOR RGB vs RGBA)
        
           Protocol:
           [1 byte TYPE] = 15
           [4 bytes WIDTH (u32 BE)]
           [4 bytes HEIGHT (u32 BE)]
           [4 bytes H264_SIZE (u32 BE)]
           [H264 DATA...]
        
/* ============================================================
   STREAM STATE & METRICS
============================================================ */

        const DecoderState = {
            UNCONFIGURED: "UNCONFIGURED",
            CONFIGURING: "CONFIGURING",
            WAITING_FOR_KEYFRAME: "WAITING_FOR_KEYFRAME",
            CONFIGURED: "CONFIGURED",
            ERROR: "ERROR",
            CLOSED: "CLOSED"
        };

        let decoderState = DecoderState.UNCONFIGURED;
        let currentStreamState = "CONNECTING";
        let streamStats = {
            received_packets: 0,
            received_bytes: 0,
            received_sps: 0,
            received_pps: 0,
            received_idr: 0,
            received_non_idr: 0
        };
        let browserPerf = {
            rxPackets: 0,
            decodedFrames: 0,
            renderedFrames: 0,
            receiveWindow: 0,
            decodeWindow: 0,
            renderWindow: 0,
            captureRenderSamples: [],
            receiveDecodeSamples: [],
            decodeOutputSamples: [],
            outputRenderSamples: [],
            queueSum: 0,
            queueSamples: 0,
            maxQueue: 0,
            staleFramesDiscarded: 0,
            idrRecoveryCount: 0,
            maxWsRxBuffer: 0,
            lastReport: performance.now()
        };
        const videoDiagnostics = {
            lastPacketAt: 0,
            lastPacketType: null,
            lastPacketLength: 0,
            lastIdrAt: 0,
            lastDecodedAt: 0,
            lastRenderedAt: 0,
            lastDecoderError: null
        };
        let lastKeyDeltaLogAt = 0;

        function recordVideoMetrics(now) {
            if (now - browserPerf.lastReport < 1000) {
                return;
            }

            const average = (values) => values.length
                ? Math.round(values.reduce((sum, value) => sum + value, 0) / values.length)
                : 0;
            const decoderQueue = videoDecoder ? videoDecoder.decodeQueueSize : 0;
            console.log(
                `[VIDEO METRICS] capture->render avg=${average(browserPerf.captureRenderSamples)}ms ` +
                `receive->decode avg=${average(browserPerf.receiveDecodeSamples)}ms ` +
                `decode->output avg=${average(browserPerf.decodeOutputSamples)}ms ` +
                `output->render avg=${average(browserPerf.outputRenderSamples)}ms ` +
                `receiveFPS=${browserPerf.receiveWindow} decodeFPS=${browserPerf.decodeWindow} ` +
                `renderFPS=${browserPerf.renderWindow} avgDecodeQueue=${browserPerf.queueSamples ? Math.round(browserPerf.queueSum / browserPerf.queueSamples) : 0} ` +
                `maxDecodeQueue=${browserPerf.maxQueue} wsRxBufferMax=${browserPerf.maxWsRxBuffer} ` +
                `staleFrames=${browserPerf.staleFramesDiscarded} idrRecovery=${browserPerf.idrRecoveryCount} ` +
                `currentDecodeQueue=${decoderQueue} ` +
                `lastPacketAgeMs=${videoDiagnostics.lastPacketAt ? Date.now() - videoDiagnostics.lastPacketAt : "n/a"} ` +
                `lastIdrAgeMs=${videoDiagnostics.lastIdrAt ? Date.now() - videoDiagnostics.lastIdrAt : "n/a"} ` +
                `lastDecodedAgeMs=${videoDiagnostics.lastDecodedAt ? Date.now() - videoDiagnostics.lastDecodedAt : "n/a"} ` +
                `lastRenderedAgeMs=${videoDiagnostics.lastRenderedAt ? Date.now() - videoDiagnostics.lastRenderedAt : "n/a"} ` +
                `chatPending=${chatVideoSnapshot !== null}`
            );
            browserPerf.receiveWindow = 0;
            browserPerf.decodeWindow = 0;
            browserPerf.renderWindow = 0;
            browserPerf.captureRenderSamples = [];
            browserPerf.receiveDecodeSamples = [];
            browserPerf.decodeOutputSamples = [];
            browserPerf.outputRenderSamples = [];
            browserPerf.queueSum = 0;
            browserPerf.queueSamples = 0;
            browserPerf.maxQueue = 0;
            browserPerf.lastReport = now;
        }

        function addActivityLog(message) {
            const feed = document.getElementById("activityFeed");
            if (!feed) return;
            const timeStr = new Date().toLocaleTimeString();
            const entry = document.createElement("div");
            entry.innerHTML = `<strong>${timeStr}</strong>: ${message}`;
            feed.appendChild(entry);
            feed.scrollTop = feed.scrollHeight;
        }

        function setStreamState(newState) {
            currentStreamState = newState;
            console.log("[STREAM STATE]", newState);
            if (newState === "WAITING_FOR_KEYFRAME") {
                console.warn(
                    `[VIDEO WAIT DIAGNOSTIC] reason=decoder_wait ` +
                    `lastPacketAt=${videoDiagnostics.lastPacketAt || "none"} ` +
                    `lastPacketType=${videoDiagnostics.lastPacketType ?? "none"} ` +
                    `lastPacketLength=${videoDiagnostics.lastPacketLength} ` +
                    `lastIdrAt=${videoDiagnostics.lastIdrAt || "none"} ` +
                    `lastDecodedAt=${videoDiagnostics.lastDecodedAt || "none"} ` +
                    `lastRenderedAt=${videoDiagnostics.lastRenderedAt || "none"} ` +
                    `receiveLoopAlive=${Boolean(ws && ws.readyState === WebSocket.OPEN)} ` +
                    `videoQueueSize=${videoDecoder ? videoDecoder.decodeQueueSize : 0} ` +
                    `decoderError=${videoDiagnostics.lastDecoderError || "none"} ` +
                    `rxPackets=${streamStats.received_packets} idrPackets=${streamStats.received_idr}`
                );
            }
            addActivityLog(`State changed: ${newState}`);

            const dot = document.getElementById("headerConnDot");
            const txt = document.getElementById("headerConnText");
            if (dot && txt) {
                if (newState.includes("ERROR") || newState === "DISCONNECTED") {
                    dot.style.background = "#ef4444";
                    txt.style.color = "#ef4444";
                    txt.textContent = "Disconnected";
                } else if (newState === "DISPLAYING" || newState === "STREAM_ACTIVE" || newState === "DECODING") {
                    dot.style.background = "#22c55e";
                    txt.style.color = "#22c55e";
                    txt.textContent = "Connected";
                } else {
                    dot.style.background = "#f59e0b";
                    txt.style.color = "#f59e0b";
                    txt.textContent = "Connecting...";
                }
            }

            switch (newState) {
                case "CONNECTING":
                    setHud("CONNECTING...");
                    break;
                case "AUTHENTICATING":
                    setHud("AUTHENTICATING...");
                    break;
                case "AUTHENTICATED":
                case "ONLINE":
                case "STREAM_REQUESTED":
                    setHud("ONLINE • REQUESTING STREAM");
                    break;
                case "STREAM_ACTIVE":
                    setHud("STREAM ACTIVE • WAITING FOR KEYFRAME");
                    break;
                case "WAITING_FOR_KEYFRAME":
                    setHud("WAITING FOR KEYFRAME (SPS/PPS/IDR)...");
                    break;
                case "DECODING":
                    setHud("DECODING VIDEO...");
                    break;
                case "DISPLAYING":
                    setHud(`LIVE  • ${renderWidth || 1920}×${renderHeight || 1080}`);
                    break;
                default:
                    setHud(newState);
                    break;
            }
        }

        let cachedSPS = null;
        let cachedPPS = null;
        let lastVideoNalInfo = null;
        let chatVideoSnapshot = null;

        function logDecodeErrorDetail(error) {
            console.error(
                "[BROWSER DECODE ERROR DETAIL]\n" +
                "error=" + (error && error.message ? error.message : error) + "\n" +
                "decoderState=" + decoderState + "\n" +
                "videoDecoderState=" + (videoDecoder ? videoDecoder.state : "none") + "\n" +
                "lastNAL=" + JSON.stringify(lastVideoNalInfo)
            );
        }

        function parseH264Nals(dataBytes) {
            let nalTypes = [];
            let hasSPS = false;
            let hasPPS = false;
            let hasIDR = false;
            let hasSEI = false;
            let hasAUD = false;
            let spsUnit = null;
            let ppsUnit = null;
            let i = 0;
            const len = dataBytes.length;

            while (i + 2 < len) {
                let scLen = 0;
                if (i + 3 < len && dataBytes[i] === 0 && dataBytes[i + 1] === 0 && dataBytes[i + 2] === 0 && dataBytes[i + 3] === 1) {
                    scLen = 4;
                } else if (dataBytes[i] === 0 && dataBytes[i + 1] === 0 && dataBytes[i + 2] === 1) {
                    scLen = 3;
                }

                if (scLen > 0) {
                    const nalStart = i + scLen;
                    let nextStart = len;
                    let j = nalStart;
                    while (j + 2 < len) {
                        if ((j + 3 < len && dataBytes[j] === 0 && dataBytes[j + 1] === 0 && dataBytes[j + 2] === 0 && dataBytes[j + 3] === 1) ||
                            (dataBytes[j] === 0 && dataBytes[j + 1] === 0 && dataBytes[j + 2] === 1)) {
                            nextStart = j;
                            break;
                        }
                        j++;
                    }

                    if (nalStart < len) {
                        const nType = dataBytes[nalStart] & 0x1F;
                        nalTypes.push(nType);
                        if (nType === 7) {
                            hasSPS = true;
                            spsUnit = dataBytes.slice(nalStart, nextStart);
                        } else if (nType === 8) {
                            hasPPS = true;
                            ppsUnit = dataBytes.slice(nalStart, nextStart);
                        } else if (nType === 5) {
                            hasIDR = true;
                        } else if (nType === 6) {
                            hasSEI = true;
                        } else if (nType === 9) {
                            hasAUD = true;
                        }
                    }
                    i = nextStart;
                } else {
                    i++;
                }
            }

            return { nalTypes, hasSPS, hasPPS, hasIDR, hasSEI, hasAUD, spsUnit, ppsUnit };
        }

        function getCodecStringFromSps(sps) {
            if (sps && sps.length >= 4) {
                const p = sps[1].toString(16).padStart(2, '0');
                const c = sps[2].toString(16).padStart(2, '0');
                const l = sps[3].toString(16).padStart(2, '0');
                return `avc1.${p}${c}${l}`.toLowerCase();
            }
            return 'avc1.42402a';
        }

        function stripH264StartCode(bytes) {
            if (!bytes || bytes.length === 0) return new Uint8Array(0);
            let start = 0;
            if (bytes.length >= 4 && bytes[0] === 0 && bytes[1] === 0 && bytes[2] === 0 && bytes[3] === 1) {
                start = 4;
            } else if (bytes.length >= 3 && bytes[0] === 0 && bytes[1] === 0 && bytes[2] === 1) {
                start = 3;
            }
            return bytes.slice(start);
        }

        function buildAvccDescription(sps, pps) {
            const seqSps = sps ? stripH264StartCode(sps) : null;
            const seqPps = pps ? stripH264StartCode(pps) : null;
            if (!seqSps || seqSps.length === 0) {
                return new Uint8Array(0);
            }

            let total = 8 + seqSps.length + 2 + (seqPps && seqPps.length ? 2 + seqPps.length : 0);
            let offset = 0;
            const out = new Uint8Array(total);

            out[offset++] = 1; // configurationVersion
            out[offset++] = seqSps[1] ?? 0x42;
            out[offset++] = seqSps[2] ?? 0x00;
            out[offset++] = seqSps[3] ?? 0x1a;
            out[offset++] = 0xFF; // lengthSizeMinusOne + reserved
            out[offset++] = 0xE1; // numOfSequenceParameterSets
            out[offset++] = (seqSps.length >> 8) & 0xFF;
            out[offset++] = seqSps.length & 0xFF;
            out.set(seqSps, offset);
            offset += seqSps.length;

            if (seqPps && seqPps.length > 0) {
                out[offset++] = 0x01; // numOfPictureParameterSets
                out[offset++] = (seqPps.length >> 8) & 0xFF;
                out[offset++] = seqPps.length & 0xFF;
                out.set(seqPps, offset);
                offset += seqPps.length;
            }

            return out.slice(0, offset);
        }

        function extractAnnexBNals(dataBytes) {
            if (!dataBytes || dataBytes.length === 0) {
                return [];
            }

            const nalUnits = [];
            let i = 0;
            const len = dataBytes.length;

            while (i < len) {
                let start = -1;

                if (i + 3 < len && dataBytes[i] === 0 && dataBytes[i + 1] === 0 && dataBytes[i + 2] === 0 && dataBytes[i + 3] === 1) {
                    start = i + 4;
                } else if (i + 2 < len && dataBytes[i] === 0 && dataBytes[i + 1] === 0 && dataBytes[i + 2] === 1) {
                    start = i + 3;
                }

                if (start < 0) {
                    i++;
                    continue;
                }

                let end = len;
                for (let j = start; j + 2 < len; j++) {
                    const nextStartCode4 = dataBytes[j] === 0 && dataBytes[j + 1] === 0 && dataBytes[j + 2] === 0 && dataBytes[j + 3] === 1;
                    const nextStartCode3 = dataBytes[j] === 0 && dataBytes[j + 1] === 0 && dataBytes[j + 2] === 1;
                    if (nextStartCode4 || nextStartCode3) {
                        end = j;
                        break;
                    }
                }

                const nal = dataBytes.subarray(start, end);
                if (nal.length > 0) {
                    nalUnits.push(nal);
                }

                if (end === len) {
                    break;
                }

                i = end;
            }

            if (nalUnits.length === 0) {
                return [dataBytes];
            }

            return nalUnits;
        }

        function convertAnnexBToAvcc(dataBytes) {
            const nals = extractAnnexBNals(dataBytes);
            if (!nals || nals.length === 0) {
                return new Uint8Array(0);
            }

            let totalSize = 0;
            for (const nal of nals) {
                totalSize += 4 + nal.length;
            }

            const out = new Uint8Array(totalSize);
            let offset = 0;
            for (const nal of nals) {
                out[offset] = (nal.length >>> 24) & 0xFF;
                out[offset + 1] = (nal.length >>> 16) & 0xFF;
                out[offset + 2] = (nal.length >>> 8) & 0xFF;
                out[offset + 3] = nal.length & 0xFF;
                out.set(nal, offset + 4);
                offset += 4 + nal.length;
            }

            return out;
        }

        function buildAvccFromNalUnits(nalUnits) {
            if (!nalUnits || nalUnits.length === 0) {
                return new Uint8Array(0);
            }

            let totalSize = 0;
            for (const nal of nalUnits) {
                if (!nal || nal.length === 0) continue;
                totalSize += 4 + nal.length;
            }

            const out = new Uint8Array(totalSize);
            let offset = 0;
            for (const nal of nalUnits) {
                if (!nal || nal.length === 0) continue;
                out[offset] = (nal.length >>> 24) & 0xFF;
                out[offset + 1] = (nal.length >>> 16) & 0xFF;
                out[offset + 2] = (nal.length >>> 8) & 0xFF;
                out[offset + 3] = nal.length & 0xFF;
                out.set(nal, offset + 4);
                offset += 4 + nal.length;
            }

            return out;
        }

        function prepareAnnexBKeyframe(dataBytes, sps, pps) {
            const payloadNals = extractAnnexBNals(dataBytes);
            const effectiveSPS = sps || payloadNals.find((nal) => (nal[0] & 0x1F) === 7) || null;
            const effectivePPS = pps || payloadNals.find((nal) => (nal[0] & 0x1F) === 8) || null;
            const idrNal = payloadNals.find((nal) => (nal[0] & 0x1F) === 5) || null;

            if (!idrNal) {
                return new Uint8Array(0);
            }

            const selected = [];
            const seen = new Set();
            const addIfUnique = (nal) => {
                if (!nal || nal.length === 0) return;
                const key = Array.from(nal).map((b) => b.toString(16).padStart(2, '0')).join('');
                if (seen.has(key)) return;
                seen.add(key);
                selected.push(nal);
            };

            if (effectiveSPS) addIfUnique(effectiveSPS);
            if (effectivePPS) addIfUnique(effectivePPS);
            addIfUnique(idrNal);

            if (selected.length === 0) {
                return new Uint8Array(0);
            }

            const ordered = [];
            for (const nal of selected) {
                const type = nal[0] & 0x1F;
                if (type === 7) ordered.push(nal);
            }
            for (const nal of selected) {
                const type = nal[0] & 0x1F;
                if (type === 8) ordered.push(nal);
            }
            for (const nal of selected) {
                const type = nal[0] & 0x1F;
                if (type === 5) ordered.push(nal);
            }

            return buildAvccFromNalUnits(ordered);
        }

        let pendingWebCodecsFrame = null;
        let webCodecsAnimationFrameId = null;

        function renderPendingWebCodecsFrame() {
            webCodecsAnimationFrameId = null;
            if (!pendingWebCodecsFrame) return;

            const { frame, timing, outputAt } = pendingWebCodecsFrame;
            pendingWebCodecsFrame = null;

            if (canvas && ctx) {
                if (canvas.width !== frame.displayWidth || canvas.height !== frame.displayHeight) {
                    canvas.width = frame.displayWidth;
                    canvas.height = frame.displayHeight;
                    renderWidth = frame.displayWidth;
                    renderHeight = frame.displayHeight;

                    const resEl = document.getElementById("resDisplay");
                    if (resEl) {
                        resEl.textContent = `${renderWidth} × ${renderHeight}`;
                    }
                    addActivityLog(`Resolution changed to ${renderWidth}x${renderHeight}`);

                    if (typeof isActualSize !== 'undefined' && isActualSize) {
                        const dpr = window.devicePixelRatio || 1;
                        canvas.style.width = (canvas.width / dpr) + "px";
                        canvas.style.height = (canvas.height / dpr) + "px";
                    }
                }

                if (!window._display_logged) {
                    console.log(`[DISPLAY]\nframeCodedWidth = ${frame.codedWidth}\nframeCodedHeight = ${frame.codedHeight}\nframeDisplayWidth = ${frame.displayWidth}\nframeDisplayHeight = ${frame.displayHeight}\ncanvasWidth = ${canvas.width}\ncanvasHeight = ${canvas.height}`);
                    console.log("[DISPLAY] Frame rendered");
                    window._display_logged = true;
                }

                ctx.drawImage(frame, 0, 0, canvas.width, canvas.height);
                browserPerf.renderedFrames++;
                browserPerf.renderWindow++;
                videoDiagnostics.lastRenderedAt = Date.now();

                const renderedAt = performance.now();
                if (timing) {
                    browserPerf.outputRenderSamples.push(renderedAt - outputAt);
                    if (timing.captureTimestamp > 0) {
                        const captureRender = Date.now() - timing.captureTimestamp;
                        if (captureRender >= 0 && captureRender < 60000) {
                            browserPerf.captureRenderSamples.push(captureRender);
                            if (renderedAt - lastRenderedLatencyLogAt >= 1000) {
                                const renderTime = Date.now();
                                console.log(`[VIDEO LATENCY TRACE] capture=${timing.captureTimestamp} receive=${timing.receiveTime} decodeSubmit=${timing.decodeSubmitTime} decodeOutput=${timing.decodeOutputTime || "pending"} render=${renderTime} ageMs=${captureRender} decodeQueue=${videoDecoder ? videoDecoder.decodeQueueSize : 0}`);
                                lastRenderedLatencyLogAt = renderedAt;
                            }
                        }
                    }
                }
                setStreamState("DISPLAYING");
            }
            recordVideoMetrics(outputAt);
            frame.close();
        }

        function initVideoDecoder() {
            if (videoDecoder && videoDecoder.state !== 'closed') {
                try {
                    videoDecoder.close();
                } catch (e) { }
            }
            decoderState = DecoderState.UNCONFIGURED;

            try {
                videoDecoder = new VideoDecoder({
                    output(frame) {
                        if (decoderState === DecoderState.WAITING_FOR_KEYFRAME) {
                            decoderState = DecoderState.CONFIGURED;
                            console.log("[DECODER STATE] CONFIGURED (Keyframe decoded)");
                            console.log("[WEBCODECS] KEYFRAME DECODED");
                        }
                        browserPerf.decodedFrames++;
                        browserPerf.decodeWindow++;
                        videoDiagnostics.lastDecodedAt = Date.now();
                        videoFrameCount++;
                        const outputAt = performance.now();
                        const timing = videoTimingByTimestamp.get(frame.timestamp);
                        if (timing) {
                            videoTimingByTimestamp.delete(frame.timestamp);
                            timing.decodeOutputTime = Date.now();
                            browserPerf.receiveDecodeSamples.push(outputAt - timing.receivedAt);
                            browserPerf.decodeOutputSamples.push(outputAt - timing.submittedAt);
                        }

                        if (videoFrameCount <= 5) {
                            console.log(`[DECODER] output received (frame #${videoFrameCount}, state = ${videoDecoder ? videoDecoder.state : 'none'})`);
                        }

                        if (videoFrameCount === 1) {
                            console.log("[VIDEO RENDER] first visible frame");
                        }

                        if (pendingWebCodecsFrame) {
                            pendingWebCodecsFrame.frame.close();
                            browserPerf.staleFramesDiscarded = (browserPerf.staleFramesDiscarded || 0) + 1;
                        }

                        pendingWebCodecsFrame = { frame, timing, outputAt };

                        if (!webCodecsAnimationFrameId) {
                            webCodecsAnimationFrameId = requestAnimationFrame(renderPendingWebCodecsFrame);
                        }
                    },
                    error(error) {
                        // A decode error must NOT tear down the stream or close the WebSocket.
                        // Log the exact frame/NAL context and recover by waiting for the next
                        // valid SPS/PPS/IDR keyframe. The host keeps streaming; the viewer stays open.
                        console.error("[BROWSER DECODE ERROR]", error);
                        console.log("[DECODER] decode error");
                        videoDiagnostics.lastDecoderError = error && error.message ? error.message : String(error);
                        logDecodeErrorDetail(error);
                        if (decoderState !== DecoderState.UNCONFIGURED) {
                            console.warn("[WEBCODECS RECOVER] Decoder error recovered. Resetting decoder. Waiting for next keyframe.");
                            decoderState = DecoderState.UNCONFIGURED;
                            setStreamState("WAITING_FOR_KEYFRAME");
                            // We MUST completely close/recreate the decoder because a decode error puts it into a permanently closed/error state.
                            initVideoDecoder();
                        }
                    }
                });

                if (window._rx_count <= 5) {
                    const realState = videoDecoder ? videoDecoder.state : "none";
                    console.log(`[DECODER]\nconfigured = ${videoDecoder ? 'YES' : 'NO'}\nstate = ${realState}\ndecoderState = ${decoderState}\ndecodeQueueSize = ${videoDecoder ? videoDecoder.decodeQueueSize : 0}`);
                }
                return true;
            } catch (err) {
                console.error("[VIDEO FATAL] Failed to construct VideoDecoder:", err);
                decoderState = DecoderState.ERROR;
                return false;
            }
        }

        function compactVideoBacklogAtIdr() {
            if (wsRxBuffer.length <= MAX_RX_BUFFER_BYTES &&
                (!videoDecoder || videoDecoder.decodeQueueSize <= MAX_DECODER_QUEUE)) {
                return false;
            }

            let offset = 0;
            let latestIdrOffset = -1;
            let discardedPackets = 0;
            let packetsSeen = 0;
            while (offset + 21 <= wsRxBuffer.length) {
                const type = wsRxBuffer[offset];
                if (type !== 13 && type !== 15) {
                    break;
                }

                const payloadSize =
                    ((wsRxBuffer[offset + 9] << 24) >>> 0) |
                    (wsRxBuffer[offset + 10] << 16) |
                    (wsRxBuffer[offset + 11] << 8) |
                    wsRxBuffer[offset + 12];
                const totalPacketSize = 21 + payloadSize;
                if (payloadSize <= 0 || payloadSize > 20 * 1024 * 1024 ||
                    offset + totalPacketSize > wsRxBuffer.length) {
                    break;
                }

                const payload = wsRxBuffer.subarray(offset + 21, offset + totalPacketSize);
                if (parseH264Nals(payload).hasIDR) {
                    latestIdrOffset = offset;
                    discardedPackets = packetsSeen;
                }
                packetsSeen++;
                offset += totalPacketSize;
            }

            if (latestIdrOffset <= 0) {
                return false;
            }

            wsRxBuffer = wsRxBuffer.subarray(latestIdrOffset);
            browserPerf.staleFramesDiscarded += discardedPackets + 1;
            console.warn(`[VIDEO RECOVERY] Discarded ${discardedPackets + 1} stale complete access units; resuming at a real IDR.`);
            return true;
        }

        function handleVideoPacket(buffer) {
            try {
                const bytes = new Uint8Array(buffer);
                const length = bytes.length;
                window._rx_count = (window._rx_count || 0) + 1;
                videoDiagnostics.lastPacketAt = Date.now();
                videoDiagnostics.lastPacketType = bytes[0] ?? null;
                videoDiagnostics.lastPacketLength = length;

                const nowMs = performance.now();
                if (!window.__videoHexLogLast || (nowMs - window.__videoHexLogLast) > 3000) {
                    const hex = Array.from(bytes.slice(0, 32)).map((b) => b.toString(16).padStart(2, '0')).join(' ');
                    console.log(`[VIDEO HEX]\n${hex}`);
                    window.__videoHexLogLast = nowMs;
                }

                const view = new DataView(buffer);
                const packetType = view.getUint8(0);

                if (packetType !== 13 && packetType !== 15) {
                    console.warn("[VIDEO] Not a video packet:", packetType);
                    return;
                }

                const likelyLegacyH264Start =
                    packetType === 15 && length >= 13 && (
                        length < 21 ||
                        (
                            bytes[13] === 0 &&
                            ((bytes[14] === 0 && bytes[15] === 0 && bytes[16] === 1) ||
                                (bytes[14] === 0 && bytes[15] === 1))
                        )
                    );
                const legacyFormat = likelyLegacyH264Start;
                const headerBytes = legacyFormat ? 13 : 21;

                if (length < headerBytes) {
                    console.warn("[VIDEO] Packet too small:", { packetType, length, required: headerBytes, legacyFormat });
                    return;
                }

                const width = view.getUint32(1, false);
                const height = view.getUint32(5, false);
                const payloadSize = view.getUint32(9, false);
                const captureTimestamp = legacyFormat ? 0 : Number(view.getBigUint64(13, false));

                const available = length - headerBytes;

                const compatLogWindowMs = 3000;
                if (!window.__videoCompatLastLog || (nowMs - window.__videoCompatLastLog) > compatLogWindowMs) {
                    console.log(`[VIDEO PARSER]\ntype = ${packetType}\nlegacy = ${legacyFormat}\nheaderBytes = ${headerBytes}\nwidth = ${width}\nheight = ${height}\npayloadSize = ${payloadSize}\navailable = ${available}\npacketSize = ${length}`);
                    console.log(`[VIDEO PACKET]\ntype=${packetType}\nwidth=${width}\nheight=${height}\npayloadSize=${payloadSize}\nlength=${length}\nheaderBytes=${headerBytes}`);
                    window.__videoCompatLastLog = nowMs;
                }

                if (payloadSize <= 0 || payloadSize > available) {
                    console.error(
                        "[VIDEO] Invalid H.264 payload size.",
                        { packetType, legacyFormat, payloadSize, available, packetSize: length, headerBytes }
                    );
                    return;
                }

                const actualPayloadSize = (payloadSize > 0 && payloadSize <= available) ? payloadSize : available;
                const h264Payload = bytes.slice(headerBytes, headerBytes + actualPayloadSize);

                if (!window.__videoPayloadLogLast || (nowMs - window.__videoPayloadLogLast) > 3000) {
                    console.log(`[H264 RX]\npayloadBytes=${h264Payload.length}\nannexB=${h264Payload.length >= 3 && h264Payload[0] === 0 && h264Payload[1] === 0 && (h264Payload[2] === 1 || (h264Payload[2] === 0 && h264Payload[3] === 1))}\nnalCount=${parseH264Nals(h264Payload).nalTypes.length}\nnalTypes=[${parseH264Nals(h264Payload).nalTypes.join(',')}]`);
                    window.__videoPayloadLogLast = nowMs;
                }

                streamStats.received_packets++;
                streamStats.received_bytes += actualPayloadSize;
                browserPerf.rxPackets++;
                if (chatVideoSnapshot) {
                    console.info(
                        `[CHAT VIDEO AFTER SEND] delayMs=${Date.now() - chatVideoSnapshot.sentAt} ` +
                        `videoPacketsBefore=${chatVideoSnapshot.packetCountBefore} ` +
                        `videoPacketsAfter=${streamStats.received_packets} ` +
                        `decodedFrames=${browserPerf.decodedFrames} renderedFrames=${browserPerf.renderedFrames} ` +
                        `parserBufferBytes=${wsRxBuffer.length}`
                    );
                    chatVideoSnapshot = null;
                }

                const receiveTime = Date.now();
                const receivedAt = performance.now();
                browserPerf.receiveWindow++;

                // The agent capture timestamp is a Unix-epoch value in MILLISECONDS, the same
                // base as Date.now(). Only compute latency when it is a plausible epoch value AND
                // the result is sane. If the agent/relay did not populate the timestamp (or used
                // an incompatible base such as mixing performance.now()), DO NOT subtract it and
                // emit a nonsense value like 1.78e12 ms. Log the raw values for diagnosis instead.
                const EPOCH_MIN = 1577836800000;  // 2020-01-01 UTC (ms)
                const EPOCH_MAX = 4102444800000;  // 2100-01-01 UTC (ms)
                let latencyValid = captureTimestamp >= EPOCH_MIN && captureTimestamp <= EPOCH_MAX;
                let networkLatency = -1;
                if (latencyValid) {
                    networkLatency = receiveTime - captureTimestamp;
                    if (networkLatency < 0 || networkLatency > 60000) {
                        latencyValid = false; // implausible for a LAN screen-share -> incompatible base
                    }
                }
                const decodeQueue = videoDecoder ? videoDecoder.decodeQueueSize : 0;
                browserPerf.queueSum += decodeQueue;
                browserPerf.queueSamples++;
                browserPerf.maxQueue = Math.max(browserPerf.maxQueue, decodeQueue);

                if (receivedAt - lastLatencyLogAt >= 1000) {
                    if (latencyValid) {
                        console.log(`[VIDEO LATENCY] capture=${captureTimestamp} receive=${receiveTime} render=pending ageMs=${networkLatency} decodeQueue=${decodeQueue}`);
                    } else {
                        console.log(`[VIDEO LATENCY] capture=${captureTimestamp} receive=${receiveTime} render=pending ageMs=n/a decodeQueue=${decodeQueue}`);
                    }
                    lastLatencyLogAt = receivedAt;
                }

                // Capability Detection BEFORE using VideoDecoder
                if (!("VideoDecoder" in window)) {
                    console.error("[VIDEO FATAL] WebCodecs VideoDecoder is NOT available");
                    console.error("[VIDEO FATAL] Browser:", navigator.userAgent);
                    console.error("[VIDEO FATAL] isSecureContext:", window.isSecureContext);
                    console.error("[VIDEO FATAL] WebCodecs:", ("VideoDecoder" in window));
                    if (!window.isSecureContext) {
                        console.error("[VIDEO FATAL] Reason: WebCodecs is ONLY enabled in Secure Contexts (HTTPS or http://localhost). Accessing via LAN IP (e.g. http://192.168.x.x) disables WebCodecs unless HTTPS is used or 'chrome://flags/#unsafely-treat-insecure-origin-as-secure' is enabled for this origin.");
                        setHud("FATAL: WebCodecs unavailable (Non-Secure Context). Open via https:// or localhost, or enable browser flag.");
                    }
                    return;
                }

                // STEP 2: NAL Parsing & Parameter Set Caching
                const nals = parseH264Nals(h264Payload);
                if (nals.hasSPS && nals.spsUnit) {
                    cachedSPS = nals.spsUnit;
                }
                if (nals.hasPPS && nals.ppsUnit) {
                    cachedPPS = nals.ppsUnit;
                }

                // STEP 3: Keyframe State & Evaluation with PERSISTENT SPS/PPS caching.
                // SPS/PPS may arrive in an earlier packet; cache them and reuse for later IDR frames.
                const currentSPS = nals.hasSPS;
                const currentPPS = nals.hasPPS;
                const currentIDR = nals.hasIDR;
                if (currentIDR) {
                    videoDiagnostics.lastIdrAt = Date.now();
                }
                const cachedSPSExists = cachedSPS !== null;
                const cachedPPSExists = cachedPPS !== null;

                const hasSPS = currentSPS || cachedSPSExists;
                const hasPPS = currentPPS || cachedPPSExists;
                const hasIDR = currentIDR;

                // Classification:
                //   KEY                        -> IDR + (current or cached) SPS + (current or cached) PPS
                //   WAITING_FOR_PARAMETER_SETS -> IDR present but required SPS/PPS not yet available
                //   DELTA                      -> non-IDR frame (only decodable after a keyframe)
                let classification;
                if (hasIDR && hasSPS && hasPPS) {
                    classification = "KEY";
                } else if (hasIDR) {
                    classification = "WAITING_FOR_PARAMETER_SETS";
                } else {
                    classification = "DELTA";
                }

                const isKey = classification === "KEY";

                // keyDeltaStr MUST be initialized before any log/reference to it (fixes TDZ ReferenceError).
                const keyDeltaStr = isKey ? "key" : (classification === "DELTA" ? "delta" : "waiting");
                if (window._rx_count <= 5 || nowMs - lastKeyDeltaLogAt >= 1000) {
                    console.log('[VIDEO] key/delta =', keyDeltaStr);
                    lastKeyDeltaLogAt = nowMs;
                }

                if (window._rx_count <= 5) {
                    console.log(`[VIDEO] NAL types=[${nals.nalTypes.join(',')}]`);
                    console.log(`[VIDEO] currentSPS=${currentSPS}`);
                    console.log(`[VIDEO] cachedSPS=${cachedSPSExists}`);
                    console.log(`[VIDEO] currentPPS=${currentPPS}`);
                    console.log(`[VIDEO] cachedPPS=${cachedPPSExists}`);
                    console.log(`[VIDEO] IDR=${currentIDR}`);
                    console.log(`[VIDEO] classification=${classification}`);
                }

                for (const n of nals.nalTypes) {
                    if (n === 7) {
                        streamStats.received_sps++;
                    } else if (n === 8) {
                        streamStats.received_pps++;
                    } else if (n === 5) {
                        streamStats.received_idr++;
                    } else if (n === 1) {
                        streamStats.received_non_idr++;
                    }
                }

                // Codec string derivation from SPS
                const spsForCodec = nals.spsUnit || cachedSPS;
                const codecString = getCodecStringFromSps(spsForCodec);
                const ppsForCodec = nals.hasPPS ? nals.ppsUnit : cachedPPS;
                const avccDescription = buildAvccDescription(spsForCodec, ppsForCodec);
                if (spsForCodec || ppsForCodec) {
                    console.log(`[VIDEO AVCC]\ncodec=${codecString}\nspsBytes=${spsForCodec ? spsForCodec.length : 0}\nppsBytes=${ppsForCodec ? ppsForCodec.length : 0}\ndescriptionBytes=${avccDescription.length}`);
                }

                if (isKey && videoDecoder && videoDecoder.decodeQueueSize > MAX_DECODER_QUEUE) {
                    console.warn(`[VIDEO RECOVERY] Resetting decoder at IDR; queue was ${videoDecoder.decodeQueueSize}.`);
                    videoTimingByTimestamp.clear();
                    try {
                        videoDecoder.close();
                    } catch (e) {
                        console.error("[VIDEO RECOVERY] Failed to close overloaded decoder:", e);
                    }
                    videoDecoder = null;
                    decoderState = DecoderState.UNCONFIGURED;
                    browserPerf.idrRecoveryCount++;
                }

                // Initialize VideoDecoder if needed
                if (!videoDecoder || videoDecoder.state === 'closed' || decoderState === DecoderState.ERROR) {
                    if (!initVideoDecoder()) {
                        return;
                    }
                }

                // Keep the JS-side tracking in sync with the real decoder state.
                if (videoDecoder && videoDecoder.state === 'closed') {
                    decoderState = DecoderState.UNCONFIGURED;
                }

                // Configure decoder when it is genuinely unconfigured. configure() is synchronous:
                // immediately after it returns, videoDecoder.state === 'configured'. We must not call
                // decode() until that is true, otherwise Chrome throws and the frame is lost.
                if (decoderState === DecoderState.UNCONFIGURED || (videoDecoder && videoDecoder.state === 'unconfigured')) {
                    console.log(`[VIDEO DECODER CONFIG]\ncodec=${codecString}\nwidth=${width}\nheight=${height}\ndescriptionBytes=${avccDescription.length}\nstate=${videoDecoder ? videoDecoder.state : 'none'}`);
                    console.log(`[DECODER] current state = ${videoDecoder ? videoDecoder.state : 'none'}`);
                    console.log(`[DECODER] configured = ${videoDecoder ? 'YES' : 'NO'}`);
                    console.log(`[DECODER] configure() starting`);
                    try {
                        videoDecoder.configure({
                            codec: codecString,
                            codedWidth: width,
                            codedHeight: height,
                            description: avccDescription,
                            optimizeForLatency: true
                        });
                        console.log(`[VIDEO DECODER CONFIGURED]\nstate=${videoDecoder.state}\ncodec=${codecString}\nwidth=${width}\nheight=${height}`);
                        console.log(`[DECODER] configure() completed (state = ${videoDecoder.state})`);
                        decoderState = DecoderState.WAITING_FOR_KEYFRAME;
                        setStreamState("WAITING_FOR_KEYFRAME");
                        console.log(`[WEBCODECS CONFIG]\ncodec=${codecString}\nwidth=${width}\nheight=${height}\ndescriptionBytes=${avccDescription.length}\nformat=AVCC`);
                    } catch (e) {
                        console.error("[VIDEO DECODER ERROR]", e);
                        console.error("[BROWSER DECODER CONFIG ERROR]", e);
                        logDecodeErrorDetail(e);
                        decoderState = DecoderState.WAITING_FOR_KEYFRAME;
                        return;
                    }
                }

                // Monotonic timestamp in microseconds
                const timestamp = Math.round(performance.now() * 1000);

                // Defensive: never decode while the decoder is not actually configured.
                if (!videoDecoder || videoDecoder.state !== 'configured') {
                    console.warn("[WEBCODECS] Skipping decode: decoder not in 'configured' state (state=" + (videoDecoder ? videoDecoder.state : "none") + "). Waiting for configure.");
                    return;
                }

                // Track NAL context for diagnostics on any decode error.
                lastVideoNalInfo = {
                    isKey: isKey,
                    nalTypes: nals.nalTypes,
                    hasSPS: nals.hasSPS,
                    hasPPS: nals.hasPPS,
                    hasIDR: nals.hasIDR,
                    bytes: actualPayloadSize,
                    width: width,
                    height: height,
                    codec: codecString
                };

                // State Handling: WAITING_FOR_KEYFRAME
                if (decoderState === DecoderState.WAITING_FOR_KEYFRAME) {
                    if (!isKey) {
                        console.warn("[BROWSER VIDEO] Waiting for first keyframe (SPS/PPS/IDR) before decoding delta frames.");
                        return;
                    }

                    const accessUnitNals = extractAnnexBNals(h264Payload);
                    const accessUnitTypes = accessUnitNals.map((nal) => nal[0] & 0x1F);
                    const containsIDR = accessUnitTypes.includes(5);
                    if (!containsIDR) {
                        console.warn(`[VIDEO KEYFRAME VALIDATION]\ncontainsIDR=false\nactualNalTypes=[${accessUnitTypes.join(',')}]\nwaitingForRealIDR=true`);
                        return;
                    }

                    const avccKeyframe = prepareAnnexBKeyframe(h264Payload, cachedSPS, cachedPPS);
                    const first32 = Array.from(avccKeyframe.slice(0, 32)).map(b => b.toString(16).padStart(2, '0')).join(' ');
                    const nalCount = accessUnitNals.length;
                    console.log(`[VIDEO KEYFRAME VALIDATION]\ncontainsIDR=${containsIDR}\nactualNalTypes=[${accessUnitTypes.join(',')}]\nhasSPS=${nals.hasSPS}\nhasPPS=${nals.hasPPS}\nhasIDR=${nals.hasIDR}\navccBytes=${avccKeyframe.byteLength}`);
                    console.log(`[KEYFRAME SUBMIT]\ndecoderState=${decoderState}\nvideoDecoderState=${videoDecoder ? videoDecoder.state : 'none'}\ncodec=${codecString}\nwidth=${width}\nheight=${height}\nnalTypes=[${accessUnitTypes.join(',')}]\nhasSPS=${nals.hasSPS}\nhasPPS=${nals.hasPPS}\nhasIDR=${nals.hasIDR}\navccBytes=${avccKeyframe.byteLength}`);
                    console.log(`[H264 AVCC]\nnalCount=${nalCount}\ntotalBytes=${avccKeyframe.byteLength}\nfirstBytes=${first32}`);
                    console.log(`[WEBCODECS DECODE]\ntype=key\ntimestamp=${timestamp}\nbytes=${avccKeyframe.byteLength}\nNAL types=[${accessUnitTypes.join(',')}]`);
                    console.log(`[DECODE PAYLOAD FORMAT]\nfirst_32_hex: ${first32}\nstarts_with_00_00_00_01: false\nstarts_with_00_00_01: false\nmode=AVCC`);

                    console.log(`[DECODER] decode() starting (state = ${videoDecoder ? videoDecoder.state : 'none'})`);

                    try {
                        const chunk = new EncodedVideoChunk({
                            type: 'key',
                            timestamp: timestamp,
                            data: avccKeyframe
                        });
                        videoTimingByTimestamp.set(timestamp, {
                            captureTimestamp,
                            receiveTime,
                            receivedAt,
                            submittedAt: performance.now(),
                            decodeSubmitTime: Date.now()
                        });
                        videoDecoder.decode(chunk);
                        decoderState = DecoderState.CONFIGURED;
                    } catch (e) {
                        console.error("[BROWSER DECODER ERROR] decode(key) exception:", e);
                        logDecodeErrorDetail(e);
                        decoderState = DecoderState.UNCONFIGURED;
                        initVideoDecoder();
                    }
                    return;
                }

                if (decoderState === DecoderState.CONFIGURED) {
                    const chunkType = isKey ? 'key' : 'delta';
                    const avccChunk = convertAnnexBToAvcc(h264Payload);

                    if (videoDecoder && videoDecoder.decodeQueueSize > MAX_DECODER_QUEUE && chunkType === 'delta') {
                        browserPerf.staleFramesDiscarded++;
                        console.warn(`[WEBCODECS RECOVERY] Decoder queue reached ${videoDecoder.decodeQueueSize}; dropping deltas until the next real IDR.`);
                        videoTimingByTimestamp.clear();
                        try {
                            videoDecoder.close();
                        } catch (e) {
                            console.error("[WEBCODECS RECOVERY] Failed to close delayed decoder:", e);
                        }
                        videoDecoder = null;
                        decoderState = DecoderState.UNCONFIGURED;
                        browserPerf.idrRecoveryCount++;
                        return;
                    }

                    try {
                        const chunk = new EncodedVideoChunk({
                            type: chunkType,
                            timestamp: timestamp,
                            data: avccChunk
                        });
                        videoTimingByTimestamp.set(timestamp, {
                            captureTimestamp,
                            receiveTime,
                            receivedAt,
                            submittedAt: performance.now(),
                            decodeSubmitTime: Date.now()
                        });
                        videoDecoder.decode(chunk);
                        if (videoTimingByTimestamp.size > MAX_DECODER_QUEUE + 8) {
                            const oldestTimestamp = videoTimingByTimestamp.keys().next().value;
                            videoTimingByTimestamp.delete(oldestTimestamp);
                        }
                    } catch (e) {
                        videoTimingByTimestamp.delete(timestamp);
                        console.error(`[BROWSER DECODER ERROR] decode(${chunkType}) exception:`, e);
                        logDecodeErrorDetail(e);
                        decoderState = DecoderState.WAITING_FOR_KEYFRAME;
                    }
                }
            } catch (e) {
                // A single malformed/exception-throwing frame must NOT terminate the video
                // WebSocket or the whole stream. Log it and skip the frame; decoding continues
                // with the next packet. (Primary fix is the keyDeltaStr init order above.)
                console.error("[VIDEO] handleVideoPacket() unexpected error (frame skipped, stream continues):", e);
            }
        }

        /* ============================================================
           AUDIO PACKET

        ============================================================ */

        function handleAudioPacket(buffer) {
            /*
                TYPE 17 = AUDIO
        
                [1 byte type]
                [4 bytes data size (u32 BE)]
                [4 bytes sample rate (u32 BE)]
                [2 bytes channels (u16 BE)]
                [N bytes audio data: Float32 PCM LE]
            */
            const tRx = performance.now();
            const type = new Uint8Array(buffer)[0];

            if (type !== 17) {
                console.warn("[AUDIO] Not an audio packet:", type);
                return;
            }

            if (!isAudioEnabled) {
                return;
            }

            if (buffer.byteLength < 11) {
                console.warn("[AUDIO] Packet too small.");
                return;
            }

            const view = new DataView(buffer);
            const byteLen = view.getUint32(1, false);
            const sampleRate = view.getUint32(5, false);
            const channels = view.getUint16(9, false);

            if (
                byteLen <= 0 ||
                byteLen % 4 !== 0 ||
                byteLen !== buffer.byteLength - 11 ||
                sampleRate <= 0 ||
                sampleRate > 192000 ||
                channels <= 0 ||
                channels > 32
            ) {
                return;
            }

            const start = 11;
            const end = start + byteLen;
            if (end > buffer.byteLength) {
                console.error("[AUDIO] Invalid audio length.");
                return;
            }

            const tDecodeStart = performance.now();
            // Fast extraction of Float32Array: sliced ArrayBuffer starts at offset 0 (4-byte aligned)
            const audioBytes = buffer.slice(start, end);
            const floatArray = new Float32Array(audioBytes);

            if (!floatArray.length) {
                return;
            }

            if (!audioCtx) {
                audioCtx = new (window.AudioContext || window.webkitAudioContext)({
                    latencyHint: "interactive",
                    sampleRate: sampleRate || 48000
                });
            }

            if (audioCtx.state === "suspended") {
                audioCtx.resume().catch(console.error);
            }

            const frames = Math.floor(floatArray.length / channels);
            if (frames <= 0 || floatArray.length % channels !== 0) {
                return;
            }

            const audioBuffer = audioCtx.createBuffer(
                channels,
                frames,
                sampleRate
            );

            if (channels === 1) {
                // Direct fast copy for mono voice (zero nested loops)
                audioBuffer.copyToChannel(floatArray, 0);
            } else {
                for (let c = 0; c < channels; c++) {
                    const channelData = audioBuffer.getChannelData(c);
                    for (let i = 0; i < frames; i++) {
                        channelData[i] = floatArray[i * channels + c];
                    }
                }
            }
            const tDecodeEnd = performance.now();

            const source = audioCtx.createBufferSource();
            source.buffer = audioBuffer;
            source.connect(audioCtx.destination);

            const curTime = audioCtx.currentTime;
            const TARGET_JITTER_BUFFER = 0.025; // 25ms optimal jitter buffer for real-time speech
            const MAX_ALLOWED_LATENCY = 0.080;   // 80ms max queue lag before resynchronizing

            // If audio underran (gap in speech / starting fresh)
            if (nextAudioTime < curTime) {
                nextAudioTime = curTime + TARGET_JITTER_BUFFER;
            } 
            // If audio queue accumulated excessive latency (e.g. background tab throttling or network burst)
            else if (nextAudioTime > curTime + MAX_ALLOWED_LATENCY) {
                // Stop any queued but unplayed nodes to prevent overlapping echo/distortion
                for (let i = 0; i < activeAudioNodes.length; i++) {
                    try { activeAudioNodes[i].stop(); } catch (_) {}
                }
                activeAudioNodes = [];
                nextAudioTime = curTime + TARGET_JITTER_BUFFER;
            }

            source.start(nextAudioTime);
            activeAudioNodes.push(source);
            source.onended = () => {
                const idx = activeAudioNodes.indexOf(source);
                if (idx !== -1) activeAudioNodes.splice(idx, 1);
            };

            const scheduledTime = nextAudioTime;
            nextAudioTime += audioBuffer.duration;

            audioReceiverDiagnosticCount = (audioReceiverDiagnosticCount || 0) + 1;
            if (audioReceiverDiagnosticCount % 100 === 1) {
                const queueDurationMs = Math.max(0, (nextAudioTime - curTime) * 1000);
                console.log(`[B-AUDIO] rx: ${tRx.toFixed(2)}ms | decode: ${(tDecodeEnd - tDecodeStart).toFixed(2)}ms | scheduled_at: ${scheduledTime.toFixed(3)}s (cur: ${curTime.toFixed(3)}s) | queue_depth: ${queueDurationMs.toFixed(1)}ms | chunk_dur: ${(audioBuffer.duration * 1000).toFixed(1)}ms`);
            }
        }


        /* ============================================================
           CHAT PACKET
        ============================================================ */

        function handleChatPacket(buffer) {

            if (buffer.byteLength < 4) {
                console.error(`[CHAT RX FRAMING ERROR] type=16 packet_bytes=${buffer.byteLength} expected_min=4`);
                return;
            }

            const view =
                new DataView(buffer);

            const len =
                (
                    view.getUint8(2) << 8
                ) |
                view.getUint8(3);

            const payloadEnd = 4 + len;
            if (payloadEnd !== buffer.byteLength) {
                console.error(`[CHAT RX FRAMING ERROR] type=16 declared_payload_len=${len} consumed_bytes=${payloadEnd} actual_packet_bytes=${buffer.byteLength} payload_start=4 payload_end=${payloadEnd}`);
                return;
            }

            const msgBytes =
                new Uint8Array(
                    buffer,
                    4,
                    len
                );

            if (!chatPacketDiagnosticLogged) {
                const payloadPrefixHex = Array.from(msgBytes.slice(0, 64))
                    .map(byte => byte.toString(16).padStart(2, "0"))
                    .join(" ");
                console.info(`[CHAT RX TRACE] type=16 declared_payload_len=${len} consumed_bytes=${payloadEnd} payload_start=4 payload_end=${payloadEnd} payload_bytes=${msgBytes.length} payload_prefix_hex="${payloadPrefixHex}"`);
                chatPacketDiagnosticLogged = true;
            }
            const packetHex = Array.from(new Uint8Array(buffer))
                .map(byte => byte.toString(16).padStart(2, "0"))
                .join(" ");
            console.info(`[CHAT RX EXACT] type=16 packet_bytes=${buffer.byteLength} payload_bytes=${len} bytes_hex="${packetHex}"`);
            let text;
            try {
                text = new TextDecoder("utf-8", { fatal: true }).decode(msgBytes);
            } catch (error) {
                console.error(`[CHAT RX UTF8 ERROR] type=16 payload_len=${len} message=${error.message}`);
                return;
            }

            console.info(`[CHAT RX] direction=B->A type=16 payload_bytes=${len} characters=${text.length}`);
            queueMicrotask(() => {
                window.dispatchEvent(new CustomEvent("chat_message_received", {
                    detail: { text }
                }));
            });

        }


        /* ============================================================
           WEBSOCKET MESSAGE
        ============================================================ */

        async function handleMessage(event) {
            let chunk;
            if (event.data instanceof ArrayBuffer) {
                chunk = new Uint8Array(event.data);
            } else if (event.data instanceof Blob) {
                chunk = new Uint8Array(await event.data.arrayBuffer());
            } else {
                console.warn("[WS] Non-binary message:", event.data);
                return;
            }

            if (chunk.length === 0) return;

            const newBuf = new Uint8Array(wsRxBuffer.length + chunk.length);
            newBuf.set(wsRxBuffer);
            newBuf.set(chunk, wsRxBuffer.length);
            wsRxBuffer = newBuf;
            browserPerf.maxWsRxBuffer = Math.max(browserPerf.maxWsRxBuffer, wsRxBuffer.length);

            if (wsRxProcessing) {
                return;
            }

            wsRxProcessing = true;
            try {
                while (wsRxBuffer.length > 0) {
                    compactVideoBacklogAtIdr();
                    const type = wsRxBuffer[0];
                    const bufferLen = wsRxBuffer.length;

                    if (type === 13 || type === 15) {
                        if (bufferLen < 21) {
                            return;
                        }

                        const width =
                            ((wsRxBuffer[1] << 24) >>> 0) |
                            (wsRxBuffer[2] << 16) |
                            (wsRxBuffer[3] << 8) |
                            wsRxBuffer[4];
                        const height =
                            ((wsRxBuffer[5] << 24) >>> 0) |
                            (wsRxBuffer[6] << 16) |
                            (wsRxBuffer[7] << 8) |
                            wsRxBuffer[8];
                        const payloadSize =
                            ((wsRxBuffer[9] << 24) >>> 0) |
                            (wsRxBuffer[10] << 16) |
                            (wsRxBuffer[11] << 8) |
                            wsRxBuffer[12];

                        const totalPacketSize = 21 + payloadSize;

                        const saneHeader =
                            width > 0 && width <= 10000 &&
                            height > 0 && height <= 10000 &&
                            payloadSize > 0 && payloadSize <= 20 * 1024 * 1024 &&
                            totalPacketSize >= 21;

                        if (!saneHeader) {
                            wsRxBuffer = wsRxBuffer.slice(1);
                            continue;
                        }

                        if (bufferLen < totalPacketSize) {
                            return;
                        }

                        if (!window.__videoParserLogLast || (performance.now() - window.__videoParserLogLast) > 3000) {
                            console.log(`[VIDEO PARSER]\nfirstByte=${type}\nheaderSize=21\nwidth=${width}\nheight=${height}\npayloadSize=${payloadSize}\navailable=${bufferLen}\nexpected=${totalPacketSize}\nfirstBytes=${Array.from(wsRxBuffer.slice(0, 16)).map((b) => b.toString(16).padStart(2, '0')).join(' ')}`);
                            window.__videoParserLogLast = performance.now();
                        }

                        const packetBytes = wsRxBuffer.subarray(0, totalPacketSize);
                        const packetBuffer = packetBytes.slice().buffer;
                        handleVideoPacket(packetBuffer);
                        wsRxBuffer = wsRxBuffer.subarray(totalPacketSize);
                        continue;
                    }
                    else if (type === 17) {
                        if (bufferLen < 11) {
                            return;
                        }
                        const view = new DataView(wsRxBuffer.buffer, wsRxBuffer.byteOffset, wsRxBuffer.byteLength);
                        const payloadSize = view.getUint32(1, false);
                        const totalPacketSize = 11 + payloadSize;

                        if (bufferLen < totalPacketSize) {
                            return;
                        }

                        const packetBytes = wsRxBuffer.subarray(0, totalPacketSize);
                        const packetBuffer = packetBytes.slice().buffer;
                        handleAudioPacket(packetBuffer);
                        wsRxBuffer = wsRxBuffer.subarray(totalPacketSize);
                        continue;
                    }
                    else if (type === 16) {
                        if (bufferLen < 4) {
                            return;
                        }
                        const view = new DataView(wsRxBuffer.buffer, wsRxBuffer.byteOffset, wsRxBuffer.byteLength);
                        const payloadSize = (view.getUint8(2) << 8) | view.getUint8(3);
                        const totalPacketSize = 4 + payloadSize;

                        if (bufferLen < totalPacketSize) {
                            return;
                        }

                        const packetBytes = wsRxBuffer.subarray(0, totalPacketSize);
                        const packetBuffer = packetBytes.slice().buffer;
                        handleChatPacket(packetBuffer);
                        const nextPacketType = wsRxBuffer[totalPacketSize];
                        console.info(
                            `[WS DISPATCH] type=16 declared_bytes=${totalPacketSize} consumed_bytes=${totalPacketSize} ` +
                            `buffer_before=${bufferLen} buffer_after=${bufferLen - totalPacketSize} ` +
                            `next_packet_type=${nextPacketType ?? "pending"}`
                        );
                        wsRxBuffer = wsRxBuffer.subarray(totalPacketSize);
                        continue;
                    }
                    else if (type === 18) {
                        if (bufferLen < 2) return;
                        const typingState = wsRxBuffer[1];
                        if (typingState > 1) {
                            console.error(`[CHAT TYPING RX] Invalid type 18 state=${typingState}`);
                        } else {
                            setRemoteChatTyping(typingState === 1);
                        }
                        wsRxBuffer = wsRxBuffer.subarray(2);
                        continue;
                    }
                    else if (type === 30) {
                        receiveReverseRequest();
                        wsRxBuffer = wsRxBuffer.subarray(1);
                        continue;
                    }
                    else if (type === 31) {
                        if (bufferLen < 2) return;
                        const decision = wsRxBuffer[1];
                        if (decision > 1) {
                            console.error(`[REVERSE] Invalid TYPE 31 decision=${decision}`);
                        } else {
                            console.log(`[REVERSE] Permission response received: ${decision === 1 ? "accepted" : "rejected"}`);
                        }
                        wsRxBuffer = wsRxBuffer.subarray(2);
                        continue;
                    }
                    else if (type === 1) {
                        websocketAuthenticated = true;
                        sessionWasEstablished = true;
                        console.log("[WS] Session approved by host.");
                        wsRxBuffer = wsRxBuffer.slice(1);
                        continue;
                    }
                    else if (type === 2) {
                        websocketAuthenticated = true;
                        sessionWasEstablished = true;
                        setStreamState("STREAM_ACTIVE");
                        wsRxBuffer = wsRxBuffer.slice(1);
                        continue;
                    }
                    else if (type >= 20 && type <= 27) {
                        let totalPacketSize = 0;
                        let headerLen = 0;
                        if (type === 20) {
                            headerLen = 19;
                            if (bufferLen < headerLen) return;
                            const view = new DataView(wsRxBuffer.buffer, wsRxBuffer.byteOffset, wsRxBuffer.byteLength);
                            const nameLen = view.getUint16(17, false);
                            totalPacketSize = headerLen + nameLen;
                        } else if (type === 21) {
                            headerLen = 17;
                            if (bufferLen < headerLen) return;
                            const view = new DataView(wsRxBuffer.buffer, wsRxBuffer.byteOffset, wsRxBuffer.byteLength);
                            const payloadLen = view.getUint32(13, false);
                            totalPacketSize = headerLen + payloadLen;
                        } else if (type === 22) {
                            totalPacketSize = 49;
                        } else if (type === 23) {
                            totalPacketSize = 9;
                        } else if (type === 24) {
                            headerLen = 13;
                            if (bufferLen < headerLen) return;
                            const view = new DataView(wsRxBuffer.buffer, wsRxBuffer.byteOffset, wsRxBuffer.byteLength);
                            const msgLen = view.getUint16(11, false);
                            totalPacketSize = headerLen + msgLen;
                        } else if (type === 25 || type === 26 || type === 27) {
                            totalPacketSize = 9;
                        }

                        if (bufferLen < totalPacketSize) return;

                        const packetBytes = wsRxBuffer.subarray(0, totalPacketSize);
                        const packetBuffer = packetBytes.slice().buffer;
                        if (typeof handleFilePacket === 'function') {
                            try {
                                await handleFilePacket(packetBuffer);
                            } catch (error) {
                                console.error(`[FILE RX] Packet handler failed for type ${type}:`, error);
                            }
                        }
                        wsRxBuffer = wsRxBuffer.subarray(totalPacketSize);
                        continue;
                    }
                    else if (type === 14) {
                        if (videoFrameCount === 0 && currentStreamState === "CONNECTING") {
                            setStreamState("STREAM_ACTIVE");
                        }
                        if (wsRxBuffer.length >= 9) {
                            wsRxBuffer = wsRxBuffer.slice(9);
                        } else {
                            wsRxBuffer = new Uint8Array(0);
                        }
                        continue;
                    }
                    else {
                        console.debug("[WS] Unknown packet type:", type);
                        wsRxBuffer = wsRxBuffer.slice(1);
                        continue;
                    }
                }
            } finally {
                wsRxProcessing = false;
            }
        }


        /* ============================================================
           INITIALIZATION PACKET
        ============================================================ */

        function sendInitializationPacket() {
            if (!isSocketOpen()) {
                return;
            }

            const cleanId = String(DEVICE_ID).replace(/[^0-9a-zA-Z_\-]/g, '');
            const idBytes = new TextEncoder().encode(cleanId);

            // Protocol: [1 byte Type = 2] + [N bytes System ID] + [32 bytes Auth Hash]
            const pkt = new Uint8Array(1 + idBytes.length + 32);
            pkt[0] = 2;
            pkt.set(idBytes, 1);
            // [1 + idBytes.length .. end] is 32 bytes of zeros for default pin/auth

            ws.send(pkt);
            if (/^\d{9}$/.test(REQUESTER_SYSTEM_ID)) {
                const identityPacket = new Uint8Array(10);
                identityPacket[0] = 32;
                identityPacket.set(new TextEncoder().encode(REQUESTER_SYSTEM_ID), 1);
                try {
                    ws.send(identityPacket);
                } catch (error) {
                    console.error("[REVERSE] Failed to send the peer identity:", error);
                }
            } else {
                console.warn("[REVERSE] Viewer identity unavailable; this session cannot be reversed.");
            }

            console.log(
                "[WS] Viewer handshake sent for host:",
                cleanId,
                "Packet length:",
                pkt.length
            );
        }

        function resetWebSocketState() {
            wsRxBuffer = new Uint8Array(0);
            websocketAuthenticated = false;
        }

        function fallbackToRelay() {
            if (!directConnection || directFallbackAttempted || websocketAuthenticated || !isStreaming) {
                return false;
            }

            directFallbackAttempted = true;
            directConnection = false;
            console.warn("[DIRECT] Connection failed");
            console.log("[DIRECT] Falling back to relay");

            const failedSocket = ws;
            ws = null;
            if (failedSocket) {
                failedSocket.onopen = null;
                failedSocket.onmessage = null;
                failedSocket.onerror = null;
                failedSocket.onclose = null;
                try { failedSocket.close(); } catch (e) { }
            }

            resetWebSocketState();
            setHud("CONNECTING...");
            console.log("[RELAY] Connecting to " + WS_URL);
            connectWebSocket(WS_URL, false);
            return true;
        }

        function connectWebSocket(targetUrl, isDirect) {
            directConnection = isDirect;
            resetWebSocketState();

            try {
                ws = new WebSocket(targetUrl);
            } catch (error) {
                console.error("[WS] Creation failed:", error);
                if (!fallbackToRelay()) {
                    stopWebStream();
                }
                return;
            }

            ws.binaryType = "arraybuffer";

            ws.onopen = function () {
                console.log("[AUTH DEBUG] WebSocket OPEN");
                console.log("[WS] OPEN");
                setHud("AUTHENTICATING...");
                console.log("[AUTH DEBUG] Sending auth");
                sendInitializationPacket();
                console.log("[AUTH DEBUG] Auth sent");
                console.log("[AUTH DEBUG] Waiting for authentication response");
            };

            ws.onmessage = handleMessage;

            ws.onerror = function (error) {
                console.error("[WS] Error event fired (point: ws.onerror handler)", error);
                console.error(`[WS] state: isStreaming=${isStreaming} decoderState=${decoderState} videoFrameCount=${videoFrameCount}`);
                if (fallbackToRelay()) {
                    return;
                }
                setHud("CONNECTION ERROR");
            };

            ws.onclose = function (event) {
                stopMicrophone();
                stopLocalChatTyping();
                setRemoteChatTyping(false);
                const closer = event.wasClean ? "BROWSER (local close)" : "REMOTE (relay/agent)";
                console.log(`[AUTH DEBUG] WebSocket CLOSED\ncode: ${event.code}\nreason: ${event.reason || 'none'}\nwasClean: ${event.wasClean}\ninitiatedBy: ${closer}\npoint: ws.onclose handler`);
                console.log("[WS] Closed:", event.code, event.reason);
                console.log(`[WS] state at close: isStreaming=${isStreaming} decoderState=${decoderState} videoFrameCount=${videoFrameCount} rxPackets=${browserPerf.rxPackets}`);
                if (fallbackToRelay()) {
                    return;
                }
                stopWebStream();
                if (reverseTransitionPending) {
                    window.parent.postMessage("end_integrated_session", "*");
                } else if (sessionWasEstablished) {
                    showSessionEnded();
                }
            };
        }


        /* ============================================================
           START STREAM
        ============================================================ */

        async function startWebStream() {
            console.log(`[SESSION] page loaded`);
            console.log(`[SESSION] authenticated user = <?= json_encode($_SESSION['user_id'] ?? null) ?>`);
            console.log(`[SESSION] session id = <?= json_encode(session_id()) ?>`);

            console.log("[AUTH DEBUG] startWebStream called");
            console.log("[AUTH DEBUG] WebSocket created");
            console.log(`[SESSION] target device = <?= json_encode($_GET['id'] ?? null) ?>`);
            console.log(`[SESSION] relay URL = ${WS_URL}`);
            console.log(`[WS] Attempting:\n${WS_URL}`);

            if (isStreaming) {

                return;

            }


            canvas =
                document.getElementById(
                    "remoteCanvas"
                );


            ctx =
                canvas.getContext(
                    "2d",
                    {
                        alpha: false
                    }
                );


            if (!ctx) {

                alert(
                    "Unable to create canvas."
                );

                return;

            }


            videoFrameCount = 0;
            renderWidth = 0;
            renderHeight = 0;
            videoDecodeBusy = false;
            safeCloseImage();
            directConnection = false;
            directFallbackAttempted = false;
            resetWebSocketState();

            if (videoDecoder && videoDecoder.state !== 'closed') {
                try { videoDecoder.close(); } catch (e) { }
                videoDecoder = null;
            }
            decoderState = DecoderState.UNCONFIGURED;
            cachedSPS = null;
            cachedPPS = null;


            canvas.width = 1280;

            canvas.height = 720;


            /*
             * Clear the canvas explicitly.
             */

            ctx.fillStyle = "#000000";

            ctx.fillRect(
                0,
                0,
                canvas.width,
                canvas.height
            );


            isStreaming = true;


            streamBox.style.display =
                "flex";

            if (typeof advToolbar !== 'undefined' && advToolbar) {
                advToolbar.style.display = "flex";
            }


            setHud(
                "CONNECTING..."
            );


            canvas.focus();


            startRenderLoop();


            connectWebSocket(WS_URL, false);
            return;


            ws.onopen =
                function () {

                    console.log("[AUTH DEBUG] WebSocket OPEN");
                    console.log("[WS] OPEN");

                    setHud(
                        "AUTHENTICATING..."
                    );

                    console.log("[AUTH DEBUG] Sending auth");
                    sendInitializationPacket();
                    console.log("[AUTH DEBUG] Auth sent");
                    console.log("[AUTH DEBUG] Waiting for authentication response");

                };


            ws.onmessage =
                handleMessage;


            ws.onerror =
                function (error) {

                    console.error(
                        "[WS] Error event fired (point: ws.onerror handler)",
                        error
                    );
                    console.error(`[WS] state: isStreaming=${isStreaming} decoderState=${decoderState} videoFrameCount=${videoFrameCount}`);

                    setHud(
                        "CONNECTION ERROR"
                    );

                };


            ws.onclose =
                function (event) {

                    // event.wasClean === false + a 1xxx code means the REMOTE (relay/agent) closed
                    // the socket, NOT this browser. A browser-initiated close sets ws.close() itself
                    // (see stopWebStream) and would be wasClean=true with code 1000/1001.
                    const closer = event.wasClean ? "BROWSER (local close)" : "REMOTE (relay/agent)";
                    console.log(`[AUTH DEBUG] WebSocket CLOSED\ncode: ${event.code}\nreason: ${event.reason || 'none'}\nwasClean: ${event.wasClean}\ninitiatedBy: ${closer}\npoint: ws.onclose handler`);
                    console.log(
                        "[WS] Closed:",
                        event.code,
                        event.reason
                    );
                    console.log(`[WS] state at close: isStreaming=${isStreaming} decoderState=${decoderState} videoFrameCount=${videoFrameCount} rxPackets=${browserPerf.rxPackets}`);

                    if (isStreaming) {
                        setStreamState("DISCONNECTED");
                    }

                };

        }


        function stopWebStream() {
            isStreaming = false;
            stopLocalChatTyping();
            setRemoteChatTyping(false);
            stopRenderLoop();
            videoDecodeBusy = false;
            safeCloseImage();

            if (videoDecoder && videoDecoder.state !== 'closed') {
                try { videoDecoder.close(); } catch (e) { }
                videoDecoder = null;
            }
            decoderState = DecoderState.UNCONFIGURED;
            cachedSPS = null;
            cachedPPS = null;

            if (mediaRecorder && mediaRecorder.state === "recording") {
                try { mediaRecorder.stop(); } catch (e) { }
            }

            if (webrtcDataChannel) {
                webrtcDataChannel.close();
                webrtcDataChannel = null;
            }
            if (webrtcPeerConnection) {
                webrtcPeerConnection.close();
                webrtcPeerConnection = null;
            }

            if (ws) {
                try {
                    console.log(`[WS] BROWSER explicitly closing WebSocket (point: stopWebStream). isStreaming=${isStreaming} videoFrameCount=${videoFrameCount}`);
                    ws.onopen = null;
                    ws.onmessage = null;
                    ws.onerror = null;
                    ws.onclose = null;
                    ws.close();
                } catch (e) { }
                ws = null;
            }


            if (canvas && ctx) {

                ctx.clearRect(
                    0,
                    0,
                    canvas.width,
                    canvas.height
                );

            }


            setStreamState("DISCONNECTED");

            if (chatPanel) {
                chatPanel.classList.remove(
                    "open"
                );
            }
        }


        /* ============================================================
           STREAM TOGGLE
        ============================================================ */

        function toggleWebStream() {

            if (isStreaming) {

                stopWebStream();

            } else {

                startWebStream();

            }

        }


        /* ============================================================
           FILE TRANSFER
        ============================================================ */

        async function sendFiles(files) {

            if (!isSocketOpen()) {

                alert(
                    "WebSocket not connected."
                );

                return;

            }

            if (!files || !files.length) {

                return;

            }

            for (
                const file of files
            ) {

                try {

                    await sendSingleFile(file);

                } catch (error) {

                    console.error(
                        "[FILE]",
                        error
                    );

                    alert(
                        `Failed to send "${file.name}".`
                    );

                    return;

                }

            }

        }


        let incomingFiles = {};
        let folderDestinationHandle = null;
        let folderDestinationResolve = null;

        function requestFolderDestination() {
            if (folderDestinationHandle) return Promise.resolve(folderDestinationHandle);
            document.getElementById("folderDestinationDialog").style.display = "flex";
            return new Promise(resolve => {
                folderDestinationResolve = resolve;
            });
        }

        function finishFolderDestination(handle) {
            document.getElementById("folderDestinationDialog").style.display = "none";
            const resolve = folderDestinationResolve;
            folderDestinationResolve = null;
            if (resolve) resolve(handle);
        }

        async function selectFolderDestination() {
            if (!folderDestinationResolve) return;
            try {
                if (!window.isSecureContext || typeof window.showDirectoryPicker !== "function") {
                    throw new Error("This browser does not support safe folder destinations. Use a current browser on HTTPS or localhost.");
                }
                folderDestinationHandle = await window.showDirectoryPicker({ mode: "readwrite" });
                finishFolderDestination(folderDestinationHandle);
            } catch (error) {
                if (error.name !== "AbortError") {
                    console.error("[FILE RX] Could not choose folder destination:", error);
                    window.alert(error.message);
                }
                finishFolderDestination(null);
            }
        }

        function cancelFolderDestination() {
            finishFolderDestination(null);
        }

        async function writeReceivedFolderFile(directoryHandle, relativePath, blob) {
            const parts = validateRelativeTransferPath(relativePath).split("/");
            let parent = directoryHandle;
            for (const part of parts.slice(0, -1)) {
                parent = await parent.getDirectoryHandle(part, { create: true });
            }
            const fileName = parts[parts.length - 1];
            try {
                await parent.getFileHandle(fileName);
                throw new Error(`The destination already contains "${relativePath}".`);
            } catch (error) {
                if (error.name !== "NotFoundError") throw error;
            }
            const fileHandle = await parent.getFileHandle(fileName, { create: true });
            const writable = await fileHandle.createWritable();
            try {
                await writable.write(blob);
                await writable.close();
            } catch (error) {
                await writable.abort();
                throw error;
            }
        }

        const outgoingFileResponses = new Map();
        const outgoingFileErrors = new Map();
        const outgoingFileIds = new Set();

        function waitForFileResponse(transferId, expectedType) {
            return new Promise((resolve, reject) => {
                const timer = setTimeout(() => {
                    outgoingFileResponses.delete(transferId);
                    reject(new Error("Timed out waiting for file transfer response."));
                }, 60000);
                outgoingFileResponses.set(transferId, type => {
                    clearTimeout(timer);
                    if (type === expectedType) resolve(type);
                    else if (type === 25) reject(new Error("The receiver rejected the file."));
                    else reject(new Error(outgoingFileErrors.get(transferId) || `Unexpected file response: ${type}`));
                });
            });
        }

        function sendFileControl(type, transferId) {
            const packet = new Uint8Array(9);
            packet[0] = type;
            new DataView(packet.buffer).setBigUint64(1, BigInt(transferId), false);
            ws.send(packet);
        }

        function sendFileError(transferId, message) {
            const messageBytes = new TextEncoder().encode(message);
            const packet = new Uint8Array(13 + messageBytes.length);
            packet[0] = 24;
            new DataView(packet.buffer).setBigUint64(1, BigInt(transferId), false);
            new DataView(packet.buffer).setUint16(11, messageBytes.length, false);
            packet.set(messageBytes, 13);
            ws.send(packet);
        }

        function addTransferUI(transferId, filename, isUpload) {
            const feed = document.getElementById("transferFeed");
            const el = document.createElement("div");
            el.id = `transfer-${transferId}`;
            el.style = "padding: 8px; background: #2a2d35; border-radius: 4px; border-left: 3px solid #007bff;";
            el.innerHTML = `
                <div style="display:flex; justify-content:space-between; margin-bottom:4px;">
                    <span id="transfer-name-${transferId}" style="font-weight:600; text-overflow:ellipsis; overflow:hidden; white-space:nowrap; max-width:180px;"></span>
                    <span id="transfer-pct-${transferId}">0%</span>
                </div>
                <div style="background:#1a1d24; height:4px; border-radius:2px; overflow:hidden;">
                    <div id="transfer-bar-${transferId}" style="background:#007bff; width:0%; height:100%; transition:width 0.1s;"></div>
                </div>
                <div id="transfer-status-${transferId}" style="margin-top:4px; color:#aaa;">Starting...</div>
            `;
            el.querySelector(`#transfer-name-${transferId}`).textContent = `${isUpload ? "↗" : "↙"} ${filename}`;
            feed.prepend(el);
        }

        function updateTransferUI(transferId, pct, status) {
            const pctEl = document.getElementById(`transfer-pct-${transferId}`);
            const barEl = document.getElementById(`transfer-bar-${transferId}`);
            const statEl = document.getElementById(`transfer-status-${transferId}`);
            if (pctEl) pctEl.innerText = `${pct}%`;
            if (barEl) barEl.style.width = `${pct}%`;
            if (statEl) statEl.innerText = status;
        }

        async function handleFilePacket(buffer) {
            const bytes = new Uint8Array(buffer);
            if (bytes.length < 9) {
                console.error(`[FILE RX FRAMING ERROR] packet_bytes=${bytes.length} expected_min=9`);
                return;
            }
            const type = bytes[0];
            const view = new DataView(buffer);
            const transferId = view.getBigUint64(1, false).toString();

            if (type === 25 || type === 26 || type === 27) {
                const resolve = outgoingFileResponses.get(transferId);
                if (resolve) {
                    outgoingFileResponses.delete(transferId);
                    resolve(type);
                }
                if (type === 27) {
                    updateTransferUI(transferId, 100, "Complete");
                    const bar = document.getElementById(`transfer-bar-${transferId}`);
                    if (bar) bar.style.background = "#28a745";
                }
                return;
            }

            if (type === 20) {
                if (bytes.length < 19) {
                    console.error(`[FILE RX FRAMING ERROR] type=20 packet_bytes=${bytes.length} expected_min=19`);
                    sendFileControl(25, transferId);
                    return;
                }
                const fileSize = view.getBigUint64(9, false);
                const nameLen = view.getUint16(17, false);
                if (bytes.length !== 19 + nameLen || fileSize > BigInt(Number.MAX_SAFE_INTEGER)) {
                    console.error(`[FILE RX FRAMING ERROR] type=20 packet_bytes=${bytes.length} filename_len=${nameLen} size=${fileSize}`);
                    sendFileControl(25, transferId);
                    return;
                }
                const nameBytes = bytes.subarray(19, 19 + nameLen);
                let filename;
                try {
                    filename = validateRelativeTransferPath(new TextDecoder("utf-8", { fatal: true }).decode(nameBytes));
                } catch (error) {
                    console.error("[FILE RX] Rejected unsafe offered path:", error);
                    sendFileControl(25, transferId);
                    return;
                }

                const directoryOffer = filename.includes("/");
                const destinationHandle = directoryOffer ? await requestFolderDestination() : null;
                if (directoryOffer && !destinationHandle) {
                    sendFileControl(25, transferId);
                    addTransferUI(transferId, filename, false);
                    updateTransferUI(transferId, 0, "Folder destination cancelled");
                    return;
                }

                if (!window.confirm(`Accept "${filename}" (${Number(fileSize)} bytes)?`)) {
                    sendFileControl(25, transferId);
                    addTransferUI(transferId, filename, false);
                    updateTransferUI(transferId, 0, "Rejected");
                    return;
                }
                incomingFiles[transferId] = {
                    filename: filename,
                    size: Number(fileSize),
                    chunks: [],
                    receivedBytes: 0,
                    directoryHandle: destinationHandle
                };
                console.log(`[FILE] Incoming file start: ${filename}`);
                addTransferUI(transferId, filename, false);
                updateTransferUI(transferId, 0, "Receiving...");
                sendFileControl(26, transferId);
            } else if (type === 21) {
                if (bytes.length < 17) {
                    console.error(`[FILE RX FRAMING ERROR] type=21 packet_bytes=${bytes.length} expected_min=17`);
                    return;
                }
                const chunkLen = view.getUint32(13, false);
                if (chunkLen > 10 * 1024 * 1024 || bytes.length !== 17 + chunkLen) {
                    console.error(`[FILE RX FRAMING ERROR] type=21 packet_bytes=${bytes.length} declared_chunk_len=${chunkLen}`);
                    sendFileError(transferId, "Invalid file chunk framing");
                    delete incomingFiles[transferId];
                    return;
                }
                if (!incomingFiles[transferId]) return;
                const incoming = incomingFiles[transferId];
                if (incoming.receivedBytes + chunkLen > incoming.size) {
                    sendFileError(transferId, "Received more data than the offered file size");
                    updateTransferUI(transferId, 0, "Verification failed");
                    delete incomingFiles[transferId];
                    return;
                }
                const payload = bytes.slice(17);
                incoming.chunks.push(payload);
                incoming.receivedBytes += chunkLen;

                const pct = incoming.size === 0
                    ? 0
                    : Math.floor((incoming.receivedBytes / incoming.size) * 100);
                updateTransferUI(transferId, pct, "Receiving...");
            } else if (type === 22) {
                if (bytes.length !== 49) {
                    console.error(`[FILE RX FRAMING ERROR] type=22 packet_bytes=${bytes.length} expected=49`);
                    sendFileError(transferId, "Invalid file completion framing");
                    delete incomingFiles[transferId];
                    return;
                }
                if (!incomingFiles[transferId]) return;
                const incoming = incomingFiles[transferId];
                const finalSize = Number(view.getBigUint64(9, false));
                const expectedHash = bytes.slice(17, 49);
                const blob = new Blob(incoming.chunks);
                const actualHash = new Uint8Array(await crypto.subtle.digest("SHA-256", await blob.arrayBuffer()));
                const valid = incoming.receivedBytes === incoming.size
                    && finalSize === incoming.size
                    && actualHash.every((byte, index) => byte === expectedHash[index]);
                if (!valid) {
                    sendFileError(transferId, "File size or SHA-256 verification failed");
                    updateTransferUI(transferId, 0, "Verification failed");
                    const bar = document.getElementById(`transfer-bar-${transferId}`);
                    if (bar) bar.style.background = "#dc3545";
                    delete incomingFiles[transferId];
                    return;
                }
                console.log(`[FILE] Transfer verified: ${incoming.filename}`);
                try {
                    if (incoming.directoryHandle) {
                        await writeReceivedFolderFile(incoming.directoryHandle, incoming.filename, blob);
                    } else {
                        const url = URL.createObjectURL(blob);
                        const a = document.createElement("a");
                        a.href = url;
                        a.download = incoming.filename;
                        document.body.appendChild(a);
                        a.click();
                        document.body.removeChild(a);
                        setTimeout(() => URL.revokeObjectURL(url), 1000);
                    }
                    updateTransferUI(transferId, 100, "Complete");
                    sendFileControl(27, transferId);
                } catch (error) {
                    console.error("[FILE RX] Could not save verified file:", error);
                    sendFileError(transferId, `Could not save file: ${error.message}`);
                    updateTransferUI(transferId, 0, "Save failed");
                    const bar = document.getElementById(`transfer-bar-${transferId}`);
                    if (bar) bar.style.background = "#dc3545";
                }
                delete incomingFiles[transferId];
            } else if (type === 23 || type === 24) {
                const message = type === 24
                    ? new TextDecoder().decode(bytes.subarray(13))
                    : "The receiver cancelled the transfer.";
                if (incomingFiles[transferId]) {
                    console.log(`[FILE] Transfer cancelled/error: ${incomingFiles[transferId].filename}`);
                    updateTransferUI(transferId, 0, type === 23 ? "Cancelled" : `Error: ${message}`);
                    const bar = document.getElementById(`transfer-bar-${transferId}`);
                    if (bar) bar.style.background = "#dc3545";
                    delete incomingFiles[transferId];
                }
                if (outgoingFileIds.has(transferId)) {
                    outgoingFileErrors.set(transferId, message);
                    updateTransferUI(transferId, 0, type === 23 ? "Cancelled" : `Error: ${message}`);
                    const bar = document.getElementById(`transfer-bar-${transferId}`);
                    if (bar) bar.style.background = "#dc3545";
                }
                const resolve = outgoingFileResponses.get(transferId);
                if (resolve) {
                    outgoingFileResponses.delete(transferId);
                    resolve(type);
                }
            }
        }

        async function sendSingleFile(file) {
            const filename = validateRelativeTransferPath(file.webkitRelativePath || file.name);
            const nameBytes = new TextEncoder().encode(filename);
            if (nameBytes.length > 4096) throw new Error("Filename too long.");

            const transferIdBytes = new Uint8Array(8);
            crypto.getRandomValues(transferIdBytes);
            const transferIdStr = new DataView(transferIdBytes.buffer).getBigUint64(0, false).toString();
            outgoingFileErrors.delete(transferIdStr);
            outgoingFileIds.add(transferIdStr);
            let offerSent = false;
            let acceptedByReceiver = false;

            try {
            addTransferUI(transferIdStr, filename, true);
            updateTransferUI(transferIdStr, 0, "Waiting for receiver...");

            // TYPE 20: 1 + 8 + 8 + 2 + nameBytes.length = 19 + nameBytes.length
            const metaPkt = new Uint8Array(19 + nameBytes.length);
            metaPkt[0] = 20;
            metaPkt.set(transferIdBytes, 1);
            const metaView = new DataView(metaPkt.buffer);
            metaView.setBigUint64(9, BigInt(file.size), false);
            metaView.setUint16(17, nameBytes.length, false);
            metaPkt.set(nameBytes, 19);
            const accepted = waitForFileResponse(transferIdStr, 26);
            ws.send(metaPkt);
            offerSent = true;
            console.info(`[FILE TX OFFER] direction=A->B transfer_id=${transferIdStr} filename=${JSON.stringify(filename)} total_bytes=${file.size}`);
            await accepted;
            acceptedByReceiver = true;
            console.info(`[FILE TX ACCEPT] direction=A->B transfer_id=${transferIdStr}`);
            updateTransferUI(transferIdStr, 0, "Sending...");

            // Calculate Hash
            const hashBuffer = await crypto.subtle.digest("SHA-256", await file.arrayBuffer());
            const hashBytes = new Uint8Array(hashBuffer);

            // TYPE 21
            const chunkSize = 256 * 1024; // 256 KB
            let offset = 0;
            let chunkIndex = 0;
            console.info(`[FILE TX START] direction=A->B transfer_id=${transferIdStr} chunk_size=${chunkSize}`);

            while (offset < file.size) {
                if (outgoingFileErrors.has(transferIdStr)) {
                    const message = outgoingFileErrors.get(transferIdStr);
                    outgoingFileErrors.delete(transferIdStr);
                    throw new Error(message);
                }
                if (!isSocketOpen()) {
                    updateTransferUI(transferIdStr, 0, "Failed: Disconnected");
                    document.getElementById(`transfer-bar-${transferIdStr}`).style.background = "#dc3545";
                    throw new Error("WebSocket disconnected.");
                }
                const end = Math.min(offset + chunkSize, file.size);
                const chunkBytes = new Uint8Array(await file.slice(offset, end).arrayBuffer());

                // TYPE 21: 1 + 8 + 4 + 4 + chunkBytes.length = 17 + chunkBytes.length
                const pkt = new Uint8Array(17 + chunkBytes.length);
                pkt[0] = 21;
                pkt.set(transferIdBytes, 1);
                const view = new DataView(pkt.buffer);
                view.setUint32(9, chunkIndex, false);
                view.setUint32(13, chunkBytes.length, false);
                pkt.set(chunkBytes, 17);
                ws.send(pkt);

                offset += chunkBytes.length;
                chunkIndex++;

                const pct = Math.floor((offset / file.size) * 100);
                updateTransferUI(transferIdStr, pct, "Sending...");

                // Yield to allow UI updates and prevent blocking
                await new Promise(r => setTimeout(r, 10));
            }

            // TYPE 22: 1 + 8 + 8 + 32 = 49
            const endPkt = new Uint8Array(49);
            endPkt[0] = 22;
            endPkt.set(transferIdBytes, 1);
            const endView = new DataView(endPkt.buffer);
            endView.setBigUint64(9, BigInt(file.size), false);
            endPkt.set(hashBytes, 17);
            if (outgoingFileErrors.has(transferIdStr)) {
                const message = outgoingFileErrors.get(transferIdStr);
                outgoingFileErrors.delete(transferIdStr);
                throw new Error(message);
            }
            const completed = waitForFileResponse(transferIdStr, 27);
            ws.send(endPkt);
            console.info(`[FILE TX END] direction=A->B transfer_id=${transferIdStr} chunks=${chunkIndex} total_bytes=${offset} sha256=${Array.from(hashBytes, byte => byte.toString(16).padStart(2, "0")).join("")}`);
            await completed;
            console.info(`[FILE TX COMPLETE_ACK] direction=A->B transfer_id=${transferIdStr}`);
            updateTransferUI(transferIdStr, 100, "Complete");
            document.getElementById(`transfer-bar-${transferIdStr}`).style.background = "#28a745";
            } catch (error) {
                const message = error instanceof Error ? error.message : String(error);
                if (isSocketOpen() && offerSent) {
                    try {
                        if (acceptedByReceiver) {
                            sendFileError(transferIdStr, message);
                        } else {
                            sendFileControl(23, transferIdStr);
                        }
                    } catch (signalError) {
                        console.error(`[FILE] Failed to signal transfer failure for ${transferIdStr}:`, signalError);
                    }
                }
                updateTransferUI(transferIdStr, 0, `Failed: ${message}`);
                const bar = document.getElementById(`transfer-bar-${transferIdStr}`);
                if (bar) bar.style.background = "#dc3545";
                throw error;
            } finally {
                outgoingFileIds.delete(transferIdStr);
                outgoingFileErrors.delete(transferIdStr);
            }
        }


        /* ============================================================
           MOUSE COORDINATES
        ============================================================ */

        function getCoordinates(event) {
            if (!canvas) {
                return null;
            }

            const canvasWidth = canvas.width || 1920;
            const canvasHeight = canvas.height || 1080;

            const rect = canvas.getBoundingClientRect();
            if (rect.width <= 0 || rect.height <= 0) {
                return null;
            }

            /*
             * Canvas uses object-fit: contain.
             * Calculate displayed video rect inside canvas letterbox.
             */
            const scale = Math.min(
                rect.width / canvasWidth,
                rect.height / canvasHeight
            );

            const displayedWidth = canvasWidth * scale;
            const displayedHeight = canvasHeight * scale;

            const offsetX = (rect.width - displayedWidth) / 2;
            const offsetY = (rect.height - displayedHeight) / 2;

            let x = event.clientX - rect.left - offsetX;
            let y = event.clientY - rect.top - offsetY;

            x = Math.max(0, Math.min(displayedWidth, x));
            y = Math.max(0, Math.min(displayedHeight, y));

            const canvasX = x / scale;
            const canvasY = y / scale;

            const remoteX = Math.max(0, Math.min(canvasWidth - 1, Math.floor(canvasX)));
            const remoteY = Math.max(0, Math.min(canvasHeight - 1, Math.floor(canvasY)));

            const normalizedX = Math.max(0, Math.min(65535, Math.floor((remoteX / (canvasWidth - 1)) * 65535)));
            const normalizedY = Math.max(0, Math.min(65535, Math.floor((remoteY / (canvasHeight - 1)) * 65535)));

            return {
                x: normalizedX,
                y: normalizedY,
                remoteX: remoteX,
                remoteY: remoteY
            };
        }

        /* ============================================================
           MOUSE MOVE
        ============================================================ */

        function sendMouseMove(event) {
            if (!isSocketOpen()) {
                return;
            }

            const coords = getCoordinates(event);
            if (!coords) {
                return;
            }

            console.log(`[INPUT MOUSE]\nevent=mousemove\nx=${coords.remoteX}\ny=${coords.remoteY}`);
            console.log(`[BROWSER INPUT] mousemove`);
            console.log(`[BROWSER CONTROL TX] MOUSE_MOVE`);
            console.log(`[CONTROL TX] type=MOUSE_MOVE`);
            console.log(`[CONTROL TX] bytes=9`);
            console.log(`[CONTROL TX] device=${DEVICE_ID}`);

            const pkt = new Uint8Array(9);
            pkt[0] = 0;
            pkt[1] = (coords.x >> 8) & 0xff;
            pkt[2] = coords.x & 0xff;
            pkt[3] = (coords.y >> 8) & 0xff;
            pkt[4] = coords.y & 0xff;
            console.log(`[CONTROL][BROWSER_TX]\ntype=MOUSE_MOVE\nx=${coords.x}\ny=${coords.y}`);

            ws.send(pkt);
        }

        /* ============================================================
           MOUSE DOWN
        ============================================================ */

        function sendMouseDown(event) {
            if (!isSocketOpen()) {
                return;
            }

            const coords = getCoordinates(event);
            if (!coords) {
                return;
            }

            let type = 1; // Left down
            let btnName = "LEFT";
            if (event.button === 2) {
                type = 3; // Right down
                btnName = "RIGHT";
            } else if (event.button === 1) {
                type = 7; // Middle down
                btnName = "MIDDLE";
            }

            console.log(`[INPUT MOUSE]\nevent=mousedown\nbutton=${event.button}\nx=${coords.remoteX}\ny=${coords.remoteY}`);
            console.log(`[BROWSER INPUT] mousedown`);
            console.log(`[BROWSER CONTROL TX] MOUSE_DOWN`);
            console.log(`[CONTROL TX] type=MOUSE_DOWN`);
            console.log(`[CONTROL TX] bytes=9`);
            console.log(`[CONTROL TX] device=${DEVICE_ID}`);

            const pkt = new Uint8Array(9);
            pkt[0] = type;
            pkt[1] = (coords.x >> 8) & 0xff;
            pkt[2] = coords.x & 0xff;
            pkt[3] = (coords.y >> 8) & 0xff;
            pkt[4] = coords.y & 0xff;

            ws.send(pkt);
        }

        /* ============================================================
           MOUSE UP
        ============================================================ */

        function sendMouseUp(event) {
            if (!isSocketOpen()) {
                return;
            }

            const coords = getCoordinates(event);

            let type = 2; // Left up
            let btnName = "LEFT";
            if (event.button === 2) {
                type = 4; // Right up
                btnName = "RIGHT";
            } else if (event.button === 1) {
                type = 8; // Middle up
                btnName = "MIDDLE";
            }

            console.log(`[INPUT MOUSE]\nevent=mouseup\nbutton=${event.button}`);
            console.log(`[BROWSER INPUT] mouseup`);
            console.log(`[BROWSER CONTROL TX] MOUSE_UP`);
            console.log(`[CONTROL TX] type=MOUSE_UP`);
            console.log(`[CONTROL TX] bytes=9`);
            console.log(`[CONTROL TX] device=${DEVICE_ID}`);

            const pkt = new Uint8Array(9);
            pkt[0] = type;
            if (coords) {
                pkt[1] = (coords.x >> 8) & 0xff;
                pkt[2] = coords.x & 0xff;
                pkt[3] = (coords.y >> 8) & 0xff;
                pkt[4] = coords.y & 0xff;
            }

            ws.send(pkt);
        }

        /* ============================================================
           MOUSE WHEEL
        ============================================================ */

        function sendMouseWheel(event) {
            if (!isSocketOpen()) {
                return;
            }

            event.preventDefault();
            console.log(`[INPUT MOUSE]\nevent=wheel\ndeltaX=${event.deltaX}\ndeltaY=${event.deltaY}`);
            console.log(`[BROWSER INPUT] wheel`);
            console.log(`[BROWSER CONTROL TX] MOUSE_WHEEL`);
            console.log(`[CONTROL TX] type=MOUSE_WHEEL`);
            console.log(`[CONTROL TX] bytes=9`);
            console.log(`[CONTROL TX] device=${DEVICE_ID}`);

            const scroll = event.deltaY > 0 ? -120 : 120;
            const pkt = new Uint8Array(9);
            pkt[0] = 9;
            pkt[3] = (scroll >> 8) & 0xff;
            pkt[4] = scroll & 0xff;

            ws.send(pkt);
        }

        // Track active keys to prevent stuck keys on blur/disconnect
        const activeKeys = new Set();

        function releaseAllKeys() {
            if (activeKeys.size === 0) return;
            console.log(`[INPUT KEYBOARD] Releasing ${activeKeys.size} stuck keys due to blur/disconnect`);
            activeKeys.forEach(keyCode => {
                if (isSocketOpen()) {
                    const pkt = new Uint8Array(9);
                    pkt[0] = 6; // KEY_UP
                    pkt[1] = (keyCode >> 24) & 0xff;
                    pkt[2] = (keyCode >> 16) & 0xff;
                    pkt[3] = (keyCode >> 8) & 0xff;
                    pkt[4] = keyCode & 0xff;
                    ws.send(pkt);
                }
            });
            activeKeys.clear();
        }

        window.addEventListener("blur", releaseAllKeys);
        document.addEventListener("visibilitychange", () => {
            if (document.hidden) releaseAllKeys();
        });

        function sendKeyboard(event, type) {
            if (!isSocketOpen()) {
                return;
            }

            event.preventDefault();
            const keyCode = event.keyCode || event.which;
            const typeName = type === 5 ? "KEY_DOWN" : "KEY_UP";

            if (type === 5) {
                activeKeys.add(keyCode);
                console.log(`[INPUT KEYBOARD]\nevent=keydown\nkey=${event.key}\ncode=${event.code}\nkeyCode=${keyCode}\nctrl=${event.ctrlKey}\nshift=${event.shiftKey}\nalt=${event.altKey}`);
                console.log(`[BROWSER INPUT] keydown`);
                console.log(`[BROWSER CONTROL TX] KEY_DOWN`);
            } else {
                activeKeys.delete(keyCode);
                console.log(`[INPUT KEYBOARD]\nevent=keyup\nkey=${event.key}\ncode=${event.code}`);
                console.log(`[BROWSER INPUT] keyup`);
                console.log(`[BROWSER CONTROL TX] KEY_UP`);
            }

            console.log(`[CONTROL TX] type=${typeName}`);
            console.log(`[CONTROL TX] bytes=9`);
            console.log(`[CONTROL TX] device=${DEVICE_ID}`);

            const pkt = new Uint8Array(9);
            pkt[0] = type;
            pkt[1] = (keyCode >> 24) & 0xff;
            pkt[2] = (keyCode >> 16) & 0xff;
            pkt[3] = (keyCode >> 8) & 0xff;
            pkt[4] = keyCode & 0xff;
            console.log(`[CONTROL][BROWSER_TX]\ntype=${typeName}\nkey=${event.key}\ncode=${keyCode}`);

            ws.send(pkt);
        }

        /* ============================================================
           CANVAS EVENTS
        ============================================================ */

        function attachCanvasEvents() {
            if (!canvas) {
                return;
            }

            canvas.setAttribute("tabindex", "0");
            canvas.style.outline = "none";

            canvas.addEventListener("mousemove", sendMouseMove);

            canvas.addEventListener("mousedown", event => {
                canvas.focus();
                sendMouseDown(event);
            });

            canvas.addEventListener("mouseup", sendMouseUp);
            // Also handle mouse leaving canvas while dragging
            canvas.addEventListener("mouseleave", sendMouseUp);

            canvas.addEventListener("contextmenu", event => {
                event.preventDefault();
            });

            canvas.addEventListener("wheel", sendMouseWheel, { passive: false });

            // Attach keyboard events to window for seamless focus retention
            window.addEventListener("keydown", event => {
                if (document.activeElement === chatInput) {
                    return;
                }
                if (!isSocketOpen()) {
                    return;
                }
                sendKeyboard(event, 5);
            });

            window.addEventListener("keyup", event => {
                if (document.activeElement === chatInput) {
                    return;
                }
                if (!isSocketOpen()) {
                    return;
                }
                sendKeyboard(event, 6);
            });
        }


        /* ============================================================
           DRAG & DROP
        ============================================================ */

        function attachDropEvents() {
            if (!streamBox || !dropOverlay) return;

            streamBox.addEventListener(
                "dragover",
                event => {
                    event.preventDefault();
                    dropOverlay.classList.add("active");
                }
            );

            streamBox.addEventListener(
                "dragleave",
                event => {
                    event.preventDefault();
                    dropOverlay.classList.remove("active");
                }
            );

            streamBox.addEventListener(
                "drop",
                async event => {
                    event.preventDefault();
                    dropOverlay.classList.remove("active");

                    if (!isSocketOpen()) {
                        alert("WebSocket not connected.");
                        return;
                    }

                    await sendFiles(event.dataTransfer.files);
                }
            );
        }

        /* ============================================================
           BUTTON EVENTS
        ============================================================ */

        if (audioBtn) {
            audioBtn.addEventListener("click", toggleAudio);
        }
        if (micBtn) {
            micBtn.addEventListener("click", toggleMicrophone);
        }
        document.getElementById("reverseAcceptBtn").addEventListener("click", () => answerReverseRequest(true));
        document.getElementById("reverseRejectBtn").addEventListener("click", () => answerReverseRequest(false));
        window.addEventListener("pagehide", stopMicrophone);

        if (chatBtn) {
            chatBtn.addEventListener("click", toggleChat);
        }

        if (chatClose) {
            chatClose.addEventListener("click", toggleChat);
        }

        if (sendChatBtn) {
            sendChatBtn.addEventListener("click", sendChat);
        }

        if (chatInput) {
            chatInput.addEventListener("input", updateLocalChatTyping);
            chatInput.addEventListener("keydown", event => {
                if (event.key === "Enter") {
                    sendChat();
                }
            });
        }

        if (recordBtn) {
            recordBtn.addEventListener("click", toggleRecording);
        }

        if (monitorSelect) {
            monitorSelect.addEventListener("change", function () {
                switchMonitor(this.value);
            });
        }


        /* ============================================================
           DYNAMIC UI LOGIC
        ============================================================ */

        function switchPanelTab(tabName) {
            const tabSession = document.getElementById("tabSession");
            const tabActivity = document.getElementById("tabActivity");
            const tabTransfers = document.getElementById("tabTransfers");

            const contentSession = document.getElementById("panelContentSession");
            const contentActivity = document.getElementById("panelContentActivity");
            const contentTransfers = document.getElementById("panelContentTransfers");

            // Reset all
            tabSession.classList.remove("active");
            tabActivity.classList.remove("active");
            tabTransfers.classList.remove("active");
            contentSession.style.display = "none";
            contentActivity.style.display = "none";
            contentTransfers.style.display = "none";

            if (tabName === 'session') {
                tabSession.classList.add("active");
                contentSession.style.display = "block";
            } else if (tabName === 'activity') {
                tabActivity.classList.add("active");
                contentActivity.style.display = "block";
            } else if (tabName === 'transfers') {
                tabTransfers.classList.add("active");
                contentTransfers.style.display = "block";
            }
        }

        function showFileTransferChoice() {
            document.getElementById("fileTransferChoiceDialog").style.display = "flex";
        }

        function closeFileTransferChoice() {
            document.getElementById("fileTransferChoiceDialog").style.display = "none";
        }

        function chooseTransferSource(kind) {
            closeFileTransferChoice();
            const input = document.getElementById(kind === "folder" ? "folderUploadInput" : "fileUploadInput");
            if (kind === "folder" && !("webkitdirectory" in input)) {
                window.alert("Folder selection is not supported by this browser. Select files individually or use a supported browser.");
                return;
            }
            input.value = "";
            input.click();
        }

        function validateRelativeTransferPath(path) {
            const normalized = String(path).replace(/\\/g, "/");
            if (!normalized || normalized.startsWith("/") || /^[A-Za-z]:/.test(normalized) || normalized.includes("\0")) {
                throw new Error("The transfer path is not a safe relative path.");
            }
            const parts = normalized.split("/");
            if (parts.some(part => !part || part === "." || part === ".."
                || part.length > 240
                || /[<>:"|?*\x00-\x1f]/.test(part)
                || /[. ]$/.test(part)
                || /^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\.|$)/i.test(part))) {
                throw new Error("The transfer path contains an unsafe component.");
            }
            return parts.join("/");
        }

        function handleFileInput(event) {
            if (event.target.files && event.target.files.length > 0) {
                if (!isSocketOpen()) {
                    alert("WebSocket not connected.");
                    return;
                }
                sendFiles(event.target.files);
                addActivityLog(`Started sending ${event.target.files.length} file(s)`);
            }
            event.target.value = "";
        }

        function endSessionAndRedirect(event) {
            event.preventDefault();
            if (ws && (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING)) {
                ws.close(1000, "Session ended by user");
            } else if (sessionWasEstablished) {
                showSessionEnded();
            }
            addActivityLog("Session manually ended by user.");
        }

        let sessionStartTime = null;
        let sessionTimerInterval = null;

        function startSessionTimer() {
            if (!sessionStartTime) {
                sessionStartTime = Date.now();
            }
            if (sessionTimerInterval) clearInterval(sessionTimerInterval);

            const timerEl = document.getElementById("connTimeVal");
            sessionTimerInterval = setInterval(() => {
                if (currentStreamState === "DISCONNECTED") return;
                const diff = Math.floor((Date.now() - sessionStartTime) / 1000);
                const hrs = String(Math.floor(diff / 3600)).padStart(2, '0');
                const mins = String(Math.floor((diff % 3600) / 60)).padStart(2, '0');
                const secs = String(diff % 60).padStart(2, '0');
                if (timerEl) timerEl.textContent = `${hrs}:${mins}:${secs}`;
            }, 1000);
        }

        function toggleFullscreen() {
            const box = document.documentElement;
            if (!document.fullscreenElement) {
                if (box.requestFullscreen) {
                    box.requestFullscreen();
                } else if (box.webkitRequestFullscreen) {
                    box.webkitRequestFullscreen();
                }
            } else {
                if (document.exitFullscreen) {
                    document.exitFullscreen();
                }
            }
        }

        /* ============================================================
           INITIALIZE & AUTO-CONNECT
        ============================================================ */

        canvas =
            document.getElementById(
                "remoteCanvas"
            );

        attachCanvasEvents();

        attachDropEvents();

        // Start timer
        startSessionTimer();

        // Automatically start remote desktop stream
        if (document.readyState === "loading") {
            document.addEventListener("DOMContentLoaded", startWebStream);
        } else {
            startWebStream();
        }


        /* ============================================================
           CLEANUP
        ============================================================ */

        window.addEventListener(
            "beforeunload",
            () => {

                stopWebStream();

            }
        );


        /* ============================================================
           DEBUG
        ============================================================ */

        console.log(
            "================================="
        );

        console.log(
            "REMOTE VIEWER INITIALIZED"
        );

        console.log(
            "Device ID:",
            DEVICE_ID
        );

        console.log(
            "WebSocket:",
            WS_URL
        );

        console.log(
            "Video protocol:\nTYPE 13 + WIDTH + HEIGHT + H264 SIZE + H264\nDecoder: WebCodecs H.264 (Annex-B->AVCC converted)"
        );



        console.log(
            "================================="
        );

    </script>

</body>

</html>