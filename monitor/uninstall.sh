#!/bin/bash
# uninstall.sh — Remove JCR1440 Telemetry Monitor
set -euo pipefail

SERVICE_NAME="jcr1440-telemetry"
INSTALL_DIR="/opt/jcr1440-monitor"

echo "JCR1440 Telemetry Monitor — Uninstaller"
echo ""

if [ "$(id -u)" -ne 0 ]; then
    echo "ERROR: Must run as root (sudo ./uninstall.sh)"
    exit 1
fi

echo "Stopping service..."
systemctl stop "${SERVICE_NAME}.service" 2>/dev/null || true
systemctl disable "${SERVICE_NAME}.service" 2>/dev/null || true

echo "Removing service file..."
rm -f "/etc/systemd/system/${SERVICE_NAME}.service"
systemctl daemon-reload

echo "Removing helper commands..."
rm -f /usr/local/bin/jcr1440-status
rm -f /usr/local/bin/jcr1440-logs
rm -f /usr/local/bin/jcr1440-tail

echo "Removing install directory..."
rm -rf "$INSTALL_DIR"

echo ""
echo "Uninstalled. Log data preserved at /var/log/jcr1440/"
echo "To remove logs: sudo rm -rf /var/log/jcr1440/"
