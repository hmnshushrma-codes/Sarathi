#!/bin/bash
# update.sh — Pull latest code from git, rebuild, and update all services
#
# Usage:
#   sudo ./update.sh                # update everything
#   sudo ./update.sh --monitor-only # update only the monitor service
#   sudo ./update.sh --cluster-only # rebuild only the cluster binary
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
INSTALL_DIR="/opt/jcr1440-monitor"
CLUSTER_DIR="/opt/jcr1440"

echo "======================================"
echo "JioMotive — Update Script"
echo "======================================"
echo ""

if [ "$(id -u)" -ne 0 ]; then
    echo "ERROR: Must run as root (sudo ./update.sh)"
    exit 1
fi

MODE="${1:-all}"

# Step 1: Git pull
echo "[1] Pulling latest code..."
cd "$PROJECT_DIR"
sudo -u "$(stat -c '%U' "$PROJECT_DIR")" git pull --ff-only 2>&1 || {
    echo "  Git pull failed. You may need to resolve conflicts manually."
    echo "  Continuing with local code..."
}
echo ""

# Step 2: Update monitor
if [ "$MODE" = "all" ] || [ "$MODE" = "--monitor-only" ]; then
    echo "[2] Updating JCR1440 monitor..."
    mkdir -p "$INSTALL_DIR"
    cp "$SCRIPT_DIR/monitor.py" "$INSTALL_DIR/"
    cp "$SCRIPT_DIR/create-bundle.sh" "$INSTALL_DIR/"
    chmod +x "$INSTALL_DIR/monitor.py"
    chmod +x "$INSTALL_DIR/create-bundle.sh"

    # Update helper commands
    for cmd in jcr1440-status jcr1440-logs jcr1440-tail; do
        if [ -f "$SCRIPT_DIR/$cmd" ]; then
            cp "$SCRIPT_DIR/$cmd" "/usr/local/bin/$cmd"
            chmod +x "/usr/local/bin/$cmd"
        fi
    done

    # Update service file
    if [ -f "$SCRIPT_DIR/jcr1440-telemetry.service" ]; then
        cp "$SCRIPT_DIR/jcr1440-telemetry.service" "/etc/systemd/system/jcr1440-telemetry.service"
        systemctl daemon-reload
    fi

    echo "  Restarting jcr1440-telemetry..."
    systemctl restart jcr1440-telemetry 2>/dev/null || true
    echo "  Done."
    echo ""
fi

# Step 3: Rebuild cluster
if [ "$MODE" = "all" ] || [ "$MODE" = "--cluster-only" ]; then
    echo "[3] Rebuilding JCR1440 cluster..."

    # Source cargo env for the owning user
    OWNER=$(stat -c '%U' "$PROJECT_DIR")
    CARGO_HOME="/home/${OWNER}/.cargo"

    if [ -f "${CARGO_HOME}/env" ]; then
        export PATH="${CARGO_HOME}/bin:$PATH"
    fi

    if ! command -v cargo &>/dev/null; then
        echo "  ERROR: cargo not found. Install Rust first."
        echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    else
        cd "$PROJECT_DIR"
        sudo -u "$OWNER" bash -c "source ${CARGO_HOME}/env && cd $PROJECT_DIR && cargo build -p jcr1440-cluster --release 2>&1"

        if [ -f "$PROJECT_DIR/target/release/jcr1440-cluster" ]; then
            mkdir -p "$CLUSTER_DIR"
            cp "$PROJECT_DIR/target/release/jcr1440-cluster" "$CLUSTER_DIR/"
            echo "  Binary updated at $CLUSTER_DIR/jcr1440-cluster"

            echo "  Restarting jcr1440-cluster..."
            systemctl restart jcr1440-cluster 2>/dev/null || true
        else
            echo "  ERROR: Build failed, binary not found."
        fi
    fi
    echo ""
fi

echo "======================================"
echo "Update complete!"
echo "======================================"
echo ""
echo "Service status:"
systemctl status jcr1440-telemetry --no-pager 2>/dev/null || echo "  jcr1440-telemetry: not installed"
echo ""
systemctl status jcr1440-cluster --no-pager 2>/dev/null || echo "  jcr1440-cluster: not installed"
echo ""
