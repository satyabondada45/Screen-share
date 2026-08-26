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
        Active Remote Session - <?= htmlspecialchars($deviceName, ENT_QUOTES, 'UTF-8') ?>
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
                    : 'var(--offline)' ?>
            ;

            box-shadow:
                <?= $isOnline
                    ? '0 0 10px var(--online)'
                    : 'none' ?>
            ;

        }

        .container {

            width: 100%;

            max-width: 1400px;

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
                0 10px 25px rgba(0, 0, 0, .3);

            display: flex;

            flex-direction: column;

            min-height:
                calc(100vh - 108px);

        }

        .session-description {

            color: var(--text-muted);

            margin-top: 5px;

        }

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

            transition: .2s;

            text-align: center;

            font-size: .9rem;

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

            background: rgba(239, 68, 68, .15);

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

            color: white;

        }

        .btn-warning:hover {

            background: #ca8a04;

        }

        .main-actions {

            display: grid;

            grid-template-columns: 1fr 1fr;

            gap: 12px;

            margin-bottom: 16px;

        }

        .toolbar {

            display: none;

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

            font-size: .9rem;

            outline: none;

            cursor: pointer;

        }

        .canvas-container {

            display: none;

            margin-top: 10px;

            background: #000;

            border-radius: 8px;

            overflow: hidden;

            border: 1px solid var(--border);

            position: relative;

            flex: 1;

            min-height: 500px;

            box-shadow:
                0 12px 30px rgba(0, 0, 0, .5);

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

        .hud-badge {

            position: absolute;

            top: 12px;

            right: 12px;

            background:
                rgba(15, 23, 42, .9);

            border: 1px solid var(--border);

            padding: 6px 12px;

            border-radius: 6px;

            font-family: monospace;

            font-size: .8rem;

            color: var(--online);

            pointer-events: none;

            z-index: 10;

        }

        .drop-overlay {

            position: absolute;

            inset: 0;

            background:
                rgba(59, 130, 246, .85);

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

            transition: .2s;

            text-align: center;

        }

        .drop-overlay.active {

            opacity: 1;

            pointer-events: auto;

        }

        .chat-panel {

            position: fixed;

            bottom: 20px;

            right: 20px;

            width: 320px;

            background: var(--card-bg);

            border: 1px solid var(--border);

            border-radius: 10px;

            box-shadow:
                0 10px 30px rgba(0, 0, 0, .6);

            display: flex;

            flex-direction: column;

            overflow: hidden;

            z-index: 50;

            transform: translateY(150%);

            transition:
                transform .3s ease;

        }

        .chat-panel.open {

            transform: translateY(0);

        }

        .chat-header {

            background: #0f172a;

            padding: 12px 16px;

            font-weight: bold;

            border-bottom: 1px solid var(--border);

            display: flex;

            justify-content: space-between;

            align-items: center;

        }

        .chat-close {

            cursor: pointer;

            color: var(--text-muted);

            font-size: 1.2rem;

        }

        .chat-messages {

            height: 250px;

            overflow-y: auto;

            padding: 12px;

            font-size: .85rem;

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

        }

        .msg.recv {

            background: #334155;

            align-self: flex-start;

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

        }

        .chat-input button {

            background: var(--accent);

            border: none;

            color: white;

            padding: 0 16px;

            cursor: pointer;

            font-weight: bold;

        }

        @media(max-width:700px) {

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

                width:
                    calc(100% - 20px);

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


    <div class="topbar">

        <div class="device-badge">

            <div class="dot"></div>

            <strong>

                Host:
                <?= htmlspecialchars(
                    $deviceName,
                    ENT_QUOTES,
                    'UTF-8'
                ) ?>

            </strong>

        </div>

        <a href="../devices.php" class="btn btn-danger" style="
            padding:6px 14px;
            font-size:.8rem;
        ">
            Back to Dashboard
        </a>

    </div>


    <div class="container">

        <div class="session-card">

            <h2>
                Remote Access Terminal
            </h2>

            <p class="session-description">
                Browser remote desktop session.
            </p>


            <div class="code-display">

                <div style="
                font-size:.8rem;
                color:var(--text-muted);
                margin-bottom:4px;
            ">
                    REMOTE SESSION ACCESS ID
                </div>

                <div class="session-code">

                    <?= htmlspecialchars(
                        $sessionCode,
                        ENT_QUOTES,
                        'UTF-8'
                    ) ?>

                </div>

            </div>


            <div class="main-actions">

                <button class="btn btn-success" id="streamBtn" type="button">
                    Start Web Canvas Stream
                </button>

                <a href="screenshare://<?= rawurlencode($deviceUid) ?>" class="btn btn-primary">
                    Launch Native Desktop Viewer
                </a>

            </div>


            <div class="toolbar" id="advToolbar">

                <select class="monitor-select" id="monitorSelect">

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


                <button class="btn btn-secondary" id="audioBtn" type="button">
                    🔈 Listen Audio
                </button>


                <button class="btn btn-secondary" id="chatBtn" type="button">
                    💬 Chat
                </button>


                <button class="btn btn-secondary" id="recordBtn" type="button">
                    🔴 Record Session
                </button>

            </div>


            <div id="stream-box" class="canvas-container">

                <div id="hud" class="hud-badge">
                    DISCONNECTED
                </div>


                <div id="dropOverlay" class="drop-overlay">

                    📁 Drop files here to send to Remote Host

                    <br>

                    <small style="font-size:.9rem;">
                        Files will be placed in RemoteDrop/
                    </small>

                </div>


                <canvas id="remoteCanvas" tabindex="0"></canvas>

            </div>

        </div>

    </div>


    <div class="chat-panel" id="chatPanel">

        <div class="chat-header">

            <span>
                Session Chat
            </span>

            <span class="chat-close" id="chatClose">
                &times;
            </span>

        </div>


        <div class="chat-messages" id="chatMessages"></div>


        <div class="chat-input">

            <input type="text" id="chatInput" placeholder="Type a message..." autocomplete="off">

            <button type="button" id="sendChatBtn">
                Send
            </button>

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
                $deviceUid,
                JSON_UNESCAPED_SLASHES
            ) ?>;

        const WS_PORT = "9001";
        const WS_HOST = location.hostname || "192.168.29.229";
        const WS_URL = location.protocol === "https:"
            ? `wss://${location.host}/ws`
            : `ws://${WS_HOST}:${WS_PORT}`;


        /* ============================================================
           STATE
        ============================================================ */

        let ws = null;

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
        
           The agent now sends RGB data (3 bytes per pixel) to the remote
           viewer. This fixes the black-screen issue on the web viewer.
============================================================ */

let videoCodecConfigured = false;
let hasReceivedFirstKeyframe = false;

function parseH264Nals(dataBytes) {
    let nalTypes = [];
    let hasSPS = false;
    let hasPPS = false;
    let hasIDR = false;
    let spsUnit = null;
    let ppsUnit = null;
    let i = 0;

    while (i + 2 < dataBytes.length) {
        let scLen = 0;
        if (i + 3 < dataBytes.length && dataBytes[i] === 0 && dataBytes[i+1] === 0 && dataBytes[i+2] === 0 && dataBytes[i+3] === 1) {
            scLen = 4;
        } else if (dataBytes[i] === 0 && dataBytes[i+1] === 0 && dataBytes[i+2] === 1) {
            scLen = 3;
        }

        if (scLen > 0) {
            const nalStart = i + scLen;
            let nextStart = dataBytes.length;
            let j = nalStart;
            while (j + 2 < dataBytes.length) {
                if ((j + 3 < dataBytes.length && dataBytes[j] === 0 && dataBytes[j+1] === 0 && dataBytes[j+2] === 0 && dataBytes[j+3] === 1) ||
                    (dataBytes[j] === 0 && dataBytes[j+1] === 0 && dataBytes[j+2] === 1)) {
                    nextStart = j;
                    break;
                }
                j++;
            }

            if (nalStart < dataBytes.length) {
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
                }
            }
            i = nextStart;
        } else {
            i++;
        }
    }

    return { nalTypes, hasSPS, hasPPS, hasIDR, spsUnit, ppsUnit };
}

