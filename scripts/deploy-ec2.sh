#!/usr/bin/env bash
set -e

echo "=== Step 1: Check new binary hash ==="
sha256sum /home/ubuntu/relay-server.new

echo "=== Step 2: Backup old binary ==="
BACKUP="/opt/deskstream/relay-server/relay-server.bak.$(date +%Y%m%d_%H%M%S)"
sudo cp -pv /opt/deskstream/relay-server/relay-server "$BACKUP"

echo "=== Step 3: Install new binary ==="
sudo install -o deskstream -g deskstream -m 0755 /home/ubuntu/relay-server.new /opt/deskstream/relay-server/relay-server
rm -f /home/ubuntu/relay-server.new

echo "=== Step 4: Restart relay-server.service ==="
sudo systemctl restart relay-server.service

echo "=== Step 5: Verify status ==="
sleep 2
sudo systemctl is-active relay-server.service
sudo systemctl status relay-server.service --no-pager

echo "=== Step 6: Verify running process and SHA256 ==="
PID=$(systemctl show -p MainPID --value relay-server.service)
echo "Main PID: $PID"
sudo sha256sum /proc/$PID/exe
sudo sha256sum /opt/deskstream/relay-server/relay-server

echo "=== Step 7: Verify listening socket ==="
sudo ss -tulpn | grep 9001
echo "=== Deployment Successfully Completed ==="
