<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Managed Devices - Remote Control Dashboard</title>
    <style>
        :root {
            --bg: #0f172a;
            --card-bg: #1e293b;
            --card-border: #334155;
            --text-main: #f8fafc;
            --text-muted: #94a3b8;
            --accent: #3b82f6;
            --accent-hover: #2563eb;
            --online: #22c55e;
            --offline: #64748b;
        }

        * { box-sizing: border-box; margin: 0; padding: 0; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; }
        body { background-color: var(--bg); color: var(--text-main); padding: 40px 20px; }
        .container { max-width: 1100px; margin: 0 auto; }
        
        header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 30px; border-bottom: 1px solid var(--card-border); padding-bottom: 20px; }
        h1 { font-size: 1.6rem; font-weight: 700; }
        .subtitle { color: var(--text-muted); font-size: 0.9rem; margin-top: 4px; }
        
        .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(320px, 1fr)); gap: 20px; }
        .card { background: var(--card-bg); border: 1px solid var(--card-border); border-radius: 12px; padding: 20px; display: flex; flex-direction: column; justify-content: space-between; transition: transform 0.2s, border-color 0.2s; }
        .card:hover { transform: translateY(-2px); border-color: var(--accent); }
        
        .card-header { display: flex; justify-content: space-between; align-items: flex-start; margin-bottom: 12px; }
        .device-name { font-size: 1.1rem; font-weight: 600; color: var(--text-main); }
        .badge { font-size: 0.75rem; padding: 4px 10px; border-radius: 9999px; font-weight: 600; text-transform: uppercase; }
        .badge.online { background: rgba(34, 197, 94, 0.15); color: var(--online); border: 1px solid var(--online); }
        .badge.offline { background: rgba(100, 116, 139, 0.15); color: var(--offline); border: 1px solid var(--offline); }
        
        .meta-row { display: flex; justify-content: space-between; font-size: 0.85rem; margin-bottom: 8px; color: var(--text-muted); }
        .meta-val { color: var(--text-main); font-weight: 500; }
        .device-id { font-family: monospace; font-size: 1rem; letter-spacing: 1px; color: #38bdf8; font-weight: 600; }
        
        .btn-connect { margin-top: 15px; width: 100%; padding: 10px; border: none; border-radius: 8px; background: var(--accent); color: white; font-weight: 600; cursor: pointer; transition: background 0.2s; }
        .btn-connect:hover:not(:disabled) { background: var(--accent-hover); }
        .btn-connect:disabled { background: var(--card-border); color: var(--text-muted); cursor: not-allowed; }
        
        .empty-state { text-align: center; grid-column: 1 / -1; padding: 60px 20px; color: var(--text-muted); }
    </style>
</head>
<body>
    <div class="container">
        <header>
            <div>
                <h1>Managed Remote Workstations</h1>
                <p class="subtitle">Live streaming inventory and remote agent presence</p>
            </div>
            <div id="sync-status" style="font-size: 0.8rem; color: var(--text-muted);">Syncing...</div>
        </header>

        <div id="device-grid" class="grid">
            <div class="empty-state">Loading registered workstations...</div>
        </div>
    </div>

    <script>
        function formatDeviceId(idStr) {
            if (idStr.length === 6) {
                return idStr.slice(0, 3) + '-' + idStr.slice(3);
            }
            return idStr;
        }

        async function fetchDevices() {
            try {
                const res = await fetch('../backend/api/devices/list.php');
                const data = await res.json();
                
                const grid = document.getElementById('device-grid');
                document.getElementById('sync-status').innerText = 'Last updated: ' + new Date().toLocaleTimeString();

                if (!data.devices || data.devices.length === 0) {
                    grid.innerHTML = '<div class="empty-state">No desktop agents registered. Run <code>cargo run --bin desktop-agent</code> to register a machine.</div>';
                    return;
                }

                grid.innerHTML = data.devices.map(device => `
                    <div class="card">
                        <div>
                            <div class="card-header">
                                <div class="device-name">${device.name}</div>
                                <span class="badge ${device.is_online == 1 ? 'online' : 'offline'}">
                                    ${device.is_online == 1 ? 'Online' : 'Offline'}
                                </span>
                            </div>
                            <div class="meta-row">
                                <span>Desktop ID:</span>
                                <span class="device-id">${formatDeviceId(device.device_uid)}</span>
                            </div>
                            <div class="meta-row">
                                <span>Operating System:</span>
                                <span class="meta-val">${device.os_type}</span>
                            </div>
                            <div class="meta-row">
                                <span>Local Network IP:</span>
                                <span class="meta-val">${device.ip_address || 'Unknown'}</span>
                            </div>
                            <div class="meta-row">
                                <span>Last Activity:</span>
                                <span class="meta-val">${device.last_seen_at || 'Never'}</span>
                            </div>
                        </div>
                        <button class="btn-connect" ${device.is_online == 1 ? '' : 'disabled'} 
                            onclick="initiateConnection('${device.device_uid}')">
                            ${device.is_online == 1 ? 'Connect to Session' : 'Host Offline'}
                        </button>
                    </div>
                `).join('');
            } catch (err) {
                document.getElementById('sync-status').innerText = 'Failed to poll backend';
            }
        }

        function initiateConnection(uid) {
            window.location.href = `remote/session.php?id=${uid}`;
        }

        // Fetch immediately and poll every 5 seconds
        fetchDevices();
        setInterval(fetchDevices, 5000);
    </script>
</body>
</html>