async function handleVideoPacket(buffer) {
    const bytes = new Uint8Array(buffer);
    const length = bytes.length;

    if (length < 13) {
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

    const available = length - 13;
    if (payloadSize <= 0 || payloadSize > available) {
        console.error(
            "[VIDEO] Invalid H.264 payload size.",
            { payloadSize, available, packetSize: length }
        );
        return;
    }

    const actualPayloadSize = (payloadSize > 0 && payloadSize <= available) ? payloadSize : available;
    const h264Payload = bytes.slice(13, 13 + actualPayloadSize);

    // ========================================================
    // 5. Capability Detection BEFORE using VideoDecoder
    // ========================================================
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

    // ========================================================
    // 6. Detailed Browser Logging
    // ========================================================
    console.log(
        "[BROWSER VIDEO RX]",
        {
            type: packetType,
            packetLength: length,
            width,
            height,
            h264ByteLength: actualPayloadSize
        }
    );

    const nals = parseH264Nals(h264Payload);

    console.log(
        "[BROWSER H264]",
        {
            firstBytes: Array.from(h264Payload.slice(0, 32)).map(b => b.toString(16).padStart(2, '0')).join(' '),
            nalTypes: nals.nalTypes,
            spsPresent: nals.hasSPS,
            ppsPresent: nals.hasPPS,
            idrPresent: nals.hasIDR
        }
    );

    // ========================================================
    // 7. H.264 Codec string derivation from SPS
    // ========================================================
    let codecString = 'avc1.42c028';
    if (nals.spsUnit && nals.spsUnit.length >= 4) {
        const p = nals.spsUnit[1].toString(16).padStart(2, '0');
        const c = nals.spsUnit[2].toString(16).padStart(2, '0');
        const l = nals.spsUnit[3].toString(16).padStart(2, '0');
        codecString = `avc1.${p}${c}${l}`;
    }

    // ========================================================
    // 6 & 11. WebCodecs Decoder Initialization & Reuse
    // ========================================================
    if (!videoDecoder || videoDecoder.state === 'closed') {
        videoCodecConfigured = false;
        hasReceivedFirstKeyframe = false;

        console.log("[BROWSER DECODER]", {
            videoDecoderAvailable: ("VideoDecoder" in window)
        });

        try {
            videoDecoder = new VideoDecoder({
                output(frame) {
                    console.log(
                        "[BROWSER DECODE OUTPUT]",
                        frame.codedWidth,
                        frame.codedHeight,
                        frame.displayWidth,
                        frame.displayHeight
                    );

                    if (canvas && ctx) {
                        if (canvas.width !== frame.displayWidth || canvas.height !== frame.displayHeight) {
                            canvas.width = frame.displayWidth;
                            canvas.height = frame.displayHeight;
                        }

                        console.log(
                            "[VIDEO DEBUG][CANVAS]",
                            {
                                width: canvas.width,
                                height: canvas.height
                            }
                        );

                        ctx.drawImage(frame, 0, 0, canvas.width, canvas.height);

                        console.log(
                            "[VIDEO DEBUG][CANVAS]",
                            "FRAME RENDERED"
                        );
                    }
                    frame.close();
                },
                error(error) {
                    console.error("[BROWSER DECODE ERROR]", error);
                    videoCodecConfigured = false;
                }
            });
        } catch (err) {
            console.error("[VIDEO FATAL] Failed to construct VideoDecoder:", err);
            return;
        }
    }

    // Configure decoder with derived codec string
    if (!videoCodecConfigured) {
        try {
            console.log("[BROWSER DECODER CONFIG]", {
                codec: codecString,
                codedWidth: width,
                codedHeight: height,
                descriptionPresence: false,
                state: videoDecoder.state
            });

            videoDecoder.configure({
                codec: codecString,
                codedWidth: width,
                codedHeight: height,
                optimizeForLatency: true
            });
            videoCodecConfigured = true;
            console.log("[H264] Decoder configured successfully:", codecString, width, "x", height);
        } catch (e) {
            console.error("[BROWSER DECODER CONFIG ERROR]", e);
            return;
        }
    }

    // ========================================================
    // 9. Critical Keyframe Rule
    // ========================================================
    const isKey = nals.hasIDR || nals.hasSPS;
    if (isKey) {
        hasReceivedFirstKeyframe = true;
    }

    if (!hasReceivedFirstKeyframe) {
        console.warn("[BROWSER VIDEO] Waiting for first keyframe (SPS/PPS/IDR) before decoding P-frames.");
        return;
    }

    // ========================================================
    // 10. Handle Decoder State Before Decode
    // ========================================================
    console.log("[BROWSER DECODER STATE]", videoDecoder.state);
    if (videoDecoder.state === "closed") {
        console.warn("[BROWSER DECODER] Decoder is closed. Resetting for next keyframe.");
        videoDecoder = null;
        videoCodecConfigured = false;
        return;
    }

    const timestamp = videoFrameCount * 33333;

    try {
        const chunk = new EncodedVideoChunk({
            type: isKey ? 'key' : 'delta',
            timestamp: timestamp,
            data: h264Payload
        });

        console.log(
            "[BROWSER DECODE]",
            {
                chunkType: chunk.type,
                timestamp: chunk.timestamp,
                byteLength: chunk.byteLength
            }
        );

        videoDecoder.decode(chunk);
        videoFrameCount++;

        setHud(
            "LIVE • FRAME " +
            videoFrameCount +
            " • " +
            width +
            "×" +
            height
        );
    } catch (e) {
        console.error("[BROWSER DECODER ERROR] decode() exception:", e);
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
    let buffer;
    if (event.data instanceof ArrayBuffer) {
        buffer = event.data;
    } else if (event.data instanceof Blob) {
        buffer = await event.data.arrayBuffer();
    } else {
        console.warn("[WS] Non-binary message:", event.data);
        return;
    }

    if (!buffer || buffer.byteLength === 0) {
        return;
    }

    const type =
        new Uint8Array(buffer)[0];


    console.log(
        "[WS] Packet:",
        {
            type,
            size: buffer.byteLength
        }
    );


    /*
     * TYPE 99
     * STREAM STATUS
     */

    if (type === 2) {

        console.log("[VIDEO] Stream-active packet received.");

        setHud(
            "STREAM ACTIVE • WAITING FOR FRAME"
        );

        return;

    }


    /*
     * TYPE 13 or TYPE 15
     * H.264 VIDEO FRAME
     *
     * Header: 1 byte type + 12 bytes (width:u32, height:u32, size:u32)
     */

    if (type === 13 || type === 15) {

        await handleVideoPacket(
            buffer
        );

        return;

    }


    /*
     * TYPE 17
     * AUDIO
     */

    if (type === 17) {

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
        "[WS] Unknown packet:",
        type,
        buffer.byteLength
    );

}


/* ============================================================
   INITIALIZATION PACKET
============================================================ */

function sendInitializationPacket() {

    if (!isSocketOpen()) {

        return;

    }

    /*
     * 39 bytes total
     *
     * Byte 0:
     *     2
     *
     * Byte 1..38:
     *     device UID
     */

    const pkt =
        new Uint8Array(39);

    pkt[0] = 2;

    const deviceBytes =
        new TextEncoder().encode(
            DEVICE_ID
        );

    const copyLength =
        Math.min(
            deviceBytes.length,
            38
        );

    pkt.set(
        deviceBytes.slice(
            0,
            copyLength
        ),
        1
    );

    ws.send(pkt);

    console.log(
        "[WS] Initialization sent:",
        DEVICE_ID
    );

}


/* ============================================================
   START STREAM
============================================================ */

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
            "Unable to create canvas."
        );

        return;

    }


    videoFrameCount = 0;

    renderWidth = 0;

    renderHeight = 0;

    videoDecodeBusy = false;

    safeCloseImage();


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

            console.log(
                "[WS] Connected:",
                WS_URL
            );

            setHud(
                "AUTHENTICATING..."
            );

            sendInitializationPacket();

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


    if (
        mediaRecorder &&
        mediaRecorder.state ===
        "recording"
    ) {

        try {

            mediaRecorder.stop();

        } catch (e) {}

    }


    if (ws) {

        try {

            ws.onopen = null;

            ws.onmessage = null;

            ws.onerror = null;

            ws.onclose = null;

            ws.close();

        } catch (e) {}

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


    chatPanel.classList.remove(
        "open"
    );

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


    /*
     * Canvas uses object-fit: contain.
     *
     * Therefore the visible image may have
     * letterboxing.
     *
     * We calculate the actual displayed
     * image rectangle.
     */

    const scale =
        Math.min(
            rect.width / canvas.width,
            rect.height / canvas.height
        );


    const displayedWidth =
        canvas.width * scale;

    const displayedHeight =
        canvas.height * scale;


    const offsetX =
        (rect.width -
            displayedWidth) / 2;


    const offsetY =
        (rect.height -
            displayedHeight) / 2;


    let x =
        event.clientX -
        rect.left -
        offsetX;


    let y =
        event.clientY -
        rect.top -
        offsetY;


    x =
        Math.max(
            0,
            Math.min(
                displayedWidth,
                x
            )
        );


    y =
        Math.max(
            0,
            Math.min(
                displayedHeight,
                y
            )
        );


    const canvasX =
        x / scale;


    const canvasY =
        y / scale;


    const normalizedX =
        Math.max(
            0,
            Math.min(
                65535,
                Math.floor(
                    canvasX /
                    canvas.width *
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
                    canvasY /
                    canvas.height *
                    65535
                )
            )
        );


    return {

        x: normalizedX,

        y: normalizedY

    };

}


