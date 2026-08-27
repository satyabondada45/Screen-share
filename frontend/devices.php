<?php
if (session_status() === PHP_SESSION_NONE) {
    session_start();
}

$dbPath = __DIR__ . '/../backend/config/database.php';
if (!isset($pdo) || $pdo === null) {
    if (file_exists($dbPath)) {
        require $dbPath;
    }
}

$currentUserId = $_SESSION['user_id'] ?? null;
$currentUsername = $_SESSION['username'] ?? 'User';

$hostDevice = null;
if ($pdo) {
    if ($currentUserId) {
        $stmt = $pdo->prepare("SELECT * FROM devices WHERE user_id = ? AND machine_identifier IS NOT NULL ORDER BY is_online DESC, last_seen_at DESC, id ASC LIMIT 1");
        $stmt->execute([$currentUserId]);
        $hostDevice = $stmt->fetch(PDO::FETCH_ASSOC);
    }
    if (!$hostDevice) {
        $clientIp = $_SERVER['HTTP_X_FORWARDED_FOR'] ?? $_SERVER['REMOTE_ADDR'] ?? '127.0.0.1';
        $stmt = $pdo->prepare("SELECT * FROM devices WHERE machine_identifier IS NOT NULL AND (ip_address = ? OR ip_address = '127.0.0.1' OR ip_address = '::1') ORDER BY is_online DESC, last_seen_at DESC, id DESC LIMIT 1");
        $stmt->execute([$clientIp]);
        $hostDevice = $stmt->fetch(PDO::FETCH_ASSOC);
    }
}

$hostUid = $hostDevice['system_id'] ?? $hostDevice['device_uid'] ?? 'Connecting...';
$hostName = $hostDevice['name'] ?? ($currentUsername . '-PC');

