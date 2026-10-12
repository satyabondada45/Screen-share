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
