# DeskStream Relay Server Deployment

This directory contains the necessary files to deploy the DeskStream Relay Server to an AWS EC2 Linux instance (e.g., Ubuntu).

## Prerequisites

1. An AWS EC2 instance running Ubuntu 22.04 LTS or newer.
2. A compiled Linux binary of the `relay-server`. (Compile it on a Linux machine or use a cross-compiler).

## Installation Instructions

1. **Create a dedicated user (Recommended for security):**
   ```bash
   sudo useradd -r -s /bin/false deskstream
   ```

2. **Prepare the installation directory:**
   ```bash
   sudo mkdir -p /opt/deskstream/relay-server
   sudo chown -R deskstream:deskstream /opt/deskstream
   ```

3. **Copy the compiled binary:**
   Upload the Linux-compiled `relay-server` binary to `/opt/deskstream/relay-server/` and ensure it is executable.
   ```bash
   sudo chmod +x /opt/deskstream/relay-server/relay-server
   ```

4. **Install the Systemd Service:**
   Copy the `relay-server.service` file to the systemd directory:
   ```bash
   sudo cp deploy/relay-server.service /etc/systemd/system/relay-server.service
   ```

5. **Configure Environment Variables:**
   Edit `/etc/systemd/system/relay-server.service` and verify the `Environment=` values.
   - Set `SCREENSHARE_BACKEND_URL=https://friendssoftwaresolutions.in/DeskStream/api`.
   - Leave `SCREENSHARE_BACKEND_HOST` and `SCREENSHARE_BACKEND_PORT` unset when using this HTTPS URL.
   - Leave `RELAY_PORT=9001` unless you require a different port.

6. **Reload Systemd and Start the Service:**
   ```bash
   sudo systemctl daemon-reload
   sudo systemctl enable relay-server.service
   sudo systemctl start relay-server.service
   ```

7. **Verify it is running:**
   ```bash
   sudo systemctl status relay-server
   journalctl -u relay-server -f
   ```

## Notes
- Ensure your AWS Security Group allows inbound TCP traffic on the port specified in `RELAY_PORT` (default: 9001).
- The relay server leverages `ctrlc` for graceful shutdown handling when receiving SIGTERM/SIGINT.
- Configure `SCREENSHARE_BACKEND_URL=https://friendssoftwaresolutions.in/DeskStream/api` in the systemd unit. The relay calls `/devices/heartbeat.php` with an HTTP `POST` and JSON `system_id`; the checked-in endpoint returns JSON containing `status` and `updated` and requires no authentication header.
- The relay uses HTTPS with certificate verification. Leave `SCREENSHARE_BACKEND_HOST` and `SCREENSHARE_BACKEND_PORT` unset for Hostinger; those optional overrides must match the endpoint and its TLS certificate.

## Browser WebSocket TLS proxy

The browser session page uses `wss://admin.friendssoftwaresolutions.in` (TLS port 443), while the relay listens for its native and WebSocket traffic on TCP port 9001. Terminate TLS at Nginx and proxy WebSocket upgrades to the local relay; keep TCP 9001 available for desktop agents.

Set `RELAY_WS_URL=wss://admin.friendssoftwaresolutions.in` in the PHP application's environment. The checked-in project `.env` is read by `backend/config/database.php` before `frontend/remote/session.php` builds its WebSocket URL; do not leave it set to `ws://127.0.0.1:9001` for remote sessions.

1. Install Nginx and obtain a valid certificate for `admin.friendssoftwaresolutions.in` (for example, using Certbot). Ensure the certificate files exist at the paths in `nginx/relay-server.conf`.
2. Install the Nginx configuration:
   ```bash
   sudo cp deploy/nginx/relay-server.conf /etc/nginx/conf.d/deskstream-relay.conf
   sudo nginx -t
   sudo systemctl reload nginx
   ```
3. Allow inbound TCP 443 in the AWS security group and host firewall. Keep the existing TCP 9001 rule for desktop-agent connections; do not expose other ports.
4. From a remote client, verify `https://admin.friendssoftwaresolutions.in/health` responds through the proxy and that TCP 9001 remains reachable. A WebSocket upgrade should be verified by opening the DeskStream session and confirming `[RELAY][WS] event=VIEWER_WEBSOCKET_ACCEPTED` followed by the handshake and target-lookup events in `journalctl -u relay-server`.

The relay classifies an HTTP `GET` request for `/health` as a health probe. Other HTTP `GET` requests are passed to its WebSocket handshake handler; actual session setup still requires a valid binary handshake and host PIN authentication.

## Production rollout

The checked-in systemd unit already uses the Hostinger API base and relay port 9001. If the EC2 copy still contains the old localhost value, update only that environment assignment so other settings remain intact:

```bash
sudo cp -a /etc/systemd/system/relay-server.service \
  "/etc/systemd/system/relay-server.service.bak.$(date +%Y%m%d-%H%M%S)"
sudo sed -i '/^Environment="SCREENSHARE_BACKEND_URL=/c\Environment="SCREENSHARE_BACKEND_URL=https://friendssoftwaresolutions.in/DeskStream/api"' \
  /etc/systemd/system/relay-server.service
grep '^Environment="SCREENSHARE_BACKEND_URL=' /etc/systemd/system/relay-server.service
```

Deploy the updated Rust source and build on EC2 (or another Linux host with the same target), preserving a copy of the existing executable:

```bash
cd /opt/deskstream/relay-server
cargo test --bin relay-server
cargo build --release --bin relay-server
sudo cp -a relay-server "relay-server.bak.$(date +%Y%m%d-%H%M%S)"
sudo install -m 0755 target/release/relay-server relay-server.new
sudo mv relay-server.new relay-server
```

After installing the Nginx TLS proxy configuration and confirming its certificate, apply and verify the service:

```bash
sudo systemctl daemon-reload
sudo nginx -t
sudo systemctl reload nginx
sudo systemctl restart relay-server.service
sudo systemctl status relay-server.service --no-pager
sudo journalctl -u relay-server.service --since "5 minutes ago" --no-pager
sudo ss -ltnp | grep -E ':(443|9001)\b'
getent ahosts friendssoftwaresolutions.in
curl --silent --show-error --include --max-time 10 \
  --request POST --header 'Content-Type: application/json' --data '{}' \
  https://friendssoftwaresolutions.in/DeskStream/api/devices/heartbeat.php
```

The credential-free heartbeat probe is expected to return HTTP 400 with a missing-identifier response; that confirms the HTTPS endpoint answered without changing a device row. For real heartbeat verification, inspect subsequent relay logs after agents reconnect; never put a real device ID, token, PIN, or credential in a diagnostic command. Update only the `RELAY_WS_URL` line in the Hostinger `.env` to `wss://admin.friendssoftwaresolutions.in`; do not upload this workstation's complete `.env` file.
