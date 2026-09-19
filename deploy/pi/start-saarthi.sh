#!/bin/bash
# start-saarthi — Launch Saarthi vehicle cluster with preflight
set -euo pipefail

CLUSTER_BIN="/opt/jcr1440/jcr1440-cluster"
LOG_DIR="/var/log/saarthi"
SESSION_LOG="$LOG_DIR/launch-$(date +%Y%m%d_%H%M%S).log"

mkdir -p "$LOG_DIR"

echo "$(date) — Saarthi starting" | tee "$SESSION_LOG"

# Verify binary exists
if [ ! -x "$CLUSTER_BIN" ]; then
    echo "ERROR: Cluster binary not found at $CLUSTER_BIN" | tee -a "$SESSION_LOG"
    exit 1
fi

# Check if telemetry monitor is running (optional)
if systemctl is-active --quiet jcr1440-telemetry 2>/dev/null; then
    echo "Telemetry monitor: running" | tee -a "$SESSION_LOG"
else
    echo "Telemetry monitor: not running (starting...)" | tee -a "$SESSION_LOG"
    sudo systemctl start jcr1440-telemetry 2>/dev/null || true
fi

# Ensure DISPLAY is set for X11/Wayland
export DISPLAY="${DISPLAY:-:0}"
export WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-wayland-0}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"

echo "Display: DISPLAY=$DISPLAY WAYLAND=$WAYLAND_DISPLAY" | tee -a "$SESSION_LOG"

# Launch cluster with preflight
echo "$(date) — Launching cluster" | tee -a "$SESSION_LOG"
exec "$CLUSTER_BIN" --fullscreen --ip 192.168.1.1 2>&1 | tee -a "$SESSION_LOG"
