<?php
require_once __DIR__ . '/../../backend/config/database.php';

$deviceUid = $_GET['id'] ?? null;

if (!$deviceUid) {
    header("Location: ../devices.php");
    exit;
}

$stmt = $pdo->prepare("SELECT * FROM devices WHERE device_uid = :device_uid LIMIT 1");
$stmt->execute([':device_uid' => $deviceUid]);
$device = $stmt->fetch();

if (!$device) {
    die("Error: Device not found in database.");
}
?>
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Active Remote Session - <?= htmlspecialchars($device['name']) ?></title>
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
        }

        * { box-sizing: border-box; margin: 0; padding: 0; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; }
        body { background-color: var(--bg); color: var(--text-main); min-height: 100vh; display: flex; flex-direction: column; }

        .topbar {
            height: 60px;
            background: #0f172a;
            border-bottom: 1px solid var(--border);
            display: flex;
            justify-content: space-between;
            align-items: center;
            padding: 0 24px;
        }

        .device-badge { display: flex; align-items: center; gap: 10px; }
        .dot {
            width: 10px; height: 10px; border-radius: 50%;
            background: <?= $device['is_online'] ? 'var(--online)' : 'var(--offline)' ?>;
            box-shadow: 0 0 10px <?= $device['is_online'] ? 'var(--online)' : 'transparent' ?>;
        }

        .container { max-width: 1100px; margin: 24px auto; padding: 0 20px; flex: 1; width: 100%; display: flex; flex-direction: column; }
        .session-card {
            background: var(--card-bg); border: 1px solid var(--border);
            border-radius: 14px; padding: 24px; box-shadow: 0 10px 25px rgba(0,0,0,0.3);
            display: flex; flex-direction: column; flex: 1;
        }

        .code-display {
            background: #0f172a; border: 1px dashed var(--accent);
            border-radius: 10px; padding: 14px; text-align: center; margin: 16px 0;
        }
        .session-code {
            font-family: monospace; font-size: 2rem; letter-spacing: 4px;
            color: #38bdf8; font-weight: 700;
        }

        .btn {
            display: inline-flex; align-items: center; justify-content: center; gap: 6px; padding: 10px 18px; border-radius: 8px;
            font-weight: 600; text-decoration: none; cursor: default; border: none;
            transition: 0.2s; text-align: center; font-size: 0.9rem;
        }
        .btn-primary { background: var(--accent); color: white; }
        .btn-primary:hover { background: var(--accent-hover); }
        .btn-success { background: #16a34a; color: white; }
        .btn-success:hover { background: #15803d; }
        .btn-danger { background: rgba(239, 68, 68, 0.15); color: var(--danger); border: 1px solid var(--danger); }
        .btn-danger:hover { background: var(--danger); color: white; }
        .btn-secondary { background: #475569; color: white; }
        .btn-secondary:hover { background: #334155; }
        .btn-warning { background: #eab308; color: #fff; }
        .btn-warning:hover { background: #ca8a04; }

        .toolbar {
            display: flex; gap: 10px; margin-bottom: 16px; flex-wrap: wrap; align-items: center;
            background: #0f172a; padding: 12px; border-radius: 8px; border: 1px solid var(--border);
        }

        select.monitor-select {
            padding: 10px 12px; border-radius: 8px; background: var(--card-bg); color: white;
            border: 1px solid var(--border); font-size: 0.9rem; outline: none; cursor: default;
        }

        .canvas-container {
            display: none; margin-top: 10px; background: #000;
            border-radius: 8px; overflow: hidden; border: 1px solid var(--border);
            position: relative; box-shadow: 0 12px 30px rgba(0,0,0,0.5); flex: 1;
            min-height: 500px;
        }

        canvas { width: 100%; height: 100%; object-fit: contain; display: block; cursor: default; outline: none; }

        .hud-badge {
            position: absolute; top: 12px; right: 12px;
            background: rgba(15, 23, 42, 0.85); border: 1px solid var(--border);
            padding: 6px 12px; border-radius: 6px; font-family: monospace;
            font-size: 0.8rem; color: var(--online); pointer-events: none; z-index: 10;
        }

        .drop-overlay {
            position: absolute; top: 0; left: 0; width: 100%; height: 100%;
            background: rgba(59, 130, 246, 0.85); display: flex; flex-direction: column;
            justify-content: center; align-items: center; color: white; font-size: 1.5rem;
            font-weight: bold; z-index: 20; opacity: 0; pointer-events: none; transition: 0.2s;
        }
        .drop-overlay.active { opacity: 1; pointer-events: auto; }

        /* Chat Panel */
        .chat-panel {
            position: fixed; bottom: 20px; right: 20px; width: 320px;
            background: var(--card-bg); border: 1px solid var(--border); border-radius: 10px;
            box-shadow: 0 10px 30px rgba(0,0,0,0.6); display: flex; flex-direction: column;
            overflow: hidden; z-index: 50; transform: translateY(150%); transition: transform 0.3s ease;
        }
        .chat-panel.open { transform: translateY(0); }
        .chat-header {
            background: #0f172a; padding: 12px 16px; font-weight: bold; font-size: 0.95rem;
            border-bottom: 1px solid var(--border); display: flex; justify-content: space-between; align-items: center;
        }
        .chat-close { cursor: pointer; color: var(--text-muted); font-size: 1.2rem; line-height: 1; }
        .chat-close:hover { color: white; }
        .chat-messages {
            height: 250px; overflow-y: auto; padding: 12px; font-size: 0.85rem;
            display: flex; flex-direction: column; gap: 8px;
        }
        .msg { padding: 8px 12px; border-radius: 8px; max-width: 85%; word-wrap: break-word; }
        .msg.sent { background: var(--accent); align-self: flex-end; color: white; }
        .msg.recv { background: #334155; align-self: flex-start; color: white; }
        .chat-input {
            display: flex; border-top: 1px solid var(--border); background: #0f172a;
        }
        .chat-input input {
            flex: 1; background: transparent; border: none; color: white; padding: 12px;
            outline: none; font-size: 0.9rem;
        }
        .chat-input button {
            background: var(--accent); border: none; color: white; padding: 0 16px;
            cursor: pointer; font-weight: bold; transition: 0.2s;
        }
        .chat-input button:hover { background: var(--accent-hover); }

    </style>
</head>
<body>
    <div class="topbar">
        <div class="device-badge">
            <div class="dot"></div>
            <strong>Host: <?= htmlspecialchars($device['name']) ?></strong>
        </div>
        <a href="../devices.php" class="btn btn-danger" style="padding: 6px 14px; font-size: 0.8rem;">Back to Dashboard</a>
    </div>

    <div class="container">
        <div class="session-card">
            <h2>Remote Access Terminal</h2>
            <p style="color: var(--text-muted); margin-top: 4px;">Launch low-latency browser streaming or open the native desktop client.</p>

            <div class="code-display">
                <div style="font-size: 0.8rem; color: var(--text-muted); margin-bottom: 4px;">REMOTE SESSION ACCESS ID</div>
                <div class="session-code"><?= substr($device['device_uid'], 0, 3) . '-' . substr($device['device_uid'], 3) ?></div>
            </div>

            <div style="display: grid; grid-template-columns: 1fr 1fr; gap: 12px; margin-bottom: 16px;">
                <button class="btn btn-success" id="streamBtn" onclick="toggleWebStream()">Start Web Canvas Stream</button>
                <a href="screenshare://<?= htmlspecialchars($device['device_uid']) ?>" class="btn btn-primary">Launch Native Desktop Viewer</a>
            </div>

            <div class="toolbar" id="advToolbar" style="display: none;">
                <select class="monitor-select" id="monitorSelect" onchange="switchMonitor(this.value)">
                    <option value="0">Monitor 1 (Primary)</option>
                    <option value="1">Monitor 2</option>
                    <option value="2">Monitor 3</option>
                    <option value="3">Monitor 4</option>
                </select>
                <button class="btn btn-secondary" id="audioBtn" onclick="toggleAudio()">🔈 Listen Audio</button>
                <button class="btn btn-secondary" onclick="toggleChat()">💬 Chat</button>
                <button class="btn btn-secondary" id="recordBtn" onclick="toggleRecording()">🔴 Record Session</button>
            </div>

            <div id="stream-box" class="canvas-container">
                <div id="hud" class="hud-badge">CONNECTING...</div>
                <div id="dropOverlay" class="drop-overlay">📁 Drop files here to send to Remote Host (RemoteDrop/)</div>
                <canvas id="remoteCanvas" tabindex="0"></canvas>
            </div>
        </div>
    </div>

    <!-- Chat Panel -->
    <div class="chat-panel" id="chatPanel">
        <div class="chat-header">
            <span>Session Chat</span>
            <span class="chat-close" onclick="toggleChat()">&times;</span>
        </div>
        <div class="chat-messages" id="chatMessages"></div>
        <div class="chat-input">
            <input type="text" id="chatInput" placeholder="Type a message..." onkeypress="if(event.key === 'Enter') sendChat()">
            <button onclick="sendChat()">Send</button>
        </div>
    </div>

    <script>
        let ws = null;
        let isStreaming = false;
        const deviceId = "<?= htmlspecialchars($device['device_uid']) ?>";
        let canvas, ctx;
        let frameW = 0, frameH = 0;

        let audioCtx = null;
        let nextAudioTime = 0;
        let isAudioEnabled = false;

        let mediaRecorder = null;
        let recordedChunks = [];

        function toggleAudio() {
            isAudioEnabled = !isAudioEnabled;
            const btn = document.getElementById('audioBtn');
            if (isAudioEnabled) {
                btn.innerText = '🔊 Mute Audio';
                btn.classList.replace('btn-secondary', 'btn-warning');
                if (!audioCtx) {
                    audioCtx = new (window.AudioContext || window.webkitAudioContext)();
                }
                if (audioCtx.state === 'suspended') {
                    audioCtx.resume();
                }
            } else {
                btn.innerText = '🔈 Listen Audio';
                btn.classList.replace('btn-warning', 'btn-secondary');
            }
        }

        function toggleChat() {
            document.getElementById('chatPanel').classList.toggle('open');
            if (document.getElementById('chatPanel').classList.contains('open')) {
                document.getElementById('chatInput').focus();
            }
        }

        function sendChat() {
            const input = document.getElementById('chatInput');
            const msg = input.value.trim();
            if (!msg || !ws || ws.readyState !== WebSocket.OPEN) return;
            
            appendMessage('sent', msg);
            
            const encoder = new TextEncoder();
            const msgBytes = encoder.encode(msg);
            const pkt = new Uint8Array(3 + msgBytes.length);
            pkt[0] = 16;
            pkt[1] = (msgBytes.length >> 8) & 0xFF;
            pkt[2] = msgBytes.length & 0xFF;
            pkt.set(msgBytes, 3);
            ws.send(pkt);
            
            input.value = '';
        }

        function appendMessage(type, text) {
            const msgs = document.getElementById('chatMessages');
            const div = document.createElement('div');
            div.className = `msg ${type}`;
            div.innerText = text;
            msgs.appendChild(div);
            msgs.scrollTop = msgs.scrollHeight;
        }

        function switchMonitor(index) {
            if (!ws || ws.readyState !== WebSocket.OPEN) return;
            const pkt = new Uint8Array(9);
            pkt[0] = 7;
            pkt[1] = parseInt(index);
            ws.send(pkt);
        }

        function toggleRecording() {
            const btn = document.getElementById('recordBtn');
            if (mediaRecorder && mediaRecorder.state === 'recording') {
                mediaRecorder.stop();
                btn.innerText = '🔴 Record Session';
                btn.classList.replace('btn-danger', 'btn-secondary');
                return;
            }
            
            if (!canvas) return;
            const stream = canvas.captureStream(30);
            
            try {
                mediaRecorder = new MediaRecorder(stream, { mimeType: 'video/webm' });
            } catch (e) {
                console.error('MediaRecorder error:', e);
                alert('Session recording is not supported in this browser format.');
                return;
            }
            
            recordedChunks = [];
            mediaRecorder.ondataavailable = e => {
                if (e.data.size > 0) recordedChunks.push(e.data);
            };
            
            mediaRecorder.onstop = () => {
                const blob = new Blob(recordedChunks, { type: 'video/webm' });
                const url = URL.createObjectURL(blob);
                const a = document.createElement('a');
                a.href = url;
                a.download = `session-${deviceId}-${new Date().getTime()}.webm`;
                a.click();
                URL.revokeObjectURL(url);
            };
            
            mediaRecorder.start();
            btn.innerText = '⏹ Stop Recording';
            btn.classList.replace('btn-secondary', 'btn-danger');
        }

        function toggleWebStream() {
            if (isStreaming) {
                if (ws) ws.close();
                document.getElementById('stream-box').style.display = 'none';
                document.getElementById('advToolbar').style.display = 'none';
                document.getElementById('streamBtn').innerText = 'Start Web Canvas Stream';
                document.getElementById('streamBtn').className = 'btn btn-success';
                isStreaming = false;
                
                if (mediaRecorder && mediaRecorder.state === 'recording') {
                    toggleRecording();
                }
                return;
            }

            document.getElementById('stream-box').style.display = 'block';
            document.getElementById('advToolbar').style.display = 'flex';
            document.getElementById('streamBtn').innerText = 'Disconnect Web Stream';
            document.getElementById('streamBtn').className = 'btn btn-danger';
            isStreaming = true;

            canvas = document.getElementById('remoteCanvas');
            ctx = canvas.getContext('2d', { alpha: false });
            canvas.focus();

            // ws = new WebSocket("ws://127.0.0.1:9001");
            ws = new WebSocket("ws://192.168.29.229:9001");
            ws.binaryType = "arraybuffer";

            ws.onopen = () => {
                const initPkt = new Uint8Array(39);
                initPkt[0] = 2;
                for (let i = 0; i < 6; i++) {
                    initPkt[i + 1] = deviceId.charCodeAt(i);
                }
                ws.send(initPkt);
            };

            ws.onmessage = async (event) => {
                const buffer = event.data;
                const view = new DataView(buffer);
                const type = view.getUint8(0);

                if (buffer.byteLength === 1 && type === 1) {
                    document.getElementById('hud').innerText = "LIVE STREAM ACTIVE";
                    return;
                }

                if (type === 1 && buffer.byteLength >= 13) {
                    const frameW = view.getUint32(1);
                    const frameH = view.getUint32(5);
                    const compSize = view.getUint32(9);

                    if (13 + compSize > buffer.byteLength) {
                        console.warn("Incomplete frame buffer received");
                        return;
                    }

                    if (canvas.width !== frameW || canvas.height !== frameH) {
                        canvas.width = frameW;
                        canvas.height = frameH;
                    }

                    const pngSlice = new Uint8Array(buffer, 13, compSize);
                    const blob = new Blob([pngSlice], { type: 'image/png' });

                    createImageBitmap(blob).then(bmp => {
                        ctx.drawImage(bmp, 0, 0, frameW, frameH);
                        bmp.close();
                    }).catch(err => {
                        console.error("Frame render error:", err);
                    });
                    return;
                }

                if (type === 13 && isAudioEnabled) {
                    const byteLen = view.getUint32(1);
                    const sampleRate = view.getUint32(5);
                    const channels = view.getUint16(9);
                    
                    const floatArray = new Float32Array(buffer.slice(11, 11 + byteLen));
                    
                    if (!audioCtx) {
                        audioCtx = new (window.AudioContext || window.webkitAudioContext)();
                    }
                    
                    const validChannels = channels > 0 ? channels : 2;
                    const validRate = sampleRate > 0 ? sampleRate : 48000;
                    
                    const frames = floatArray.length / validChannels;
                    if (frames === 0) return;

                    const audioBuffer = audioCtx.createBuffer(validChannels, frames, validRate);
                    
                    for (let c = 0; c < validChannels; c++) {
                        const channelData = audioBuffer.getChannelData(c);
                        for (let i = 0; i < frames; i++) {
                            channelData[i] = floatArray[i * validChannels + c];
                        }
                    }
                    
                    const source = audioCtx.createBufferSource();
                    source.buffer = audioBuffer;
                    source.connect(audioCtx.destination);
                    
                    if (nextAudioTime < audioCtx.currentTime) {
                        nextAudioTime = audioCtx.currentTime + 0.05;
                    }
                    source.start(nextAudioTime);
                    nextAudioTime += audioBuffer.duration;
                    return;
                }

                if (type === 16) {
                    const len = (view.getUint8(1) << 8) | view.getUint8(2);
                    const msgBytes = new Uint8Array(buffer, 3, len);
                    const txt = new TextDecoder().decode(msgBytes);
                    appendMessage('recv', txt);
                    if (!document.getElementById('chatPanel').classList.contains('open')) {
                        toggleChat();
                    }
                    return;
                }
            };

            const streamBox = document.getElementById('stream-box');
            const dropOverlay = document.getElementById('dropOverlay');

            streamBox.addEventListener('dragover', (e) => {
                e.preventDefault();
                dropOverlay.classList.add('active');
            });

            streamBox.addEventListener('dragleave', (e) => {
                e.preventDefault();
                dropOverlay.classList.remove('active');
            });

            streamBox.addEventListener('drop', async (e) => {
                e.preventDefault();
                dropOverlay.classList.remove('active');

                if (!ws || ws.readyState !== WebSocket.OPEN) {
                    alert("WebSocket not connected!");
                    return;
                }

                const files = e.dataTransfer.files;
                if (files.length === 0) return;

                for (let file of files) {
                    const nameBytes = new TextEncoder().encode(file.name);
                    const fileSize = file.size;

                    const metaPkt = new Uint8Array(1 + 2 + 8 + nameBytes.length);
                    metaPkt[0] = 20;
                    metaPkt[1] = (nameBytes.length >> 8) & 0xFF;
                    metaPkt[2] = nameBytes.length & 0xFF;

                    const view = new DataView(metaPkt.buffer);
                    view.setBigUint64(3, BigInt(fileSize), false);
                    metaPkt.set(nameBytes, 11);
                    ws.send(metaPkt);

                    const chunkSize = 32768;
                    let offset = 0;
                    while (offset < file.size) {
                        const slice = file.slice(offset, offset + chunkSize);
                        const arrayBuffer = await slice.arrayBuffer();
                        const chunkBytes = new Uint8Array(arrayBuffer);

                        const chunkPkt = new Uint8Array(1 + 4 + chunkBytes.length);
                        chunkPkt[0] = 21;
                        const chunkView = new DataView(chunkPkt.buffer);
                        chunkView.setUint32(1, chunkBytes.length, false);
                        chunkPkt.set(chunkBytes, 5);
                        ws.send(chunkPkt);

                        offset += chunkSize;
                    }
                    alert(`File "${file.name}" sent successfully to Host's RemoteDrop folder!`);
                }
            });

            // Robust Mouse Mapping using direct Canvas Internal Scaling Buffer
            canvas.addEventListener('mousemove', (e) => {
                if (!ws || ws.readyState !== WebSocket.OPEN || !canvas.width || !canvas.height) return;
                const rect = canvas.getBoundingClientRect();
                
                // Map coordinates securely based on layout bounds versus native buffer resolution
                const scaleX = canvas.width / rect.width;
                const scaleY = canvas.height / rect.height;
                
                const canvasX = (e.clientX - rect.left) * scaleX;
                const canvasY = (e.clientY - rect.top) * scaleY;

                const normX = Math.max(0, Math.min(65535, Math.floor((canvasX / canvas.width) * 65535)));
                const normY = Math.max(0, Math.min(65535, Math.floor((canvasY / canvas.height) * 65535)));

                const pkt = new Uint8Array(9);
                pkt[0] = 0;
                pkt[1] = (normX >> 8) & 0xFF;
                pkt[2] = normX & 0xFF;
                pkt[3] = (normY >> 8) & 0xFF;
                pkt[4] = normY & 0xFF;
                ws.send(pkt);
            });

            canvas.addEventListener('mousedown', (e) => {
                if (!ws || ws.readyState !== WebSocket.OPEN || !canvas.width || !canvas.height) return;
                canvas.focus();
                const rect = canvas.getBoundingClientRect();
                
                const scaleX = canvas.width / rect.width;
                const scaleY = canvas.height / rect.height;
                
                const canvasX = (e.clientX - rect.left) * scaleX;
                const canvasY = (e.clientY - rect.top) * scaleY;

                const normX = Math.max(0, Math.min(65535, Math.floor((canvasX / canvas.width) * 65535)));
                const normY = Math.max(0, Math.min(65535, Math.floor((canvasY / canvas.height) * 65535)));

                const pkt = new Uint8Array(9);
                pkt[0] = e.button === 2 ? 3 : 1;
                pkt[1] = (normX >> 8) & 0xFF;
                pkt[2] = normX & 0xFF;
                pkt[3] = (normY >> 8) & 0xFF;
                pkt[4] = normY & 0xFF;
                ws.send(pkt);
            });

            canvas.addEventListener('mouseup', (e) => {
                if (!ws || ws.readyState !== WebSocket.OPEN) return;
                const pkt = new Uint8Array(9);
                pkt[0] = e.button === 2 ? 4 : 2;
                ws.send(pkt);
            });

            canvas.addEventListener('contextmenu', e => e.preventDefault());

            canvas.addEventListener('keydown', (e) => {
                if (!ws || ws.readyState !== WebSocket.OPEN) return;
                e.preventDefault();
                const pkt = new Uint8Array(5);
                pkt[0] = 5;
                const keyCode = e.keyCode || e.which;
                pkt[1] = (keyCode >> 24) & 0xFF;
                pkt[2] = (keyCode >> 16) & 0xFF;
                pkt[3] = (keyCode >> 8) & 0xFF;
                pkt[4] = keyCode & 0xFF;
                ws.send(pkt);
            });

            canvas.addEventListener('keyup', (e) => {
                if (!ws || ws.readyState !== WebSocket.OPEN) return;
                e.preventDefault();
                const pkt = new Uint8Array(5);
                pkt[0] = 6;
                const keyCode = e.keyCode || e.which;
                pkt[1] = (keyCode >> 24) & 0xFF;
                pkt[2] = (keyCode >> 16) & 0xFF;
                pkt[3] = (keyCode >> 8) & 0xFF;
                pkt[4] = keyCode & 0xFF;
                ws.send(pkt);
            });
        }
    </script>
</body>
</html>