function formatDeviceId($id) {
    $clean = preg_replace('/[^0-9]/', '', (string)$id);
    if (strlen($clean) === 9) {
        return substr($clean, 0, 3) . ' ' . substr($clean, 3, 3) . ' ' . substr($clean, 6, 3);
    }
    return chunk_split($clean, 3, ' ');
}
?>
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>DeskStream - Remote Desktop Dashboard</title>
    <style>
        :root {
            --bg-main: #f1f5f9;
            --card-bg: #ffffff;
            --border: #e2e8f0;
            --primary: #ef4444;
            --primary-hover: #dc2626;
            --text-main: #0f172a;
            --text-muted: #64748b;
            --online: #22c55e;
            --offline: #64748b;
            --accent: #3b82f6;
            --accent-hover: #2563eb;
        }

        * { box-sizing: border-box; margin: 0; padding: 0; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; }
        body { background-color: var(--bg-main); color: var(--text-main); height: 100vh; display: flex; flex-direction: column; overflow: hidden; }

        /* Top Title Bar */
        .topbar {
            height: 60px; background: var(--card-bg); border-bottom: 1px solid var(--border);
            display: flex; justify-content: space-between; align-items: center; padding: 0 20px; flex-shrink: 0;
        }
        .brand { display: flex; align-items: center; gap: 10px; font-weight: 700; font-size: 1.25rem; color: #0f172a; }
        .brand span { color: var(--primary); }
        .topbar-center { display: flex; align-items: center; gap: 10px; }
        .topbar-btn {
            display: inline-flex; align-items: center; gap: 6px; padding: 6px 14px; background: #fff;
            border: 1px solid var(--border); border-radius: 8px; font-weight: 600; font-size: 0.85rem; color: var(--primary); cursor: pointer;
        }
        .topbar-btn-icon { background: #f8fafc; border: 1px solid var(--border); border-radius: 8px; width: 34px; height: 34px; display: flex; align-items: center; justify-content: center; cursor: pointer; color: var(--text-muted); font-size: 1.1rem; }
        .window-controls { display: flex; gap: 15px; color: var(--text-muted); cursor: pointer; font-size: 1.1rem; }

        /* Main Workspace Layout */
        .workspace { display: flex; flex: 1; overflow: hidden; }

        /* Sidebar */
        .sidebar {
            width: 280px; background: var(--card-bg); border-right: 1px solid var(--border);
            display: flex; flex-direction: column; justify-content: space-between; padding: 20px 16px; flex-shrink: 0;
        }
        .nav-menu { display: flex; flex-direction: column; gap: 6px; }
        .nav-item {
            display: flex; align-items: center; gap: 12px; padding: 12px 16px; border-radius: 10px;
            text-decoration: none; color: var(--text-muted); font-weight: 500; font-size: 0.95rem; transition: 0.2s;
            cursor: pointer; border: none; background: transparent; text-align: left; width: 100%;
        }
        .nav-item:hover, .nav-item.active { background: #fef2f2; color: var(--primary); }
        .nav-item.active { font-weight: 600; }

        .sidebar-device-box {
            background: #f8fafc; border: 1px solid var(--border); border-radius: 12px; padding: 14px;
        }
        .status-badge { display: flex; align-items: center; gap: 6px; font-size: 0.8rem; color: var(--online); font-weight: 600; margin-bottom: 6px; }
        .status-dot { width: 8px; height: 8px; background: var(--online); border-radius: 50%; box-shadow: 0 0 6px var(--online); }

        /* Content Area */
        .content { flex: 1; padding: 24px; overflow-y: auto; display: flex; flex-direction: column; gap: 24px; background: var(--bg-main); }

        /* Top Grid Panels */
        .top-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 24px; }
        .panel {
            background: var(--card-bg); border: 1px solid var(--border); border-radius: 16px; padding: 24px;
            box-shadow: 0 4px 6px -1px rgba(0,0,0,0.02); position: relative; display: flex; flex-direction: column; justify-content: space-between;
        }
        .panel h3 { font-size: 1.15rem; font-weight: 700; margin-bottom: 4px; color: var(--text-main); }
        .panel p { color: var(--text-muted); font-size: 0.85rem; margin-bottom: 16px; }

        .id-row { display: flex; align-items: center; justify-content: space-between; margin: 10px 0; }
        .id-display { font-family: monospace; font-size: 2.1rem; font-weight: 700; color: var(--primary); letter-spacing: 1.5px; }
        .copy-btn {
            background: #fff; border: 1px solid var(--border); border-radius: 10px; width: 44px; height: 44px;
            display: flex; align-items: center; justify-content: center; cursor: pointer; color: var(--text-muted); transition: 0.2s;
        }
        .copy-btn:hover { background: #f8fafc; color: var(--text-main); border-color: #cbd5e1; }

        .alias-row { display: flex; align-items: center; gap: 6px; font-size: 0.9rem; color: var(--text-muted); margin-top: 4px; }
        .alias-row strong { color: var(--text-main); }
        .edit-icon { cursor: pointer; color: var(--text-muted); font-size: 0.85rem; }

        /* Connect to Remote Device Panel */
        .connect-input-group { display: flex; gap: 10px; margin-bottom: 16px; }
        .connect-input-group select, .connect-input-group input {
            flex: 1; padding: 12px 14px; border: 1px solid var(--border); border-radius: 10px; font-size: 0.95rem; outline: none; background: #fff; color: var(--text-main);
        }
        .connect-input-group input:focus { border-color: var(--primary); }
        .btn-connect-action {
            background: var(--primary); color: white; border: none; padding: 0 22px; border-radius: 10px; font-weight: 600; cursor: pointer; transition: 0.2s; display: flex; align-items: center; gap: 6px;
        }
        .btn-connect-action:hover { background: var(--primary-hover); }

        .mode-selectors { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
        .mode-card {
            border: 1px solid var(--border); border-radius: 12px; padding: 12px 14px; display: flex; align-items: center; gap: 12px; cursor: pointer; background: #fafafa; transition: 0.2s;
        }
        .mode-card.selected { border-color: var(--primary); background: #fef2f2; }

        /* Recent Sessions Section */
        .section-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 12px; }
        .section-header h4 { font-size: 1.05rem; font-weight: 700; }
        .section-header a { color: var(--primary); text-decoration: none; font-size: 0.85rem; font-weight: 600; }

        .grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 16px; }
        .session-card {
            background: var(--card-bg); border: 1px solid var(--border); border-radius: 14px; padding: 16px;
            display: flex; align-items: center; justify-content: space-between; transition: 0.2s; cursor: pointer;
        }
        .session-card:hover { border-color: #cbd5e1; box-shadow: 0 4px 12px rgba(0,0,0,0.03); transform: translateY(-1px); }
        .session-info { display: flex; align-items: center; gap: 12px; overflow: hidden; }
        .session-icon { width: 42px; height: 42px; border-radius: 10px; display: flex; align-items: center; justify-content: center; color: white; font-weight: bold; flex-shrink: 0; }
        .session-details { overflow: hidden; }
        .session-details h5 { font-size: 0.95rem; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
        .session-details p { font-size: 0.8rem; color: var(--text-muted); margin-top: 2px; font-family: monospace; }
        .session-time { font-size: 0.75rem; color: var(--text-muted); display: flex; align-items: center; gap: 4px; margin-top: 4px; }
        .session-actions { display: flex; flex-direction: column; align-items: flex-end; gap: 8px; color: var(--text-muted); }

        .empty-state {
            text-align: center; grid-column: 1 / -1; padding: 40px; color: var(--text-muted);
            background: var(--card-bg); border: 1px dashed var(--border); border-radius: 12px; font-size: 0.9rem;
        }

        /* Footer Status Bar */
        .footer-bar {
            height: 36px; background: var(--card-bg); border-top: 1px solid var(--border);
            display: flex; justify-content: space-between; align-items: center; padding: 0 20px; font-size: 0.8rem; color: var(--text-muted); flex-shrink: 0;
        }
        .footer-status { display: flex; align-items: center; gap: 6px; color: var(--online); font-weight: 500; }
    </style>
</head>
<body>

    <!-- Top Window Titlebar -->
    <div class="topbar">
        <div class="brand">
            <svg width="24" height="24" viewBox="0 0 24 24" fill="var(--primary)"><path d="M12 2L2 7l10 5 10-5-10-5zM2 17l10 5 10-5M2 12l10 5 10-5"/></svg>
            DeskStream
        </div>
        <div class="topbar-center">
            <button class="topbar-btn">🖥️ New Session</button>
            <div class="topbar-btn-icon">+</div>
        </div>
        <div class="window-controls">
            <span>&minus;</span>
            <span>&#x25a2;</span>
            <span>&times;</span>
        </div>
    </div>

    <!-- Main Workspace -->
    <div class="workspace">
        <!-- Sidebar -->
        <div class="sidebar">
            <div class="nav-menu">
                <button class="nav-item active">🖥️ New Session</button>
                <button class="nav-item" onclick="alert('Feature coming soon')">⏱️ Recent Sessions</button>
                <button class="nav-item" onclick="alert('Feature coming soon')">📖 Address Book</button>
                <hr style="border:0; border-top:1px solid var(--border); margin: 6px 0;">
                <button class="nav-item" onclick="alert('Feature coming soon')">⚙️ Settings</button>
                <button class="nav-item" onclick="alert('Feature coming soon')">ℹ️ About</button>
            </div>

            <!-- Sidebar Mini ID Box -->
            <div class="sidebar-device-box">
                <div class="status-badge"><div class="status-dot"></div> Ready for connections</div>
                <div style="font-size: 0.75rem; color: var(--text-muted);">Your ID</div>
                <div id="sidebarId" style="font-family: monospace; font-size: 1.05rem; font-weight: bold; margin: 2px 0; color: var(--text-main);"><?= htmlspecialchars(formatDeviceId($hostUid)) ?></div>
                <div class="alias-row" style="font-size: 0.75rem;">Alias: <strong id="sidebarAlias"><?= htmlspecialchars($hostName) ?></strong></div>
            </div>
        </div>

        <!-- Content Area -->
        <div class="content">
            <!-- Top Controls Grid -->
            <div class="top-grid">
                <!-- This Device Panel -->
                <div class="panel">
                    <div>
                        <h3>This Device</h3>
                        <p>Your device can be accessed with this address.</p>
                        <div style="font-size: 0.8rem; color: var(--text-muted);">Your ID</div>
                        <div class="id-row">
                            <div class="id-display" id="mainIdDisplay"><?= htmlspecialchars(formatDeviceId($hostUid)) ?></div>
                            <div class="copy-btn" onclick="copyId()" title="Copy ID">📋</div>
                        </div>
                        <div class="alias-row">
                            Alias: <strong id="mainAliasDisplay"><?= htmlspecialchars($hostName) ?></strong> <span class="edit-icon">✏️</span>
                        </div>
                    </div>
                    <div style="margin-top: 20px;" class="status-badge"><div class="status-dot"></div> Ready for connections</div>
                </div>

                <!-- Connect to Remote Device Panel -->
                <div class="panel">
                    <div>
                        <h3>Connect to Remote Device</h3>
                        <p>Enter the remote device ID to start a session.</p>
                        
                        <div class="connect-input-group">
                            <input type="text" id="remoteIdInput" placeholder="Enter Remote ID (e.g. 123456789)" autocomplete="off">
                            <button class="btn-connect-action" onclick="connectToRemote()">Connect &rarr;</button>
                        </div>

                        <div class="mode-selectors">
                            <div class="mode-card selected" id="modeDesktop" onclick="setMode('desktop')">
                                <span style="font-size: 1.2rem;">💻</span>
                                <div><strong>Desktop</strong><div style="font-size:0.75rem; color:var(--text-muted)">Full remote control</div></div>
                            </div>
                            <div class="mode-card" id="modeView" onclick="setMode('view')">
                                <span style="font-size: 1.2rem;">👁️</span>
                                <div><strong>View Only</strong><div style="font-size:0.75rem; color:var(--text-muted)">View remote screen</div></div>
                            </div>
                        </div>
                    </div>
                    <div id="sync-status" style="font-size: 0.75rem; color: var(--text-muted); text-align: right; margin-top: 10px;">Backend sync active</div>
                </div>
            </div>

            <!-- Recent Sessions Section -->
            <div>
                <div class="section-header">
                    <h4>Recent Sessions</h4>
                    <a href="#" onclick="fetchDevices(); return false;">Refresh List</a>
                </div>
                <div id="device-grid" class="grid">
                    <div class="empty-state">Loading registered workstations...</div>
                </div>
            </div>
        </div>
    </div>

    <!-- Footer Status Bar -->
    <div class="footer-bar">
        <span>🔒 Secure connection established.</span>
        <span class="footer-status"><div class="status-dot" style="width:6px;height:6px;"></div> Connection quality: Good</span>
    </div>

    <script>
        let currentMode = 'desktop';

        function setMode(mode) {
            currentMode = mode;
            document.getElementById('modeDesktop').classList.toggle('selected', mode === 'desktop');
            document.getElementById('modeView').classList.toggle('selected', mode === 'view');
        }

        function formatDeviceId(idStr) {
            if (!idStr) return '------';
            const clean = idStr.replace(/[^0-9]/g, '');
            if (clean.length === 9) {
                return clean.slice(0, 3) + ' ' + clean.slice(3, 6) + ' ' + clean.slice(6);
            }
            if (clean.length === 6) {
                return clean.slice(0, 3) + ' ' + clean.slice(3);
            }
            return idStr;
        }

        function copyId() {
            const idText = document.getElementById('mainIdDisplay').innerText;
            navigator.clipboard.writeText(idText.replace(/\s/g, ''));
            alert('Device ID copied to clipboard!');
        }

        function connectToRemote() {
            const rawId = document.getElementById('remoteIdInput').value.trim().replace(/[^0-9]/g, '');
            if (!rawId) {
                alert('Please enter a valid remote device ID.');
                return;
            }
            window.location.href = `remote/session.php?id=${rawId}`;
        }

        function initiateConnection(uid) {
            window.location.href = `remote/session.php?id=${uid}`;
        }

        async function fetchDevices() {
            try {
                const res = await fetch('../backend/api/devices/list.php');
                const data = await res.json();
                
                const grid = document.getElementById('device-grid');
                document.getElementById('sync-status').innerText = 'Synced at ' + new Date().toLocaleTimeString();

                if (!data.devices || data.devices.length === 0) {
                    grid.innerHTML = '<div class="empty-state">No other desktop agents registered yet. Run your Rust agent to populate this list.</div>';
                    return;
                }

                const localUid = '<?= htmlspecialchars($hostUid) ?>'.replace(/[^0-9]/g, '');
                const remoteDevices = data.devices.filter(d => d.device_uid.replace(/[^0-9]/g, '') !== localUid);

                const colors = ['#0284c7', '#ef4444', '#22c55e', '#8b5cf6', '#f59e0b', '#06b6d4'];

                grid.innerHTML = remoteDevices.map((device, index) => {
                    const color = colors[index % colors.length];
                    const formattedId = formatDeviceId(device.device_uid);
                    const isOnline = device.is_online == 1;

                    return `
                        <div class="session-card" onclick="initiateConnection('${device.device_uid}')">
                            <div class="session-info">
                                <div class="session-icon" style="background: ${color}">💻</div>
                                <div class="session-details">
                                    <h5>${device.name || 'Workstation'}</h5>
                                    <p>${formattedId}</p>
                                    <div class="session-time">
                                        <span>🕒</span> ${device.last_seen_at || 'Recently active'}
                                    </div>
                                </div>
                            </div>
                            <div class="session-actions">
                                <span style="font-size: 0.8rem;" title="${isOnline ? 'Online' : 'Offline'}">${isOnline ? '🟢' : '⚪'}</span>
                                <span style="font-size: 1.1rem; color: var(--text-muted);">&rsaquo;</span>
                            </div>
                        </div>
                    `;
                }).join('');
            } catch (err) {
                document.getElementById('sync-status').innerText = 'Polling offline';
            }
        }

        // Fetch immediately and poll every 5 seconds
        fetchDevices();
        setInterval(fetchDevices, 5000);
    </script>
</body>
</html>