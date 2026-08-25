<?php
require_once __DIR__ . '/../../backend/config/database.php';

$deviceUid = $_GET['id'] ?? null;

if (!$deviceUid) {
    header("Location: ../devices.php");
    exit;
}

$stmt = $pdo->prepare("
    SELECT *
    FROM devices
    WHERE device_uid = :device_uid
    LIMIT 1
");

$stmt->execute([
    ':device_uid' => $deviceUid
]);

$device = $stmt->fetch(PDO::FETCH_ASSOC);

if (!$device) {
    die("Error: Device not found in database.");
}

$deviceName = $device['name'] ?? 'Unknown Device';
$isOnline = !empty($device['is_online']);

$sessionCode = strlen($deviceUid) > 3
    ? substr($deviceUid, 0, 3) . '-' . substr($deviceUid, 3)
    : $deviceUid;
?>

<!DOCTYPE html>
<html lang="en">

<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">

    <title>
        Active Remote Session -
        <?= htmlspecialchars($deviceName, ENT_QUOTES, 'UTF-8') ?>
    </title>

    <style>
        :root {
            --bg: #0b0f19;
            --card-bg: #1e293b;
            --border: #334155;
            --accent: #3b82f6;
            --accent-hover: #2563eb;
            --text-main: #f8fafc;
            --text-muted: #94a3b8;
            --online: #22c55e;
            --offline: #64748b;
            --danger: #ef4444;
            --success: #16a34a;
            --warning: #eab308;
        }

        * {
            box-sizing: border-box;
            margin: 0;
            padding: 0;
            font-family:
                -apple-system,
                BlinkMacSystemFont,
                "Segoe UI",
                Roboto,
                sans-serif;
        }

        body {
            background: var(--bg);
            color: var(--text-main);
            min-height: 100vh;
            display: flex;
            flex-direction: column;
        }

        /* =========================
           TOP BAR
        ========================= */

        .topbar {
            height: 60px;
            background: #0f172a;
            border-bottom: 1px solid var(--border);

            display: flex;
            justify-content: space-between;
            align-items: center;

            padding: 0 24px;
            flex-shrink: 0;
        }

        .device-badge {
            display: flex;
            align-items: center;
            gap: 10px;
        }

        .dot {
            width: 10px;
            height: 10px;
            border-radius: 50%;

            background:
                <?= $isOnline
                    ? 'var(--online)'
                    : 'var(--offline)' ?>;

            box-shadow:
                <?= $isOnline
                    ? '0 0 10px var(--online)'
                    : 'none' ?>;
        }

        /* =========================
           MAIN
        ========================= */

        .container {
            max-width: 1200px;
            width: 100%;

            margin: 24px auto;
            padding: 0 20px;

            flex: 1;

            display: flex;
            flex-direction: column;
        }

        .session-card {
            background: var(--card-bg);
            border: 1px solid var(--border);
            border-radius: 14px;

            padding: 24px;

            box-shadow:
                0 10px 25px rgba(0, 0, 0, 0.3);

            display: flex;
            flex-direction: column;

            min-height: calc(100vh - 108px);
        }

        .session-description {
            color: var(--text-muted);
            margin-top: 4px;
        }

        /* =========================
           SESSION CODE
        ========================= */

        .code-display {
            background: #0f172a;
            border: 1px dashed var(--accent);

            border-radius: 10px;

            padding: 14px;

            text-align: center;

            margin: 16px 0;
        }

        .session-code {
            font-family: monospace;
            font-size: 2rem;
            letter-spacing: 4px;

            color: #38bdf8;
            font-weight: 700;
        }

        /* =========================
           BUTTONS
        ========================= */

        .btn {
            display: inline-flex;

            align-items: center;
            justify-content: center;

            gap: 6px;

            padding: 10px 18px;

            border-radius: 8px;

            font-weight: 600;

            text-decoration: none;

            cursor: pointer;

            border: none;

            transition: 0.2s;

            text-align: center;

            font-size: 0.9rem;
        }

        .btn-primary {
            background: var(--accent);
            color: white;
        }

        .btn-primary:hover {
            background: var(--accent-hover);
        }

        .btn-success {
            background: var(--success);
            color: white;
        }

        .btn-success:hover {
            background: #15803d;
        }

        .btn-danger {
            background: rgba(239, 68, 68, 0.15);
            color: var(--danger);
            border: 1px solid var(--danger);
        }

        .btn-danger:hover {
            background: var(--danger);
            color: white;
        }

        .btn-secondary {
            background: #475569;
            color: white;
        }

        .btn-secondary:hover {
            background: #334155;
        }

        .btn-warning {
            background: var(--warning);
            color: #fff;
        }

        .btn-warning:hover {
            background: #ca8a04;
        }

        /* =========================
           ACTION BUTTONS
        ========================= */

        .main-actions {
            display: grid;

            grid-template-columns: 1fr 1fr;

            gap: 12px;

            margin-bottom: 16px;
        }

        /* =========================
           TOOLBAR
        ========================= */

        .toolbar {
            display: flex;

            gap: 10px;

            margin-bottom: 16px;

            flex-wrap: wrap;

            align-items: center;

            background: #0f172a;

            padding: 12px;

            border-radius: 8px;

            border: 1px solid var(--border);
        }

        select.monitor-select {
            padding: 10px 12px;

            border-radius: 8px;

            background: var(--card-bg);

            color: white;

            border: 1px solid var(--border);

            font-size: 0.9rem;

            outline: none;

            cursor: pointer;
        }

        select.monitor-select:focus {
            border-color: var(--accent);
        }

        /* =========================
           STREAM
        ========================= */

        .canvas-container {
            display: none;

            margin-top: 10px;

            background: #000;

            border-radius: 8px;

            overflow: hidden;

            border: 1px solid var(--border);

            position: relative;

            box-shadow:
                0 12px 30px rgba(0, 0, 0, 0.5);

            flex: 1;

            min-height: 500px;
        }

        canvas {
            width: 100%;
            height: 100%;

            object-fit: contain;

            display: block;

            cursor: default;

            outline: none;

            background: #000;
        }

        /* =========================
           HUD
        ========================= */

        .hud-badge {
            position: absolute;

            top: 12px;
            right: 12px;

            background: rgba(15, 23, 42, 0.85);

            border: 1px solid var(--border);

            padding: 6px 12px;

            border-radius: 6px;

            font-family: monospace;

            font-size: 0.8rem;

            color: var(--online);

            pointer-events: none;

            z-index: 10;
        }

        /* =========================
           DROP OVERLAY
        ========================= */

        .drop-overlay {
            position: absolute;

            top: 0;
            left: 0;

            width: 100%;
            height: 100%;

            background: rgba(59, 130, 246, 0.85);

            display: flex;

            flex-direction: column;

            justify-content: center;

            align-items: center;

            color: white;

            font-size: 1.5rem;

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

        /* =========================
           CHAT
        ========================= */

        .chat-panel {
            position: fixed;

            bottom: 20px;
            right: 20px;

            width: 320px;

            background: var(--card-bg);

            border: 1px solid var(--border);

            border-radius: 10px;

            box-shadow:
                0 10px 30px rgba(0, 0, 0, 0.6);

            display: flex;

            flex-direction: column;

            overflow: hidden;

            z-index: 50;

            transform: translateY(150%);

            transition:
                transform 0.3s ease;
        }

        .chat-panel.open {
            transform: translateY(0);
        }

        .chat-header {
            background: #0f172a;

            padding: 12px 16px;

            font-weight: bold;

            font-size: 0.95rem;

            border-bottom: 1px solid var(--border);

            display: flex;

            justify-content: space-between;

            align-items: center;
        }

        .chat-close {
            cursor: pointer;

            color: var(--text-muted);

            font-size: 1.2rem;

            line-height: 1;
        }

        .chat-close:hover {
            color: white;
        }

        .chat-messages {
            height: 250px;

            overflow-y: auto;

            padding: 12px;

            font-size: 0.85rem;

            display: flex;

            flex-direction: column;

            gap: 8px;
        }

        .msg {
            padding: 8px 12px;

            border-radius: 8px;

            max-width: 85%;

            word-wrap: break-word;
        }

        .msg.sent {
            background: var(--accent);

            align-self: flex-end;

            color: white;
        }

        .msg.recv {
            background: #334155;

            align-self: flex-start;

            color: white;
        }

        .chat-input {
            display: flex;

            border-top: 1px solid var(--border);

            background: #0f172a;
        }

        .chat-input input {
            flex: 1;

            background: transparent;

            border: none;

            color: white;

            padding: 12px;

            outline: none;

            font-size: 0.9rem;
        }

        .chat-input button {
            background: var(--accent);

            border: none;

            color: white;

            padding: 0 16px;

            cursor: pointer;

            font-weight: bold;

            transition: 0.2s;
        }

        .chat-input button:hover {
            background: var(--accent-hover);
        }

        /* =========================
           RESPONSIVE
        ========================= */

        @media (max-width: 700px) {

            .topbar {
                padding: 0 14px;
            }

            .container {
                padding: 0 10px;
                margin: 10px auto;
            }

            .session-card {
                padding: 16px;
            }

            .main-actions {
                grid-template-columns: 1fr;
            }

            .session-code {
                font-size: 1.4rem;
            }

            .chat-panel {
                width: calc(100% - 20px);

                right: 10px;
                bottom: 10px;
            }

            .canvas-container {
                min-height: 400px;
            }
        }
    </style>
</head>

<body>

    <!-- =========================
         TOP BAR
    ========================= -->

    <div class="topbar">

        <div class="device-badge">

            <div class="dot"></div>

            <strong>
                Host:
                <?= htmlspecialchars($deviceName, ENT_QUOTES, 'UTF-8') ?>
            </strong>

        </div>

        <a
            href="../devices.php"
            class="btn btn-danger"
            style="padding: 6px 14px; font-size: 0.8rem;"
        >
            Back to Dashboard
        </a>

    </div>


    <!-- =========================
         MAIN CONTENT
    ========================= -->

    <div class="container">

        <div class="session-card">

            <h2>
                Remote Access Terminal
            </h2>

            <p class="session-description">
                Launch low-latency browser streaming or open the native desktop client.
            </p>


            <!-- SESSION CODE -->

            <div class="code-display">

                <div
                    style="
                        font-size: 0.8rem;
                        color: var(--text-muted);
                        margin-bottom: 4px;
                    "
                >
                    REMOTE SESSION ACCESS ID
                </div>

                <div class="session-code">
                    <?= htmlspecialchars($sessionCode, ENT_QUOTES, 'UTF-8') ?>
                </div>

            </div>


            <!-- MAIN ACTIONS -->

            <div class="main-actions">

                <button
                    class="btn btn-success"
                    id="streamBtn"
                    type="button"
                >
                    Start Web Canvas Stream
                </button>

                <a
                    href="screenshare://<?= rawurlencode($deviceUid) ?>"
                    class="btn btn-primary"
                >
                    Launch Native Desktop Viewer
                </a>

            </div>


            <!-- ADVANCED TOOLBAR -->

            <div
                class="toolbar"
                id="advToolbar"
                style="display: none;"
            >

                <select
                    class="monitor-select"
                    id="monitorSelect"
                >
                    <option value="0">
                        Monitor 1 (Primary)
                    </option>

                    <option value="1">
                        Monitor 2
                    </option>

                    <option value="2">
                        Monitor 3
                    </option>

                    <option value="3">
                        Monitor 4
                    </option>
                </select>


                <button
                    class="btn btn-secondary"
                    id="audioBtn"
                    type="button"
                >
                    🔈 Listen Audio
                </button>


                <button
                    class="btn btn-secondary"
                    id="chatBtn"
                    type="button"
                >
                    💬 Chat
                </button>


                <button
                    class="btn btn-secondary"
                    id="recordBtn"
                    type="button"
                >
                    🔴 Record Session
                </button>

            </div>


            <!-- STREAM BOX -->

            <div
                id="stream-box"
                class="canvas-container"
            >

                <div
                    id="hud"
                    class="hud-badge"
                >
                    DISCONNECTED
                </div>


                <div
                    id="dropOverlay"
                    class="drop-overlay"
                >
                    📁 Drop files here to send to Remote Host
                    <br>
                    <small style="font-size: 0.9rem;">
                        Files will be placed in RemoteDrop/
                    </small>
                </div>


                <canvas
                    id="remoteCanvas"
                    tabindex="0"
                ></canvas>

            </div>

        </div>

    </div>


    <!-- =========================
         CHAT PANEL
    ========================= -->

    <div
        class="chat-panel"
        id="chatPanel"
    >

        <div class="chat-header">

            <span>
                Session Chat
            </span>

            <span
                class="chat-close"
                id="chatClose"
            >
                &times;
            </span>

        </div>


        <div
            class="chat-messages"
            id="chatMessages"
        ></div>


        <div class="chat-input">

            <input
                type="text"
                id="chatInput"
                placeholder="Type a message..."
                autocomplete="off"
            >

            <button
                type="button"
                id="sendChatBtn"
            >
                Send
            </button>

        </div>

    </div>


    <script>

        /* =========================================================
           CONFIGURATION
        ========================================================= */

        const DEVICE_ID =
            <?= json_encode($deviceUid, JSON_UNESCAPED_SLASHES) ?>;

        /*
         * IMPORTANT:
         * Change this if your WebSocket server is running elsewhere.
         */
        const WS_URL = "ws://192.168.29.229:9001";


        /* =========================================================
           GLOBAL STATE
        ========================================================= */

        let ws = null;

        let isStreaming = false;

        let canvas = null;

        let ctx = null;

        let animationFrameId = null;

        let latestImage = null;

        let renderWidth = 0;

        let renderHeight = 0;

        let framePending = false;

        let videoFrameCount = 0;


        /* AUDIO */

        let audioCtx = null;

        let nextAudioTime = 0;

        let isAudioEnabled = false;


        /* RECORDING */

        let mediaRecorder = null;

        let recordedChunks = [];


        /* =========================================================
           DOM
        ========================================================= */

        const streamBtn =
            document.getElementById("streamBtn");

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


        /* =========================================================
           UTILITY
        ========================================================= */

        function setHud(text) {

            hud.textContent = text;

        }


        function isSocketOpen() {

            return ws &&
                   ws.readyState === WebSocket.OPEN;

        }


        function safeCloseImage() {

            if (latestImage) {

                try {

                    if (
                        typeof latestImage.close ===
                        "function"
                    ) {
                        latestImage.close();
                    }

                } catch (e) {

                    console.warn(
                        "Image cleanup failed:",
                        e
                    );

                }

                latestImage = null;
            }

        }


        /* =========================================================
           CHAT
        ========================================================= */

        function toggleChat() {

            chatPanel.classList.toggle("open");

            if (
                chatPanel.classList.contains("open")
            ) {

                setTimeout(() => {
                    chatInput.focus();
                }, 100);

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


            const encoder =
                new TextEncoder();

            const msgBytes =
                encoder.encode(msg);


            /*
             * Packet:
             *
             * Byte 0 = 16
             * Byte 1-2 = message length
             * Byte 3... = UTF-8 message
             */

            if (msgBytes.length > 65535) {

                alert(
                    "Message is too long."
                );

                return;
            }


            const pkt =
                new Uint8Array(
                    3 + msgBytes.length
                );

            pkt[0] = 16;

            pkt[1] =
                (msgBytes.length >> 8) & 0xFF;

            pkt[2] =
                msgBytes.length & 0xFF;

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
                    "Chat send error:",
                    error
                );

            }

        }


        /* =========================================================
           AUDIO
        ========================================================= */

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
                        "Audio initialization error:",
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

            }

        }


        /* =========================================================
           MONITOR SWITCH
        ========================================================= */

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


            /*
             * Packet:
             *
             * Byte 0 = 7
             * Byte 1 = monitor index
             */

            const pkt =
                new Uint8Array(9);

            pkt[0] = 7;

            pkt[1] =
                monitorIndex;


            ws.send(pkt);

        }


        /* =========================================================
           RECORDING
        ========================================================= */

        function toggleRecording() {

            if (!canvas) {

                alert(
                    "Start the remote stream first."
                );

                return;
            }


            /*
             * STOP RECORDING
             */

            if (
                mediaRecorder &&
                mediaRecorder.state ===
                "recording"
            ) {

                mediaRecorder.stop();

                return;
            }


            /*
             * CHECK SUPPORT
             */

            if (
                !window.MediaRecorder ||
                !canvas.captureStream
            ) {

                alert(
                    "Session recording is not supported by this browser."
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
                    "MediaRecorder error:",
                    error
                );

                alert(
                    "Unable to start session recording."
                );

                return;
            }


            recordedChunks = [];


            mediaRecorder.ondataavailable =
                function (event) {

                    if (
                        event.data &&
                        event.data.size > 0
                    ) {

                        recordedChunks.push(
                            event.data
                        );

                    }

                };


            mediaRecorder.onerror =
                function (event) {

                    console.error(
                        "Recorder error:",
                        event
                    );

                };


            mediaRecorder.onstop =
                function () {

                    if (
                        recordedChunks.length === 0
                    ) {

                        recordBtn.textContent =
                            "🔴 Record Session";

                        recordBtn.classList.remove(
                            "btn-danger"
                        );

                        recordBtn.classList.add(
                            "btn-secondary"
                        );

                        return;
                    }


                    const blob =
                        new Blob(
                            recordedChunks,
                            {
                                type:
                                    mimeType
                            }
                        );


                    const url =
                        URL.createObjectURL(
                            blob
                        );


                    const a =
                        document.createElement(
                            "a"
                        );

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
                        () => {
                            URL.revokeObjectURL(
                                url
                            );
                        },
                        1000
                    );


                    recordedChunks = [];

                    recordBtn.textContent =
                        "🔴 Record Session";

                    recordBtn.classList.remove(
                        "btn-danger"
                    );

                    recordBtn.classList.add(
                        "btn-secondary"
                    );

                };


            mediaRecorder.start(
                1000
            );


            recordBtn.textContent =
                "⏹ Stop Recording";

            recordBtn.classList.remove(
                "btn-secondary"
            );

            recordBtn.classList.add(
                "btn-danger"
            );

        }


        /* =========================================================
           RENDER LOOP
        ========================================================= */

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
                            "Canvas draw error:",
                            error
                        );

                    }

                }


                animationFrameId =
                    requestAnimationFrame(
                        render
                    );

            }


            animationFrameId =
                requestAnimationFrame(
                    render
                );

        }


        function stopRenderLoop() {

            if (animationFrameId) {

                cancelAnimationFrame(
                    animationFrameId
                );

                animationFrameId = null;

            }

        }


        /* =========================================================
           VIDEO FRAME
        ========================================================= */

        async function handleVideoFrame(buffer) {

            const bytes =
                new Uint8Array(buffer);


            /*
             * Packet structure:
             *
             * Byte 0     = packet type (1)
             * Byte 1-4   = width
             * Byte 5-8   = height
             * Byte 9-12  = JPEG payload size
             * Byte 13... = JPEG
             *
             * Total header = 13 bytes
             */


            if (buffer.byteLength < 13) {

                console.error(
                    "[VIDEO] Packet too small:",
                    buffer.byteLength
                );

                return;
            }


            const view =
                new DataView(buffer);


            const frameW =
                view.getUint32(
                    1,
                    false
                );


            const frameH =
                view.getUint32(
                    5,
                    false
                );


            const payloadSize =
                view.getUint32(
                    9,
                    false
                );


            if (
                frameW === 0 ||
                frameH === 0
            ) {

                console.error(
                    "[VIDEO] Invalid dimensions:",
                    frameW,
                    frameH
                );

                return;
            }


            const expectedLength =
                13 + payloadSize;


            if (
                bytes.length !==
                expectedLength
            ) {

                console.error(
                    "[VIDEO] Packet length mismatch:",
                    {
                        actual:
                            bytes.length,

                        payloadSize:
                            payloadSize,

                        expected:
                            expectedLength
                    }
                );

                return;
            }


            if (
                payloadSize < 2
            ) {

                console.error(
                    "[VIDEO] Empty JPEG payload."
                );

                return;
            }


            const jpeg =
                bytes.slice(
                    13,
                    13 + payloadSize
                );


            /*
             * JPEG validation
             */

            const soi =
                jpeg[0] === 0xFF &&
                jpeg[1] === 0xD8;


            const eoi =
                jpeg[jpeg.length - 2] === 0xFF &&
                jpeg[jpeg.length - 1] === 0xD9;


            if (!soi) {

                console.error(
                    "[VIDEO] Invalid JPEG SOI."
                );

                return;
            }


            if (!eoi) {

                console.warn(
                    "[VIDEO] JPEG does not contain EOI."
                );

            }


            videoFrameCount++;


            setHud(
                "LIVE • FRAME " +
                videoFrameCount
            );


            /*
             * Decode JPEG.
             */

            const blob =
                new Blob(
                    [jpeg],
                    {
                        type:
                            "image/jpeg"
                    }
                );


            const url =
                URL.createObjectURL(
                    blob
                );


            try {

                const img =
                    new Image();


                await new Promise(
                    (resolve, reject) => {

                        img.onload =
                            resolve;

                        img.onerror =
                            reject;

                        img.src =
                            url;

                    }
                );


                /*
                 * Ignore frames if stream
                 * has already been stopped.
                 */

                if (!isStreaming) {

                    return;
                }


                /*
                 * Replace old frame.
                 */

                if (latestImage) {

                    try {

                        latestImage.src = "";

                    } catch (e) {}

                }


                latestImage =
                    img;


                renderWidth =
                    frameW;

                renderHeight =
                    frameH;


                /*
                 * If rendering loop is not running,
                 * draw immediately.
                 */

                if (!animationFrameId) {

                    if (
                        canvas.width !==
                        frameW ||
                        canvas.height !==
                        frameH
                    ) {

                        canvas.width =
                            frameW;

                        canvas.height =
                            frameH;

                    }


                    ctx.drawImage(
                        img,
                        0,
                        0,
                        frameW,
                        frameH
                    );

                }

            } catch (error) {

                console.error(
                    "[VIDEO] JPEG decode failed:",
                    error
                );

            } finally {

                URL.revokeObjectURL(
                    url
                );

            }

        }


        /* =========================================================
           AUDIO PACKET
        ========================================================= */

        function handleAudioPacket(buffer) {

            if (!isAudioEnabled) {
                return;
            }


            if (buffer.byteLength < 11) {

                console.error(
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
                channels <= 0 ||
                sampleRate <= 0
            ) {

                return;
            }


            const audioStart =
                11;


            const audioEnd =
                audioStart +
                byteLen;


            if (
                audioEnd >
                buffer.byteLength
            ) {

                console.error(
                    "[AUDIO] Invalid audio size."
                );

                return;
            }


            /*
             * Float32 audio.
             */

            const audioBytes =
                buffer.slice(
                    audioStart,
                    audioEnd
                );


            const floatArray =
                new Float32Array(
                    audioBytes
                );


            if (
                floatArray.length === 0
            ) {

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
                    .catch(
                        console.error
                    );

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


            /*
             * Convert interleaved PCM
             * into Web Audio channels.
             */

            for (
                let c = 0;
                c < channels;
                c++
            ) {

                const channelData =
                    audioBuffer
                        .getChannelData(c);


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
                    audioCtx.currentTime +
                    0.05;

            }


            source.start(
                nextAudioTime
            );


            nextAudioTime +=
                audioBuffer.duration;

        }


        /* =========================================================
           CHAT PACKET
        ========================================================= */

        function handleChatPacket(buffer) {

            if (
                buffer.byteLength <
                3
            ) {

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

                console.error(
                    "[CHAT] Invalid packet length."
                );

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


        /* =========================================================
           WEBSOCKET MESSAGE
        ========================================================= */

        async function handleWebSocketMessage(event) {

            if (
                !(event.data instanceof ArrayBuffer)
            ) {

                console.warn(
                    "[WS] Non-binary message received."
                );

                return;
            }


            const buffer =
                event.data;


            if (
                buffer.byteLength === 0
            ) {

                return;
            }


            const view =
                new DataView(buffer);


            const type =
                view.getUint8(0);


            /*
             * TYPE 1
             * VIDEO FRAME
             */

            if (type === 1) {

                /*
                 * Some servers may send a
                 * one-byte "stream active"
                 * message.
                 */

                if (
                    buffer.byteLength === 1
                ) {

                    setHud(
                        "LIVE STREAM ACTIVE"
                    );

                    return;
                }


                await handleVideoFrame(
                    buffer
                );

                return;
            }


            /*
             * TYPE 13
             * AUDIO
             */

            if (type === 13) {

                handleAudioPacket(
                    buffer
                );

                return;
            }


            /*
             * TYPE 16
             * CHAT
             */

            if (type === 16) {

                handleChatPacket(
                    buffer
                );

                return;
            }


            console.debug(
                "[WS] Unknown packet type:",
                type,
                "size:",
                buffer.byteLength
            );

        }


        /* =========================================================
           DEVICE ID INITIALIZATION
        ========================================================= */

        function sendInitializationPacket() {

            if (!isSocketOpen()) {
                return;
            }


            /*
             * Your existing protocol uses:
             *
             * 39-byte initialization packet
             * Byte 0 = 2
             * Device ID starts at byte 1
             *
             * We preserve the 39-byte packet.
             */


            const initPkt =
                new Uint8Array(39);


            initPkt[0] = 2;


            const encoder =
                new TextEncoder();


            const deviceBytes =
                encoder.encode(
                    DEVICE_ID
                );


            /*
             * Maximum available ID
             * area is 38 bytes.
             */

            const copyLength =
                Math.min(
                    deviceBytes.length,
                    38
                );


            initPkt.set(
                deviceBytes.slice(
                    0,
                    copyLength
                ),
                1
            );


            ws.send(
                initPkt
            );


            console.log(
                "[WS] Initialization packet sent.",
                {
                    deviceId:
                        DEVICE_ID,
                    bytes:
                        copyLength
                }
            );

        }


        /* =========================================================
           START STREAM
        ========================================================= */

        function startWebStream() {

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
                    "Your browser could not create a canvas."
                );

                return;
            }


            /*
             * Reset state.
             */

            videoFrameCount = 0;

            renderWidth = 0;

            renderHeight = 0;

            safeCloseImage();


            canvas.width = 1280;

            canvas.height = 720;


            isStreaming = true;


            streamBox.style.display =
                "block";


            advToolbar.style.display =
                "flex";


            streamBtn.textContent =
                "Disconnect Web Stream";


            streamBtn.className =
                "btn btn-danger";


            setHud(
                "CONNECTING..."
            );


            canvas.focus();


            startRenderLoop();


            /*
             * Create WebSocket.
             */

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
                    "Could not create WebSocket connection."
                );

                return;
            }


            ws.binaryType =
                "arraybuffer";


            /* =========================
               OPEN
            ========================= */

            ws.onopen =
                function () {

                    console.log(
                        "[WS] Connected:",
                        WS_URL
                    );


                    setHud(
                        "AUTHENTICATING..."
                    );


                    sendInitializationPacket();

                };


            /* =========================
               MESSAGE
            ========================= */

            ws.onmessage =
                handleWebSocketMessage;


            /* =========================
               ERROR
            ========================= */

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


            /* =========================
               CLOSE
            ========================= */

            ws.onclose =
                function (event) {

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


        /* =========================================================
           STOP STREAM
        ========================================================= */

        function stopWebStream() {

            isStreaming = false;


            stopRenderLoop();


            safeCloseImage();


            renderWidth = 0;

            renderHeight = 0;


            if (
                mediaRecorder &&
                mediaRecorder.state ===
                "recording"
            ) {

                try {

                    mediaRecorder.stop();

                } catch (e) {

                    console.warn(
                        "Recorder stop error:",
                        e
                    );

                }

            }


            if (ws) {

                try {

                    ws.onopen = null;

                    ws.onmessage = null;

                    ws.onerror = null;

                    ws.onclose = null;

                    ws.close();

                } catch (e) {

                    console.warn(
                        "WebSocket close error:",
                        e
                    );

                }

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


            streamBox.style.display =
                "none";


            advToolbar.style.display =
                "none";


            streamBtn.textContent =
                "Start Web Canvas Stream";


            streamBtn.className =
                "btn btn-success";


            setHud(
                "DISCONNECTED"
            );


            if (
                chatPanel.classList.contains(
                    "open"
                )
            ) {

                chatPanel.classList.remove(
                    "open"
                );

            }

        }


        /* =========================================================
           TOGGLE STREAM
        ========================================================= */

        function toggleWebStream() {

            if (isStreaming) {

                stopWebStream();

            } else {

                startWebStream();

            }

        }


        /* =========================================================
           FILE TRANSFER
        ========================================================= */

        async function sendFiles(files) {

            if (!isSocketOpen()) {

                alert(
                    "WebSocket not connected."
                );

                return;
            }


            if (!files || files.length === 0) {
                return;
            }


            for (
                const file of files
            ) {

                try {

                    await sendSingleFile(
                        file
                    );

                } catch (error) {

                    console.error(
                        "File transfer failed:",
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
                nameBytes.length >
                65535
            ) {

                throw new Error(
                    "File name is too long."
                );

            }


            /*
             * TYPE 20
             *
             * Byte 0 = 20
             * Byte 1-2 = filename length
             * Byte 3-10 = file size
             * Byte 11... = filename
             */

            const metaPkt =
                new Uint8Array(
                    1 +
                    2 +
                    8 +
                    nameBytes.length
                );


            metaPkt[0] = 20;


            metaPkt[1] =
                (nameBytes.length >> 8) &
                0xFF;


            metaPkt[2] =
                nameBytes.length &
                0xFF;


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


            ws.send(
                metaPkt
            );


            /*
             * TYPE 21
             *
             * Byte 0 = 21
             * Byte 1-4 = chunk length
             * Byte 5... = data
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
                        offset +
                        chunkSize,
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


                const chunkPkt =
                    new Uint8Array(
                        1 +
                        4 +
                        chunkBytes.length
                    );


                chunkPkt[0] = 21;


                const chunkView =
                    new DataView(
                        chunkPkt.buffer
                    );


                chunkView.setUint32(
                    1,
                    chunkBytes.length,
                    false
                );


                chunkPkt.set(
                    chunkBytes,
                    5
                );


                ws.send(
                    chunkPkt
                );


                offset = end;

            }


            alert(
                `File "${file.name}" sent successfully to Host's RemoteDrop folder!`
            );

        }


        /* =========================================================
           MOUSE COORDINATES
        ========================================================= */

        function getNormalizedCoordinates(event) {

            if (
                !canvas ||
                !canvas.width ||
                !canvas.height
            ) {

                return null;
            }


            const rect =
                canvas.getBoundingClientRect();


            if (
                rect.width <= 0 ||
                rect.height <= 0
            ) {

                return null;
            }


            const x =
                event.clientX -
                rect.left;


            const y =
                event.clientY -
                rect.top;


            /*
             * Convert displayed canvas
             * coordinates to actual canvas
             * coordinates.
             */

            const canvasX =
                x *
                (canvas.width /
                    rect.width);


            const canvasY =
                y *
                (canvas.height /
                    rect.height);


            const normalizedX =
                Math.max(
                    0,
                    Math.min(
                        65535,
                        Math.floor(
                            (
                                canvasX /
                                canvas.width
                            ) *
                            65535
                        )
                    )
                );


            const normalizedY =
                Math.max(
                    0,
                    Math.min(
                        65535,
                        Math.floor(
                            (
                                canvasY /
                                canvas.height
                            ) *
                            65535
                        )
                    )
                );


            return {
                x:
                    normalizedX,

                y:
                    normalizedY
            };

        }


        /* =========================================================
           SEND MOUSE MOVE
        ========================================================= */

        function sendMouseMove(event) {

            if (!isSocketOpen()) {
                return;
            }


            const coords =
                getNormalizedCoordinates(
                    event
                );


            if (!coords) {
                return;
            }


            /*
             * TYPE 0
             *
             * Byte 0 = 0
             * Byte 1-2 = X
             * Byte 3-4 = Y
             */

            const pkt =
                new Uint8Array(9);


            pkt[0] = 0;


            pkt[1] =
                (coords.x >> 8) &
                0xFF;

            pkt[2] =
                coords.x &
                0xFF;


            pkt[3] =
                (coords.y >> 8) &
                0xFF;

            pkt[4] =
                coords.y &
                0xFF;


            ws.send(pkt);

        }


        /* =========================================================
           MOUSE DOWN
        ========================================================= */

        function sendMouseDown(event) {

            if (!isSocketOpen()) {
                return;
            }


            const coords =
                getNormalizedCoordinates(
                    event
                );


            if (!coords) {
                return;
            }


            /*
             * Left = 1
             * Right = 3
             */

            const packetType =
                event.button === 2
                    ? 3
                    : 1;


            const pkt =
                new Uint8Array(9);


            pkt[0] =
                packetType;


            pkt[1] =
                (coords.x >> 8) &
                0xFF;

            pkt[2] =
                coords.x &
                0xFF;


            pkt[3] =
                (coords.y >> 8) &
                0xFF;

            pkt[4] =
                coords.y &
                0xFF;


            ws.send(pkt);

        }


        /* =========================================================
           MOUSE UP
        ========================================================= */

        function sendMouseUp(event) {

            if (!isSocketOpen()) {
                return;
            }


            /*
             * Left = 2
             * Right = 4
             */

            const packetType =
                event.button === 2
                    ? 4
                    : 2;


            const pkt =
                new Uint8Array(9);


            pkt[0] =
                packetType;


            ws.send(pkt);

        }


        /* =========================================================
           MOUSE WHEEL
        ========================================================= */

        function sendMouseWheel(event) {

            if (!isSocketOpen()) {
                return;
            }


            event.preventDefault();


            const scroll =
                event.deltaY > 0
                    ? -120
                    : 120;


            /*
             * TYPE 8
             */

            const pkt =
                new Uint8Array(9);


            pkt[0] = 8;


            pkt[1] = 0;
            pkt[2] = 0;


            pkt[3] =
                (scroll >> 8) &
                0xFF;


            pkt[4] =
                scroll &
                0xFF;


            ws.send(pkt);

        }


        /* =========================================================
           KEYBOARD
        ========================================================= */

        function sendKeyboard(event, type) {

            if (!isSocketOpen()) {
                return;
            }


            event.preventDefault();


            const keyCode =
                event.keyCode ||
                event.which;


            /*
             * TYPE 5 = key down
             * TYPE 6 = key up
             */

            const pkt =
                new Uint8Array(5);


            pkt[0] =
                type;


            pkt[1] =
                (keyCode >> 24) &
                0xFF;


            pkt[2] =
                (keyCode >> 16) &
                0xFF;


            pkt[3] =
                (keyCode >> 8) &
                0xFF;


            pkt[4] =
                keyCode &
                0xFF;


            ws.send(pkt);

        }


        /* =========================================================
           ATTACH CANVAS EVENTS
        ========================================================= */

        function attachCanvasEvents() {

            if (!canvas) {
                return;
            }


            canvas.addEventListener(
                "mousemove",
                sendMouseMove
            );


            canvas.addEventListener(
                "mousedown",
                function (event) {

                    canvas.focus();

                    sendMouseDown(
                        event
                    );

                }
            );


            canvas.addEventListener(
                "mouseup",
                sendMouseUp
            );


            canvas.addEventListener(
                "contextmenu",
                function (event) {

                    event.preventDefault();

                }
            );


            canvas.addEventListener(
                "wheel",
                sendMouseWheel,
                {
                    passive: false
                }
            );


            canvas.addEventListener(
                "keydown",
                function (event) {

                    sendKeyboard(
                        event,
                        5
                    );

                }
            );


            canvas.addEventListener(
                "keyup",
                function (event) {

                    sendKeyboard(
                        event,
                        6
                    );

                }
            );

        }


        /* =========================================================
           DRAG & DROP
        ========================================================= */

        function attachDropEvents() {

            streamBox.addEventListener(
                "dragover",
                function (event) {

                    event.preventDefault();

                    dropOverlay.classList.add(
                        "active"
                    );

                }
            );


            streamBox.addEventListener(
                "dragleave",
                function (event) {

                    event.preventDefault();

                    dropOverlay.classList.remove(
                        "active"
                    );

                }
            );


            streamBox.addEventListener(
                "drop",
                async function (event) {

                    event.preventDefault();


                    dropOverlay.classList.remove(
                        "active"
                    );


                    if (!isSocketOpen()) {

                        alert(
                            "WebSocket not connected!"
                        );

                        return;
                    }


                    const files =
                        event.dataTransfer.files;


                    await sendFiles(
                        files
                    );

                }
            );

        }


        /* =========================================================
           BUTTON EVENTS
        ========================================================= */

        streamBtn.addEventListener(
            "click",
            toggleWebStream
        );


        audioBtn.addEventListener(
            "click",
            toggleAudio
        );


        chatBtn.addEventListener(
            "click",
            toggleChat
        );


        chatClose.addEventListener(
            "click",
            toggleChat
        );


        sendChatBtn.addEventListener(
            "click",
            sendChat
        );


        chatInput.addEventListener(
            "keydown",
            function (event) {

                if (
                    event.key ===
                    "Enter"
                ) {

                    sendChat();

                }

            }
        );


        recordBtn.addEventListener(
            "click",
            toggleRecording
        );


        monitorSelect.addEventListener(
            "change",
            function () {

                switchMonitor(
                    this.value
                );

            }
        );


        /* =========================================================
           INITIALIZE
        ========================================================= */

        attachDropEvents();

        /*
         * Canvas exists in DOM from the beginning,
         * so attach its events once.
         */

        canvas =
            document.getElementById(
                "remoteCanvas"
            );


        attachCanvasEvents();


        /* =========================================================
           PAGE CLEANUP
        ========================================================= */

        window.addEventListener(
            "beforeunload",
            function () {

                stopWebStream();

            }
        );


        /* =========================================================
           DEBUG
        ========================================================= */

        console.log(
            "Remote Viewer initialized."
        );

        console.log(
            "Device ID:",
            DEVICE_ID
        );

        console.log(
            "WebSocket:",
            WS_URL
        );

    </script>

</body>

</html>