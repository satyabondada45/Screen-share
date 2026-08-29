<?php
if (session_status() === PHP_SESSION_NONE) {
    session_start();
}

// Require authenticated user session
if (empty($_SESSION['user_id'])) {
    header("Location: ../index.php");
    exit();
}

$dbPath = __DIR__ . '/../../backend/config/database.php';
if (!isset($pdo) || $pdo === null) {
    if (file_exists($dbPath)) {
        require $dbPath;
    }
}

$deviceUid = $_GET['id'] ?? null;
$sessionToken = $_GET['token'] ?? null;

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
$sessionCode = strlen($cleanId) === 9
    ? substr($cleanId, 0, 3) . ' ' . substr($cleanId, 3, 3) . ' ' . substr($cleanId, 6, 3)
    : (strlen($cleanId) > 3 ? substr($cleanId, 0, 3) . '-' . substr($cleanId, 3) : $cleanId);

// ─── Relay Host Resolution ────────────────────────────────────────────────────
// 1. Try explicitly configured host
$relayConfigPath = __DIR__ . '/../../backend/config/relay.php';
if (file_exists($relayConfigPath)) {
    require_once $relayConfigPath;
}

if (defined('RELAY_SERVER_HOST')) {
    $relayServerAddr = RELAY_SERVER_HOST;
} else {
    // 2. Dynamic discovery from web server
    $relayServerAddr = $_SERVER['SERVER_ADDR'] ?? '';
    if (empty($relayServerAddr) || $relayServerAddr === '::1' || $relayServerAddr === '127.0.0.1') {
        $httpHost = $_SERVER['HTTP_HOST'] ?? 'localhost';
        $relayServerAddr = strtok($httpHost, ':');
    }
}
$relayWsHost = $relayServerAddr; // PHP string injected into JS
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
            z-index: 50;
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

    <div class="viewer-header">
        <div class="header-left">
            <a href="../dashboard.php" class="logo">
                <svg width="24" height="24" viewBox="0 0 32 32" fill="none">
                    <path d="M6 16L16 6L20 10L12 18L6 16Z" fill="#ef4444" />
                    <path d="M12 22L22 12L26 16L18 24L12 22Z" fill="#dc2626" />
                    <path d="M16 28L26 18L30 22L20 32L16 28Z" fill="#b91c1c" />
                </svg>
                DeskStream
            </a>
            <div class="device-info">
                <div class="device-name">
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                        <rect x="2" y="3" width="20" height="14" rx="2" ry="2"></rect>
                        <line x1="8" y1="21" x2="16" y2="21"></line>
                        <line x1="12" y1="17" x2="12" y2="21"></line>
                    </svg>
                    <?= htmlspecialchars($deviceName, ENT_QUOTES, 'UTF-8') ?>
                </div>
                <div class="device-meta">
                    Full Control <span style="color:#d1d5db;">|</span>
                    <span id="headerDeviceIP"><?= htmlspecialchars($device['ip_address'] ?? '127.0.0.1') ?></span> <span style="color:#d1d5db;">|</span>
                    <span class="dot" id="headerConnDot"></span> <span class="dot-text" id="headerConnText">Connecting...</span>
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
                <button class="btn" id="fileBtn" onclick="document.getElementById('fileUploadInput').click()">
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
            <div class="window-controls">
                <span>—</span>
                <span>□</span>
                <span>×</span>
            </div>
        </div>
    </div>

    <div class="stream-container" id="stream-box">
        <div id="hud" class="hud-badge">CONNECTING...</div>

        <div id="dropOverlay" class="drop-overlay">
            📁 Drop files here to send to Remote Host<br>
            <small style="font-size:0.9rem;">Files will be placed in RemoteDrop/</small>
        </div>

        <canvas id="remoteCanvas" tabindex="0"></canvas>

        <div class="floating-controls">
            <button class="floating-btn" id="fullscreenBtn" onclick="toggleFullscreen()">
                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <path
                        d="M8 3H5a2 2 0 0 0-2 2v3m18 0V5a2 2 0 0 0-2-2h-3m0 18h3a2 2 0 0 0 2-2v-3M3 16v3a2 2 0 0 0 2 2h3">
                    </path>
                </svg>
                Screen Fit
            </button>
            <button class="floating-btn" onclick="document.getElementById('sessionPanel').classList.toggle('open')">
                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <circle cx="12" cy="12" r="1"></circle>
                    <circle cx="12" cy="5" r="1"></circle>
                    <circle cx="12" cy="19" r="1"></circle>
                </svg>
                More
            </button>
        </div>

        <!-- Session / Settings Panel -->
        <div class="session-panel" id="sessionPanel">
            <div class="panel-tabs">
                <div class="panel-tab active" id="tabSession" onclick="switchPanelTab('session')">Session</div>
                <div class="panel-tab" id="tabActivity" onclick="switchPanelTab('activity')">Activity</div>
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
                    <span class="info-val"><?= htmlspecialchars($device['ip_address'] ?? '192.168.1.120') ?></span>
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

            <!-- Hidden File Input -->
            <input type="file" id="fileUploadInput" multiple style="display: none;" onchange="handleFileInput(event)">

        </div>

        <!-- Chat Panel -->
        <div class="chat-panel" id="chatPanel">
            <div class="chat-header">
                <span>Session Chat</span>
                <span class="chat-close" id="chatClose">&times;</span>
            </div>
            <div class="chat-messages" id="chatMessages"></div>
            <div class="chat-input">
                <input type="text" id="chatInput" placeholder="Type a message..." autocomplete="off">
                <button type="button" id="sendChatBtn">Send</button>
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

        const WS_PORT = "9001";
        // Get the real relay IP injected from PHP configuration
        const WS_HOST_PHP = <?= json_encode($relayWsHost) ?>;
        
        // Use the configured/detected server IP. We never want "localhost" 
        // if this is a remote viewer.
        const WS_HOST = (WS_HOST_PHP && WS_HOST_PHP !== 'localhost' && WS_HOST_PHP !== '::1') 
            ? WS_HOST_PHP 
            : location.hostname;

        const WS_URL = location.protocol === "https:"
            ? `wss://${WS_HOST}:${WS_PORT}`
            : `ws://${WS_HOST}:${WS_PORT}`;


        /* ============================================================
           STATE
        ============================================================ */

        let ws = null;
        let wsRxBuffer = new Uint8Array(0);

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


        /* AUDIO */

        let audioCtx = null;

        let nextAudioTime = 0;

        let isAudioEnabled = false;


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

                setTimeout(
                    () => chatInput.focus(),
                    100
                );

            }

        }


        function appendMessage(type, text) {

            const div =
                document.createElement("div");

            div.className =
                "msg " + type;

            div.textContent = text;

            chatMessages.appendChild(div);

            chatMessages.scrollTop =
                chatMessages.scrollHeight;

        }


        function sendChat() {

            const msg =
                chatInput.value.trim();

            if (!msg) {

                return;

            }

            if (!isSocketOpen()) {

                appendMessage(
                    "recv",
                    "Not connected to remote host."
                );

                return;

            }

            const msgBytes =
                new TextEncoder().encode(msg);

            if (msgBytes.length > 65535) {

                alert("Message is too long.");

                return;

            }

            const pkt =
                new Uint8Array(
                    3 + msgBytes.length
                );

            pkt[0] = 16;

            pkt[1] =
                (msgBytes.length >> 8) & 0xff;

            pkt[2] =
                msgBytes.length & 0xff;

            pkt.set(
                msgBytes,
                3
            );

            try {

                ws.send(pkt);

                appendMessage(
                    "sent",
                    msg
                );

                chatInput.value = "";

            } catch (error) {

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

            isAudioEnabled =
                !isAudioEnabled;

            if (isAudioEnabled) {

                audioBtn.textContent =
                    "🔊 Mute Audio";

                audioBtn.classList.remove(
                    "btn-secondary"
                );

                audioBtn.classList.add(
                    "btn-warning"
                );

                try {

                    if (!audioCtx) {

                        audioCtx =
                            new (
                                window.AudioContext ||
                                window.webkitAudioContext
                            )();

                    }

                    if (
                        audioCtx.state ===
                        "suspended"
                    ) {

                        await audioCtx.resume();

                    }

                } catch (error) {

                    console.error(
                        "[AUDIO] Init error:",
                        error
                    );

                }

            } else {

                audioBtn.textContent =
                    "🔈 Listen Audio";

                audioBtn.classList.remove(
                    "btn-warning"
                );

                audioBtn.classList.add(
                    "btn-secondary"
                );

                nextAudioTime = 0;

            }

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
            CONFIGURED_WAITING_FOR_KEYFRAME: "CONFIGURED_WAITING_FOR_KEYFRAME",
            DECODING: "DECODING",
            ERROR: "ERROR"
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
            lastLog: performance.now()
        };

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

        function prepareAnnexBKeyframe(dataBytes, sps, pps) {
            const nals = parseH264Nals(dataBytes);
            if (nals.hasSPS && nals.hasPPS) {
                return dataBytes;
            }
            let extraLen = 0;
            if (!nals.hasSPS && sps) extraLen += 4 + sps.length;
            if (!nals.hasPPS && pps) extraLen += 4 + pps.length;

            if (extraLen === 0) return dataBytes;

            const combined = new Uint8Array(extraLen + dataBytes.length);
            let offset = 0;
            if (!nals.hasSPS && sps) {
                combined.set([0, 0, 0, 1], offset);
                offset += 4;
                combined.set(sps, offset);
                offset += sps.length;
            }
            if (!nals.hasPPS && pps) {
                combined.set([0, 0, 0, 1], offset);
                offset += 4;
                combined.set(pps, offset);
                offset += pps.length;
            }
            combined.set(dataBytes, offset);
            return combined;
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
                        if (decoderState === DecoderState.CONFIGURED_WAITING_FOR_KEYFRAME) {
                            decoderState = DecoderState.DECODING;
                            console.log("[DECODER STATE] DECODING (Keyframe decoded)");
                            console.log("[WEBCODECS] KEYFRAME DECODED");
                        }
                        browserPerf.decodedFrames++;
                        videoFrameCount++;

                        console.log(`[WEBCODECS OUTPUT]\nwidth=${frame.displayWidth}\nheight=${frame.displayHeight}\ntimestamp=${frame.timestamp}`);

                        if (videoFrameCount === 1) {
                            console.log("[VIDEO RENDER] first visible frame");
                        }

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
                            }

                            if (!window._display_logged) {
                                console.log(`[DISPLAY]\nframeWidth = ${frame.displayWidth}\nframeHeight = ${frame.displayHeight}\ncanvasWidth = ${canvas.width}\ncanvasHeight = ${canvas.height}`);
                                console.log("[DISPLAY] Frame rendered");
                                window._display_logged = true;
                            }

                            // Temporary Diagnostic: verify if decoded VideoFrame has non-black content
                            if (videoFrameCount <= 3) {
                                try {
                                    const diagCanvas = document.createElement("canvas");
                                    diagCanvas.width = 32;
                                    diagCanvas.height = 32;
                                    const diagCtx = diagCanvas.getContext("2d");
                                    if (diagCtx) {
                                        diagCtx.drawImage(frame, 0, 0, 32, 32);
                                        const pData = diagCtx.getImageData(0, 0, 32, 32).data;
                                        let nonBlack = 0;
                                        for (let i = 0; i < pData.length; i += 4) {
                                            if (pData[i] > 10 || pData[i + 1] > 10 || pData[i + 2] > 10) nonBlack++;
                                        }
                                        console.log(`[VIDEO DIAGNOSTIC] Frame #${videoFrameCount} sample pixels: ${nonBlack} / 1024 non-black`);
                                    }
                                } catch (diagErr) { }
                            }

                            ctx.drawImage(frame, 0, 0, canvas.width, canvas.height);
                            browserPerf.renderedFrames++;
                            console.log(`[VIDEO RENDER]\nframe=${browserPerf.renderedFrames}`);
                            setStreamState("DISPLAYING");

                            if (browserPerf.renderedFrames % 100 === 0) {
                                const now = performance.now();
                                const elapsedSec = (now - browserPerf.lastLog) / 1000.0;
                                const renderFps = elapsedSec > 0 ? (100 / elapsedSec).toFixed(1) : "0.0";
                                browserPerf.lastLog = now;

                                console.log(
                                    `[VIDEO PERFORMANCE] Received: ${browserPerf.rxPackets}, Decoded: ${browserPerf.decodedFrames}, Rendered: ${browserPerf.renderedFrames}, Render FPS: ${renderFps}, Decoder Queue: ${videoDecoder.decodeQueueSize}`
                                );
                            }
                        }
                        frame.close();
                    },
                    error(error) {
                        console.error("[BROWSER DECODE ERROR]", error);
                        decoderState = DecoderState.ERROR;
                        setStreamState("ERROR");
                    }
                });
                
                if (window._rx_count <= 5) {
                    console.log(`[DECODER]\nconfigured = ${videoDecoder ? 'YES' : 'NO'}\nstate = ${decoderState}\ndecodeQueueSize = ${videoDecoder ? videoDecoder.decodeQueueSize : 0}`);
                }
                return true;
            } catch (err) {
                console.error("[VIDEO FATAL] Failed to construct VideoDecoder:", err);
                decoderState = DecoderState.ERROR;
                return false;
            }
        }

        async function handleVideoPacket(buffer) {
            const bytes = new Uint8Array(buffer);
            const length = bytes.length;
            window._rx_count = (window._rx_count || 0) + 1;

            if (length < 21) {
                console.warn("[VIDEO] Packet too small:", length);
                return;
            }

            const view = new DataView(buffer);
            const packetType = view.getUint8(0);

            if (packetType !== 13 && packetType !== 15) {
                console.warn("[VIDEO] Not a video packet:", packetType);
                return;
            }

            const width = view.getUint32(1, false);
            const height = view.getUint32(5, false);
            const payloadSize = view.getUint32(9, false);
            const captureTimestamp = Number(view.getBigUint64(13, false));

            const available = length - 21;

            if (window._rx_count <= 5) {
                console.log(`[VIDEO PARSER]\ntype = ${packetType}\nwidth = ${width}\nheight = ${height}\npayloadSize = ${payloadSize}\navailable = ${available}\npacketSize = ${length}`);
            }

            if (payloadSize <= 0 || payloadSize > available) {
                console.error(
                    "[VIDEO] Invalid H.264 payload size.",
                    { payloadSize, available, packetSize: length }
                );
                return;
            }

            const actualPayloadSize = (payloadSize > 0 && payloadSize <= available) ? payloadSize : available;
            const h264Payload = bytes.slice(21, 21 + actualPayloadSize);

            streamStats.received_packets++;
            streamStats.received_bytes += actualPayloadSize;
            browserPerf.rxPackets++;

            const receiveTime = Date.now();
            const networkLatency = receiveTime - captureTimestamp;
            const decodeQueue = videoDecoder ? videoDecoder.decodeQueueSize : 0;
            
            console.log(`[LATENCY] Capture->Browser: ${networkLatency}ms | DecodeQueue: ${decodeQueue}`);
            console.log(`[BROWSER VIDEO] width=${width} height=${height} h264_size=${actualPayloadSize}`);

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
            
            if (window._rx_count <= 5) {
                console.log(`[VIDEO] SPS received = ${nals.hasSPS ? 'YES' : 'NO'}`);
                console.log(`[VIDEO] PPS received = ${nals.hasPPS ? 'YES' : 'NO'}`);
                console.log(`[VIDEO] IDR received = ${nals.hasIDR ? 'YES' : 'NO'}`);
            }

            if (nals.hasSPS && nals.spsUnit) {
                cachedSPS = nals.spsUnit;
            }
            if (nals.hasPPS && nals.ppsUnit) {
                cachedPPS = nals.ppsUnit;
            }

            // STEP 3: Keyframe State & Evaluation
            const hasSPS = (cachedSPS !== null || nals.hasSPS);
            const hasPPS = (cachedPPS !== null || nals.hasPPS);
            const hasIDR = nals.hasIDR;
            const isKey = (hasIDR || (nals.hasSPS && nals.hasPPS)) && hasSPS && hasPPS;

            const keyDeltaStr = isKey ? "key" : "delta";
            console.log(`[BROWSER H264]\npacket_size=${length}\nreassembled_size=${actualPayloadSize}\nNAL types=[${nals.nalTypes.join(',')}]\nSPS=${hasSPS}\nPPS=${hasPPS}\nIDR=${hasIDR}\nkey/delta=${keyDeltaStr}`);

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

            // Initialize VideoDecoder if needed
            if (!videoDecoder || videoDecoder.state === 'closed' || decoderState === DecoderState.ERROR) {
                if (!initVideoDecoder()) {
                    return;
                }
            }

            // Configure decoder when in UNCONFIGURED state
            if (decoderState === DecoderState.UNCONFIGURED) {
                try {
                    videoDecoder.configure({
                        codec: codecString,
                        codedWidth: width,
                        codedHeight: height,
                        optimizeForLatency: true
                    });
                    decoderState = DecoderState.CONFIGURED_WAITING_FOR_KEYFRAME;
                    setStreamState("WAITING_FOR_KEYFRAME");
                    console.log(`[WEBCODECS CONFIG]\ncodec=${codecString}\nwidth=${width}\nheight=${height}\ndescriptionBytes=0\nformat=AnnexB`);
                } catch (e) {
                    console.error("[BROWSER DECODER CONFIG ERROR]", e);
                    decoderState = DecoderState.ERROR;
                    return;
                }
            }

            // Monotonic timestamp in microseconds
            const timestamp = Math.round(performance.now() * 1000);

            // State Handling: CONFIGURED_WAITING_FOR_KEYFRAME
            if (decoderState === DecoderState.CONFIGURED_WAITING_FOR_KEYFRAME) {
                if (!isKey) {
                    console.warn("[BROWSER VIDEO] Waiting for first keyframe (SPS/PPS/IDR) before decoding delta frames.");
                    return;
                }

                const annexBKeyframe = prepareAnnexBKeyframe(h264Payload, cachedSPS, cachedPPS);
                console.log(`[WEBCODECS DECODE]\ntype=key\ntimestamp=${timestamp}\nbytes=${annexBKeyframe.byteLength}\nNAL types=[${nals.nalTypes.join(',')}]`);

                try {
                    const chunk = new EncodedVideoChunk({
                        type: 'key',
                        timestamp: timestamp,
                        data: annexBKeyframe
                    });
                    videoDecoder.decode(chunk);
                    console.log("[DECODER] First keyframe submitted");
                } catch (e) {
                    console.error("[BROWSER DECODER ERROR] decode(key) exception:", e);
                    decoderState = DecoderState.ERROR;
                }
                return;
            }

            if (decoderState === DecoderState.DECODING) {
                const chunkType = isKey ? 'key' : 'delta';
                const chunkData = isKey ? prepareAnnexBKeyframe(h264Payload, cachedSPS, cachedPPS) : h264Payload;

                // BACKPRESSURE: If the decoder is falling behind, drop stale delta frames to remain near-real-time.
                if (videoDecoder && videoDecoder.decodeQueueSize > 5 && chunkType === 'delta') {
                    console.warn(`[WEBCODECS BACKPRESSURE] Dropping delta frame. Queue size: ${videoDecoder.decodeQueueSize}`);
                    return; // Skip decoding this frame
                }

                console.log(`[WEBCODECS DECODE]\ntype=${chunkType}\ntimestamp=${timestamp}\nbytes=${chunkData.byteLength}\nNAL types=[${nals.nalTypes.join(',')}]`);

                try {
                    const chunk = new EncodedVideoChunk({
                        type: chunkType,
                        timestamp: timestamp,
                        data: chunkData
                    });
                    videoDecoder.decode(chunk);
                } catch (e) {
                    console.error(`[BROWSER DECODER ERROR] decode(${chunkType}) exception:`, e);
                    decoderState = DecoderState.ERROR;
                }
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
                [N bytes audio data]
            */

            const type = new Uint8Array(buffer)[0];

            if (type !== 17) {
                console.warn("[AUDIO] Not an audio packet:", type);
                return;
            }

            if (!isAudioEnabled) {

                return;

            }

            if (buffer.byteLength < 11) {

                console.warn(
                    "[AUDIO] Packet too small."
                );

                return;

            }

            const view =
                new DataView(buffer);

            const byteLen =
                view.getUint32(
                    1,
                    false
                );

            const sampleRate =
                view.getUint32(
                    5,
                    false
                );

            const channels =
                view.getUint16(
                    9,
                    false
                );

            if (
                byteLen <= 0 ||
                sampleRate <= 0 ||
                channels <= 0
            ) {

                return;

            }

            const start = 11;

            const end =
                start + byteLen;

            if (
                end > buffer.byteLength
            ) {

                console.error(
                    "[AUDIO] Invalid audio length."
                );

                return;

            }

            const audioBytes =
                buffer.slice(
                    start,
                    end
                );

            const floatArray =
                new Float32Array(
                    audioBytes
                );

            if (!floatArray.length) {

                return;

            }

            if (!audioCtx) {

                audioCtx =
                    new (
                        window.AudioContext ||
                        window.webkitAudioContext
                    )();

            }

            if (
                audioCtx.state ===
                "suspended"
            ) {

                audioCtx.resume()
                    .catch(console.error);

            }

            const frames =
                Math.floor(
                    floatArray.length /
                    channels
                );

            if (frames <= 0) {

                return;

            }

            const audioBuffer =
                audioCtx.createBuffer(
                    channels,
                    frames,
                    sampleRate
                );

            for (
                let c = 0;
                c < channels;
                c++
            ) {

                const channelData =
                    audioBuffer.getChannelData(c);

                for (
                    let i = 0;
                    i < frames;
                    i++
                ) {

                    channelData[i] =
                        floatArray[
                        i * channels + c
                        ];

                }

            }

            const source =
                audioCtx.createBufferSource();

            source.buffer =
                audioBuffer;

            source.connect(
                audioCtx.destination
            );

            if (
                nextAudioTime <
                audioCtx.currentTime
            ) {

                nextAudioTime =
                    audioCtx.currentTime + .05;

            }

            source.start(
                nextAudioTime
            );

            nextAudioTime +=
                audioBuffer.duration;

        }


        /* ============================================================
           CHAT PACKET
        ============================================================ */

        function handleChatPacket(buffer) {

            if (buffer.byteLength < 3) {

                return;

            }

            const view =
                new DataView(buffer);

            const len =
                (
                    view.getUint8(1) << 8
                ) |
                view.getUint8(2);

            if (
                3 + len >
                buffer.byteLength
            ) {

                return;

            }

            const msgBytes =
                new Uint8Array(
                    buffer,
                    3,
                    len
                );

            const text =
                new TextDecoder().decode(
                    msgBytes
                );

            appendMessage(
                "recv",
                text
            );

            if (
                !chatPanel.classList.contains(
                    "open"
                )
            ) {

                toggleChat();

            }

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

            // Append new chunk to persistent buffer
            const newBuf = new Uint8Array(wsRxBuffer.length + chunk.length);
            newBuf.set(wsRxBuffer);
            newBuf.set(chunk, wsRxBuffer.length);
            wsRxBuffer = newBuf;

            // Process fully formed packets in the buffer
            while (wsRxBuffer.length > 0) {
                const type = wsRxBuffer[0];
                
                if (type === 13 || type === 15) {
                    // Video Packet (Type 13 / 15)
                    // Header: 1 (type) + 4 (width) + 4 (height) + 4 (size) + 8 (timestamp) = 21 bytes
                    if (wsRxBuffer.length < 21) {
                        return; // Wait for full header
                    }
                    const view = new DataView(wsRxBuffer.buffer, wsRxBuffer.byteOffset, wsRxBuffer.byteLength);
                    const payloadSize = view.getUint32(9, false);
                    const totalPacketSize = 21 + payloadSize;

                    if (wsRxBuffer.length < totalPacketSize) {
                        return; // Wait for full payload
                    }

                    const packetBuffer = wsRxBuffer.buffer.slice(wsRxBuffer.byteOffset, wsRxBuffer.byteOffset + totalPacketSize);
                    await handleVideoPacket(packetBuffer);
                    
                    wsRxBuffer = wsRxBuffer.slice(totalPacketSize);
                    continue;
                }
                else if (type === 17) {
                    // Audio Packet (Type 17)
                    // Header: 1 (type) + 4 (size) + 4 (rate) + 2 (channels) = 11 bytes
                    if (wsRxBuffer.length < 11) {
                        return;
                    }
                    const view = new DataView(wsRxBuffer.buffer, wsRxBuffer.byteOffset, wsRxBuffer.byteLength);
                    const payloadSize = view.getUint32(1, false);
                    const totalPacketSize = 11 + payloadSize;

                    if (wsRxBuffer.length < totalPacketSize) {
                        return;
                    }

                    const packetBuffer = wsRxBuffer.buffer.slice(wsRxBuffer.byteOffset, wsRxBuffer.byteOffset + totalPacketSize);
                    handleAudioPacket(packetBuffer);
                    
                    wsRxBuffer = wsRxBuffer.slice(totalPacketSize);
                    continue;
                }
                else if (type === 16) {
                    // Chat Packet (Type 16)
                    // Header: 1 (type) + 2 (size) = 3 bytes
                    if (wsRxBuffer.length < 3) {
                        return;
                    }
                    const view = new DataView(wsRxBuffer.buffer, wsRxBuffer.byteOffset, wsRxBuffer.byteLength);
                    const payloadSize = (view.getUint8(1) << 8) | view.getUint8(2);
                    const totalPacketSize = 3 + payloadSize;

                    if (wsRxBuffer.length < totalPacketSize) {
                        return;
                    }

                    const packetBuffer = wsRxBuffer.buffer.slice(wsRxBuffer.byteOffset, wsRxBuffer.byteOffset + totalPacketSize);
                    handleChatPacket(packetBuffer);
                    
                    wsRxBuffer = wsRxBuffer.slice(totalPacketSize);
                    continue;
                }
                else if (type === 2) {
                    // Stream Active
                    setStreamState("STREAM_ACTIVE");
                    wsRxBuffer = wsRxBuffer.slice(1);
                    continue;
                }
                else if (type === 14) {
                    // Heartbeat
                    if (videoFrameCount === 0 && currentStreamState === "CONNECTING") {
                        setStreamState("STREAM_ACTIVE");
                    }
                    wsRxBuffer = wsRxBuffer.slice(1);
                    continue;
                }
                else {
                    // Unknown packet. Discard the first byte and attempt to resync.
                    console.debug("[WS] Unknown packet type:", type);
                    wsRxBuffer = wsRxBuffer.slice(1);
                    continue;
                }
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

            console.log(
                "[WS] Viewer handshake sent for host:",
                cleanId,
                "Packet length:",
                pkt.length
            );
        }


        /* ============================================================
           START STREAM
        ============================================================ */

        function startWebStream() {
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


            try {

                ws =
                    new WebSocket(
                        WS_URL
                    );

            } catch (error) {

                console.error(
                    "[WS] Creation failed:",
                    error
                );

                stopWebStream();

                alert(
                    "Could not create WebSocket."
                );

                return;

            }


            ws.binaryType =
                "arraybuffer";


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
                        "[WS] Error:",
                        error
                    );

                    setHud(
                        "CONNECTION ERROR"
                    );

                };


            ws.onclose =
                function (event) {

                    console.log(`[AUTH DEBUG] WebSocket CLOSED\ncode: ${event.code}\nreason: ${event.reason || 'none'}\nwasClean: ${event.wasClean}`);
                    console.log(
                        "[WS] Closed:",
                        event.code,
                        event.reason
                    );

                    if (isStreaming) {

                        setHud(
                            "DISCONNECTED"
                        );

                    }

                };

        }


        /* ============================================================
           STOP STREAM
        ============================================================ */

        function stopWebStream() {

            isStreaming = false;

            stopRenderLoop();

            videoDecodeBusy = false;

            safeCloseImage();

            renderWidth = 0;
            renderHeight = 0;

            if (videoDecoder && videoDecoder.state !== 'closed') {
                try { videoDecoder.close(); } catch (e) { }
                videoDecoder = null;
            }
            decoderState = DecoderState.UNCONFIGURED;
            cachedSPS = null;
            cachedPPS = null;


            if (
                mediaRecorder &&
                mediaRecorder.state ===
                "recording"
            ) {

                try {

                    mediaRecorder.stop();

                } catch (e) { }

            }


            if (ws) {

                try {

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


            setHud(
                "DISCONNECTED"
            );

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


        async function sendSingleFile(file) {

            const nameBytes =
                new TextEncoder().encode(
                    file.name
                );

            if (
                nameBytes.length > 65535
            ) {

                throw new Error(
                    "Filename too long."
                );

            }


            /*
             * TYPE 20
             */

            const metaPkt =
                new Uint8Array(
                    11 + nameBytes.length
                );

            metaPkt[0] = 20;

            metaPkt[1] =
                (nameBytes.length >> 8) & 0xff;

            metaPkt[2] =
                nameBytes.length & 0xff;

            const metaView =
                new DataView(
                    metaPkt.buffer
                );

            metaView.setBigUint64(
                3,
                BigInt(file.size),
                false
            );

            metaPkt.set(
                nameBytes,
                11
            );

            ws.send(metaPkt);


            /*
             * TYPE 21
             */

            const chunkSize =
                32768;

            let offset = 0;

            while (
                offset < file.size
            ) {

                if (!isSocketOpen()) {

                    throw new Error(
                        "WebSocket disconnected."
                    );

                }

                const end =
                    Math.min(
                        offset + chunkSize,
                        file.size
                    );

                const slice =
                    file.slice(
                        offset,
                        end
                    );

                const arrayBuffer =
                    await slice.arrayBuffer();

                const chunkBytes =
                    new Uint8Array(
                        arrayBuffer
                    );

                const pkt =
                    new Uint8Array(
                        5 + chunkBytes.length
                    );

                pkt[0] = 21;

                const view =
                    new DataView(
                        pkt.buffer
                    );

                view.setUint32(
                    1,
                    chunkBytes.length,
                    false
                );

                pkt.set(
                    chunkBytes,
                    5
                );

                ws.send(pkt);

                offset = end;

            }

            alert(
                `File "${file.name}" sent successfully.`
            );

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

        /* ============================================================
           KEYBOARD
        ============================================================ */

        function sendKeyboard(event, type) {
            if (!isSocketOpen()) {
                return;
            }

            event.preventDefault();
            const keyCode = event.keyCode || event.which;
            const typeName = type === 5 ? "KEY_DOWN" : "KEY_UP";

            if (type === 5) {
                console.log(`[INPUT KEYBOARD]\nevent=keydown\nkey=${event.key}\ncode=${event.code}\nkeyCode=${keyCode}\nctrl=${event.ctrlKey}\nshift=${event.shiftKey}\nalt=${event.altKey}`);
                console.log(`[BROWSER INPUT] keydown`);
                console.log(`[BROWSER CONTROL TX] KEY_DOWN`);
            } else {
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
            const contentSession = document.getElementById("panelContentSession");
            const contentActivity = document.getElementById("panelContentActivity");

            if (tabName === 'session') {
                tabSession.classList.add("active");
                tabActivity.classList.remove("active");
                contentSession.style.display = "block";
                contentActivity.style.display = "none";
            } else if (tabName === 'activity') {
                tabActivity.classList.add("active");
                tabSession.classList.remove("active");
                contentActivity.style.display = "block";
                contentSession.style.display = "none";
            }
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
            stopWebStream();
            addActivityLog("Session manually ended by user.");
            setTimeout(() => {
                window.location.href = "../dashboard.php";
            }, 500);
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