/* ============================================================
   MOUSE MOVE
============================================================ */

function sendMouseMove(event) {

    if (!isSocketOpen()) {

        return;

    }

    const coords =
        getCoordinates(event);

    if (!coords) {

        return;

    }

    const pkt =
        new Uint8Array(9);

    pkt[0] = 0;

    pkt[1] =
        (coords.x >> 8) & 0xff;

    pkt[2] =
        coords.x & 0xff;

    pkt[3] =
        (coords.y >> 8) & 0xff;

    pkt[4] =
        coords.y & 0xff;

    ws.send(pkt);

}


/* ============================================================
   MOUSE DOWN
============================================================ */

function sendMouseDown(event) {

    if (!isSocketOpen()) {

        return;

    }

    const coords =
        getCoordinates(event);

    if (!coords) {

        return;

    }

    const type =
        event.button === 2
            ? 3
            : 1;

    const pkt =
        new Uint8Array(9);

    pkt[0] = type;

    pkt[1] =
        (coords.x >> 8) & 0xff;

    pkt[2] =
        coords.x & 0xff;

    pkt[3] =
        (coords.y >> 8) & 0xff;

    pkt[4] =
        coords.y & 0xff;

    ws.send(pkt);

}


/* ============================================================
   MOUSE UP
============================================================ */

