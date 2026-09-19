#!/bin/bash
# setup.sh — One-command setup for NinoDash on Raspberry Pi
#
# Usage: cd ~/Desktop/Sarathi && sudo ./setup.sh
#
# This script:
#   1. Installs system dependencies
#   2. Installs Rust (if not present)
#   3. Builds the cluster binary
#   4. Installs the binary + logo
#   5. Installs the telemetry monitor service
#   6. Installs the cluster launcher + desktop shortcut
#   7. Prints status
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
OWNER=$(stat -c '%U' "$SCRIPT_DIR")

echo "======================================"
echo "NinoDash — Full Setup"
echo "======================================"
echo ""

if [ "$(id -u)" -ne 0 ]; then
    echo "ERROR: Run as root: sudo ./setup.sh"
    exit 1
fi

# 1. System dependencies
echo "[1/7] Installing system dependencies..."
apt-get update -qq
apt-get install -y -qq \
    build-essential pkg-config \
    libssl-dev libgl-dev libegl-dev \
    libx11-dev libxcursor-dev libxrandr-dev libxi-dev \
    libwayland-dev libxkbcommon-dev \
    usbutils udev iproute2 ethtool modemmanager \
    fonts-liberation >/dev/null 2>&1 || true
echo "  Done."

# 2. Rust toolchain
echo "[2/7] Checking Rust..."
CARGO_HOME="/home/${OWNER}/.cargo"
if [ -f "${CARGO_HOME}/bin/cargo" ]; then
    echo "  Rust already installed."
else
    echo "  Installing Rust..."
    sudo -u "$OWNER" bash -c 'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y' 2>&1 | tail -3
fi

# 3. Build
echo "[3/7] Building NinoDash cluster (this may take a few minutes)..."
sudo -u "$OWNER" bash -c "source ${CARGO_HOME}/env && cd ${SCRIPT_DIR} && cargo build -p jcr1440-cluster --release 2>&1" | tail -3
echo "  Done."

# 4. Install binary + logo
echo "[4/7] Installing cluster binary..."
mkdir -p /opt/jcr1440/assets
cp "${SCRIPT_DIR}/target/release/jcr1440-cluster" /opt/jcr1440/
if [ -f "${SCRIPT_DIR}/assets/nino_dash.png" ]; then
    cp "${SCRIPT_DIR}/assets/nino_dash.png" /opt/jcr1440/assets/
    echo "  Logo installed."
fi
echo "  Binary: /opt/jcr1440/jcr1440-cluster"

# 4b. Install udev rule for USB mode switching
if [ -f "${SCRIPT_DIR}/deploy/pi/99-jcr1440-modeswitch.rules" ]; then
    cp "${SCRIPT_DIR}/deploy/pi/99-jcr1440-modeswitch.rules" /etc/udev/rules.d/
    udevadm control --reload-rules 2>/dev/null || true
    echo "  udev rule installed (auto RNDIS mode switch)."
fi

# 5. Install telemetry monitor
echo "[5/7] Installing telemetry monitor..."
mkdir -p /opt/jcr1440-monitor /var/log/jcr1440
cp "${SCRIPT_DIR}/monitor/monitor.py" /opt/jcr1440-monitor/
cp "${SCRIPT_DIR}/monitor/create-bundle.sh" /opt/jcr1440-monitor/
chmod +x /opt/jcr1440-monitor/monitor.py /opt/jcr1440-monitor/create-bundle.sh
for cmd in jcr1440-status jcr1440-logs jcr1440-tail ninodash-trip-status; do
    cp "${SCRIPT_DIR}/monitor/$cmd" "/usr/local/bin/$cmd"
    chmod +x "/usr/local/bin/$cmd"
done
cp "${SCRIPT_DIR}/monitor/jcr1440-telemetry.service" /etc/systemd/system/
systemctl daemon-reload
systemctl enable jcr1440-telemetry.service 2>/dev/null || true
echo "  Done."

# 6. Install launcher + desktop shortcut
echo "[6/7] Installing launcher..."
mkdir -p /opt/saarthi/bin /var/log/saarthi
cp "${SCRIPT_DIR}/deploy/pi/start-saarthi.sh" /opt/saarthi/bin/start-saarthi
chmod +x /opt/saarthi/bin/start-saarthi
HOME_DIR=$(eval echo "~${OWNER}")
mkdir -p "${HOME_DIR}/.local/share/applications"
cp "${SCRIPT_DIR}/deploy/pi/saarthi-cluster.desktop" "${HOME_DIR}/.local/share/applications/"
chown "${OWNER}:${OWNER}" "${HOME_DIR}/.local/share/applications/saarthi-cluster.desktop"
if [ -d "${HOME_DIR}/Desktop" ]; then
    cp "${SCRIPT_DIR}/deploy/pi/saarthi-cluster.desktop" "${HOME_DIR}/Desktop/"
    chown "${OWNER}:${OWNER}" "${HOME_DIR}/Desktop/saarthi-cluster.desktop"
    chmod +x "${HOME_DIR}/Desktop/saarthi-cluster.desktop"
fi
cp "${SCRIPT_DIR}/deploy/pi/saarthi-ui.service" /etc/systemd/system/
systemctl daemon-reload
echo "  Done."

# 7. Status
echo "[7/7] Verifying..."
echo ""
echo "======================================"
echo "NinoDash Setup Complete!"
echo "======================================"
echo ""
echo "Installed:"
[ -x /opt/jcr1440/jcr1440-cluster ] && echo "  [OK] Cluster binary" || echo "  [!!] Cluster binary MISSING"
[ -f /opt/jcr1440/assets/nino_dash.png ] && echo "  [OK] Logo" || echo "  [!!] Logo MISSING"
[ -f /opt/jcr1440-monitor/monitor.py ] && echo "  [OK] Telemetry monitor" || echo "  [!!] Monitor MISSING"
[ -x /opt/saarthi/bin/start-saarthi ] && echo "  [OK] Launcher" || echo "  [!!] Launcher MISSING"
echo ""
echo "Commands:"
echo "  /opt/saarthi/bin/start-saarthi    — Launch NinoDash"
echo "  jcr1440-status                    — Device status"
echo "  jcr1440-logs                      — Latest log session"
echo ""
echo "Auto-start on boot:"
echo "  sudo systemctl enable saarthi-ui"
echo ""
echo "Update from git:"
echo "  cd ${SCRIPT_DIR} && git pull && sudo ./setup.sh"
echo ""
