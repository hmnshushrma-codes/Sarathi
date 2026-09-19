#!/bin/bash
# install.sh — Install JCR1440 Telemetry Monitor
set -euo pipefail

INSTALL_DIR="/opt/jcr1440-monitor"
LOG_DIR="/var/log/jcr1440"
SERVICE_NAME="jcr1440-telemetry"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

echo "======================================"
echo "JCR1440 Telemetry Monitor — Installer"
echo "======================================"
echo ""

# Check root
if [ "$(id -u)" -ne 0 ]; then
    echo "ERROR: Must run as root (sudo ./install.sh)"
    exit 1
fi

# 1. Install safe dependencies
echo "[1/7] Installing dependencies..."
apt-get update -qq
apt-get install -y -qq usbutils udev iproute2 ethtool modemmanager >/dev/null 2>&1 || true
echo "  Done."

# 2. Create install directory
echo "[2/7] Creating ${INSTALL_DIR}..."
mkdir -p "$INSTALL_DIR"
cp "$SCRIPT_DIR/monitor.py" "$INSTALL_DIR/"
cp "$SCRIPT_DIR/create-bundle.sh" "$INSTALL_DIR/"
chmod +x "$INSTALL_DIR/monitor.py"
chmod +x "$INSTALL_DIR/create-bundle.sh"
echo "  Done."

# 3. Create log directory
echo "[3/7] Creating ${LOG_DIR}..."
mkdir -p "$LOG_DIR"
echo "  Done."

# 4. Install helper commands
echo "[4/7] Installing helper commands..."
for cmd in jcr1440-status jcr1440-logs jcr1440-tail; do
    cp "$SCRIPT_DIR/$cmd" "/usr/local/bin/$cmd"
    chmod +x "/usr/local/bin/$cmd"
done
echo "  Installed: jcr1440-status, jcr1440-logs, jcr1440-tail"

# 5. Install systemd service
echo "[5/7] Installing systemd service..."
cp "$SCRIPT_DIR/jcr1440-telemetry.service" "/etc/systemd/system/${SERVICE_NAME}.service"
systemctl daemon-reload
echo "  Done."

# 6. Enable service
echo "[6/7] Enabling ${SERVICE_NAME}..."
systemctl enable "${SERVICE_NAME}.service"
echo "  Done."

# 7. Start service
echo "[7/7] Starting ${SERVICE_NAME}..."
systemctl start "${SERVICE_NAME}.service"
sleep 2
echo "  Done."

echo ""
echo "======================================"
echo "Installation complete!"
echo "======================================"
echo ""
systemctl status "${SERVICE_NAME}.service" --no-pager || true
echo ""
echo "Commands:"
echo "  jcr1440-status  — Current device status"
echo "  jcr1440-logs    — Latest log session path"
echo "  jcr1440-tail    — Follow live summary"
echo ""
echo "Logs: ${LOG_DIR}/"
echo "Service: systemctl status ${SERVICE_NAME}"
echo ""