function sendMouseUp(event) {

    if (!isSocketOpen()) {

        return;

    }

    const type =
        event.button === 2
            ? 4
            : 2;

    const pkt =
        new Uint8Array(9);

    pkt[0] = type;

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

    const scroll =
        event.deltaY > 0
            ? -120
            : 120;

    const pkt =
        new Uint8Array(9);

    pkt[0] = 8;

    pkt[1] = 0;

    pkt[2] = 0;

    pkt[3] =
        (scroll >> 8) & 0xff;

    pkt[4] =
        scroll & 0xff;

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

    const keyCode =
        event.keyCode ||
        event.which;

    const pkt =
        new Uint8Array(5);

    pkt[0] = type;

    pkt[1] =
        (keyCode >> 24) & 0xff;

    pkt[2] =
        (keyCode >> 16) & 0xff;

    pkt[3] =
        (keyCode >> 8) & 0xff;

    pkt[4] =
        keyCode & 0xff;

    ws.send(pkt);

}


/* ============================================================
   CANVAS EVENTS
============================================================ */

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
        event => {

            canvas.focus();

            sendMouseDown(event);

        }
    );


    canvas.addEventListener(
        "mouseup",
        sendMouseUp
    );


    canvas.addEventListener(
        "contextmenu",
        event => {

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
        event => {

            sendKeyboard(
                event,
                5
            );

        }
    );


    canvas.addEventListener(
        "keyup",
        event => {

            sendKeyboard(
                event,
                6
            );

        }
    );

}


/* ============================================================
   DRAG & DROP
============================================================ */

function attachDropEvents() {

    streamBox.addEventListener(
        "dragover",
        event => {

            event.preventDefault();

            dropOverlay.classList.add(
                "active"
            );

        }
    );


    streamBox.addEventListener(
        "dragleave",
        event => {

            event.preventDefault();

            dropOverlay.classList.remove(
                "active"
            );

        }
    );


    streamBox.addEventListener(
        "drop",
        async event => {

            event.preventDefault();

            dropOverlay.classList.remove(
                "active"
            );

            if (!isSocketOpen()) {

                alert(
                    "WebSocket not connected."
                );

                return;

            }

            await sendFiles(
                event.dataTransfer.files
            );

        }
    );

}


/* ============================================================
   BUTTON EVENTS
============================================================ */

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
    event => {

        if (event.key === "Enter") {

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


/* ============================================================
   INITIALIZE
============================================================ */

canvas =
    document.getElementById(
        "remoteCanvas"
    );

attachCanvasEvents();

attachDropEvents();


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