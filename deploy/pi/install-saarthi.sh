#!/bin/bash
# install-saarthi.sh — Install Saarthi launcher, desktop entry, and optional auto-start
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$(dirname "$SCRIPT_DIR")")"

echo "======================================"
echo "Saarthi Cluster — Installer"
echo "======================================"
echo ""

if [ "$(id -u)" -ne 0 ]; then
    echo "ERROR: Must run as root (sudo ./install-saarthi.sh)"
    exit 1
fi

# 1. Install launcher script
echo "[1/5] Installing launcher..."
mkdir -p /opt/saarthi/bin
cp "$SCRIPT_DIR/start-saarthi.sh" /opt/saarthi/bin/start-saarthi
chmod +x /opt/saarthi/bin/start-saarthi
mkdir -p /var/log/saarthi
echo "  /opt/saarthi/bin/start-saarthi"

# 2. Install cluster binary (if built)
echo "[2/5] Installing cluster binary..."
if [ -f "$PROJECT_DIR/target/release/jcr1440-cluster" ]; then
    mkdir -p /opt/jcr1440
    cp "$PROJECT_DIR/target/release/jcr1440-cluster" /opt/jcr1440/
    echo "  /opt/jcr1440/jcr1440-cluster"
elif [ -f /opt/jcr1440/jcr1440-cluster ]; then
    echo "  Using existing binary at /opt/jcr1440/jcr1440-cluster"
else
    echo "  WARNING: No cluster binary found. Build first:"
    echo "    cargo build -p jcr1440-cluster --release"
fi

# 3. Install desktop entry
echo "[3/5] Installing desktop entry..."
OWNER=$(stat -c '%U' "$PROJECT_DIR" 2>/dev/null || echo "shambhu")
HOME_DIR=$(eval echo "~$OWNER")
mkdir -p "$HOME_DIR/.local/share/applications"
cp "$SCRIPT_DIR/saarthi-cluster.desktop" "$HOME_DIR/.local/share/applications/"
chown "$OWNER:$OWNER" "$HOME_DIR/.local/share/applications/saarthi-cluster.desktop"

# Also put on desktop
if [ -d "$HOME_DIR/Desktop" ]; then
    cp "$SCRIPT_DIR/saarthi-cluster.desktop" "$HOME_DIR/Desktop/"
    chown "$OWNER:$OWNER" "$HOME_DIR/Desktop/saarthi-cluster.desktop"
    chmod +x "$HOME_DIR/Desktop/saarthi-cluster.desktop"
    echo "  Desktop shortcut created"
fi
echo "  App menu entry created"

# 4. Install systemd service (disabled by default)
echo "[4/5] Installing auto-start service (disabled)..."
cp "$SCRIPT_DIR/saarthi-ui.service" /etc/systemd/system/
systemctl daemon-reload
echo "  To enable auto-start: sudo systemctl enable saarthi-ui"

# 5. Verify
echo "[5/5] Verifying installation..."
echo ""
if [ -x /opt/saarthi/bin/start-saarthi ]; then
    echo "  Launcher:  OK"
else
    echo "  Launcher:  MISSING"
fi
if [ -x /opt/jcr1440/jcr1440-cluster ]; then
    echo "  Binary:    OK"
else
    echo "  Binary:    MISSING"
fi
echo ""

echo "======================================"
echo "Installation complete!"
echo "======================================"
echo ""
echo "To launch manually:"
echo "  /opt/saarthi/bin/start-saarthi"
echo ""
echo "To enable auto-start on boot:"
echo "  sudo systemctl enable saarthi-ui"
echo ""
echo "To disable auto-start:"
echo "  sudo systemctl disable saarthi-ui"
echo ""
