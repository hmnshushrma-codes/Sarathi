#!/usr/bin/env bash
# ============================================================================
# JCR1440 Cluster — Raspberry Pi Setup Script
# ============================================================================
#
# Run this ONCE on a fresh Raspberry Pi (at home, with internet).
# After this completes, the Pi is ready — just plug the JCR1440 USB
# cable in and the cluster auto-launches fullscreen.
#
# Usage:
#   chmod +x setup.sh
#   sudo ./setup.sh
#
# What this does:
#   1. Installs system dependencies (GL, X11, Wayland, SSL, fonts)
#   2. Installs Rust toolchain (if not present)
#   3. Builds the cluster binary (release mode)
#   4. Installs udev rule: auto-configures RNDIS network when JCR1440 plugs in
#   5. Installs systemd services: auto-launches cluster on USB connect
#   6. Configures GPU and display settings
#   7. Disables screen blanking
#   8. Runs a bench test with mock data to verify everything works
#
# After setup, run:  sudo ./bench-test.sh    (to verify)
# Then take the Pi to the car and plug in.
# ============================================================================

set -euo pipefail

INSTALL_DIR="/opt/jcr1440"
REPO_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
RUN_USER="${SUDO_USER:-pi}"
RUN_HOME=$(eval echo "~${RUN_USER}")

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

log()  { echo -e "${GREEN}[+]${NC} $*"; }
warn() { echo -e "${YELLOW}[!]${NC} $*"; }
err()  { echo -e "${RED}[x]${NC} $*"; exit 1; }
step() { echo -e "\n${CYAN}=== $* ===${NC}"; }

# Must be root
if [ "$EUID" -ne 0 ]; then
    err "Run with sudo: sudo $0"
fi

echo ""
echo "  ╔══════════════════════════════════════════╗"
echo "  ║  JCR1440 Cluster — Pi Setup              ║"
echo "  ║  Nino's Tech · oyenino.com               ║"
echo "  ╚══════════════════════════════════════════╝"
echo ""

# ============================================================================
step "1/8  Installing system dependencies"
# ============================================================================

apt-get update -qq

apt-get install -y --no-install-recommends \
    build-essential pkg-config curl git \
    libx11-dev libxcb1-dev libwayland-dev libxkbcommon-dev \
    libssl-dev libfontconfig1-dev \
    libgl1-mesa-dev libgles2-mesa-dev \
    libxrandr-dev libxi-dev libxcursor-dev \
    network-manager \
    udev \
    xdotool

log "System packages installed"

# ============================================================================
step "2/8  Installing Rust toolchain"
# ============================================================================

if sudo -u "$RUN_USER" bash -c 'source ~/.cargo/env 2>/dev/null && rustc --version' &>/dev/null; then
    RUST_VER=$(sudo -u "$RUN_USER" bash -c 'source ~/.cargo/env && rustc --version')
    log "Rust already installed: $RUST_VER"
else
    log "Installing Rust for user $RUN_USER..."
    sudo -u "$RUN_USER" bash -c 'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y'
    log "Rust installed"
fi

# ============================================================================
step "3/8  Building cluster binary (release mode)"
# ============================================================================

log "Building from $REPO_DIR (this takes 5-10 min on Pi 4, ~3 min on Pi 5)..."
sudo -u "$RUN_USER" bash -c "
    source ~/.cargo/env
    cd '$REPO_DIR'
    cargo build --release -p jcr1440-cluster 2>&1
"

log "Build complete"

# ============================================================================
step "4/8  Installing binary and config"
# ============================================================================

mkdir -p "$INSTALL_DIR"
cp "$REPO_DIR/target/release/jcr1440-cluster" "$INSTALL_DIR/"
chmod +x "$INSTALL_DIR/jcr1440-cluster"
log "Binary installed to $INSTALL_DIR/jcr1440-cluster"

# Also copy the Python poller as a diagnostic tool
cp "$REPO_DIR/scripts/jcr1440-poller.py" "$INSTALL_DIR/" 2>/dev/null || true
cp "$REPO_DIR/scripts/jcr1440-wifi-config.py" "$INSTALL_DIR/" 2>/dev/null || true
log "Diagnostic scripts copied"

# ============================================================================
step "5/8  Installing udev rule (auto-configure RNDIS on USB plug)"
# ============================================================================

cat > /etc/udev/rules.d/90-jcr1440.rules << 'UDEV_EOF'
# JCR1440 OBD-II Dongle — Qualcomm RNDIS mode
# When the device is plugged in via USB:
#   1. Tag the RNDIS network interface
#   2. Trigger the jcr1440-net service to configure IP
#   3. Trigger the cluster display service

# Match the JCR1440 USB device (VID:PID 05c6:f00e)
ACTION=="add", SUBSYSTEM=="usb", ATTR{idVendor}=="05c6", ATTR{idProduct}=="f00e", \
    TAG+="systemd", ENV{SYSTEMD_WANTS}="jcr1440-net.service"

# Match the RNDIS network interface created by the device
ACTION=="add", SUBSYSTEM=="net", DRIVERS=="rndis_host", \
    TAG+="systemd", ENV{SYSTEMD_WANTS}="jcr1440-net.service"
UDEV_EOF

udevadm control --reload-rules
log "udev rule installed"

# ============================================================================
step "6/8  Installing systemd services"
# ============================================================================

# Service 1: Network configuration for the RNDIS interface
cat > /etc/systemd/system/jcr1440-net.service << 'SVC1_EOF'
[Unit]
Description=JCR1440 RNDIS Network Setup
After=network.target
Wants=jcr1440-cluster.service

[Service]
Type=oneshot
RemainAfterExit=yes

# Find the RNDIS interface, bring it up, configure IP, add a host route
# so 192.168.1.1 always goes to the JCR1440 (not a home router)
ExecStart=/bin/bash -c '\
    IFACE=""; \
    for d in /sys/class/net/enx*; do \
        [ -d "$d/device" ] || continue; \
        drv=$(readlink "$d/device/driver" 2>/dev/null | xargs basename 2>/dev/null); \
        [ "$drv" = "rndis_host" ] && IFACE=$(basename "$d") && break; \
    done; \
    if [ -z "$IFACE" ]; then echo "No RNDIS interface found"; exit 1; fi; \
    echo "Configuring $IFACE for JCR1440..."; \
    ip link set "$IFACE" up; \
    ip addr add 192.168.1.100/24 dev "$IFACE" 2>/dev/null || true; \
    ip route replace 192.168.1.1/32 dev "$IFACE" metric 50; \
    echo "JCR1440 network ready on $IFACE"'

ExecStop=/bin/bash -c '\
    IFACE=""; \
    for d in /sys/class/net/enx*; do \
        [ -d "$d/device" ] || continue; \
        drv=$(readlink "$d/device/driver" 2>/dev/null | xargs basename 2>/dev/null); \
        [ "$drv" = "rndis_host" ] && IFACE=$(basename "$d") && break; \
    done; \
    [ -n "$IFACE" ] && ip route del 192.168.1.1/32 dev "$IFACE" 2>/dev/null || true'

[Install]
WantedBy=multi-user.target
SVC1_EOF

# Service 2: The cluster display
cat > /etc/systemd/system/jcr1440-cluster.service << SVC2_EOF
[Unit]
Description=JCR1440 Racing Instrument Cluster — Nino's Tech
After=jcr1440-net.service graphical.target
Wants=jcr1440-net.service
# Only start when the network service is running (device plugged in)
BindsTo=jcr1440-net.service

[Service]
Type=simple
User=${RUN_USER}
Environment=DISPLAY=:0
Environment=WAYLAND_DISPLAY=wayland-0
Environment=XDG_RUNTIME_DIR=/run/user/$(id -u ${RUN_USER})

ExecStart=${INSTALL_DIR}/jcr1440-cluster --fullscreen --ip 192.168.1.1

Restart=on-failure
RestartSec=3

# Stop gracefully when USB is unplugged
TimeoutStopSec=5

[Install]
WantedBy=graphical.target
SVC2_EOF

systemctl daemon-reload
systemctl enable jcr1440-net.service
systemctl enable jcr1440-cluster.service
log "Services installed and enabled"

# ============================================================================
step "7/8  Configuring GPU and display"
# ============================================================================

BOOT_CONFIG=""
if [ -f /boot/firmware/config.txt ]; then
    BOOT_CONFIG="/boot/firmware/config.txt"
elif [ -f /boot/config.txt ]; then
    BOOT_CONFIG="/boot/config.txt"
fi

if [ -n "$BOOT_CONFIG" ]; then
    # Ensure GPU memory is sufficient
    if ! grep -q "^gpu_mem=" "$BOOT_CONFIG"; then
        echo "gpu_mem=128" >> "$BOOT_CONFIG"
        log "Set gpu_mem=128"
    fi

    # Ensure KMS driver is enabled (hardware OpenGL)
    if ! grep -q "vc4-kms-v3d" "$BOOT_CONFIG"; then
        echo "dtoverlay=vc4-kms-v3d" >> "$BOOT_CONFIG"
        log "Enabled vc4-kms-v3d overlay"
    fi

    # Force HDMI output
    if ! grep -q "^hdmi_force_hotplug=" "$BOOT_CONFIG"; then
        echo "hdmi_force_hotplug=1" >> "$BOOT_CONFIG"
        log "Set hdmi_force_hotplug=1"
    fi

    # Disable console blanking
    if ! grep -q "^consoleblank=" "$BOOT_CONFIG"; then
        echo "consoleblank=0" >> "$BOOT_CONFIG"
        log "Disabled console blanking"
    fi
else
    warn "Could not find boot config — GPU settings not applied"
fi

# Disable screen saver in X11/LXDE
AUTOSTART_DIR="${RUN_HOME}/.config/lxsession/LXDE-pi"
if [ -d "$AUTOSTART_DIR" ] || mkdir -p "$AUTOSTART_DIR" 2>/dev/null; then
    AUTOSTART="${AUTOSTART_DIR}/autostart"
    grep -q "xset s off" "$AUTOSTART" 2>/dev/null || echo "@xset s off" >> "$AUTOSTART"
    grep -q "xset -dpms" "$AUTOSTART" 2>/dev/null || echo "@xset -dpms" >> "$AUTOSTART"
    grep -q "xset s noblank" "$AUTOSTART" 2>/dev/null || echo "@xset s noblank" >> "$AUTOSTART"
    chown -R "${RUN_USER}:${RUN_USER}" "${RUN_HOME}/.config"
    log "Screen saver disabled"
fi

log "GPU and display configured"

# ============================================================================
step "8/8  Setup complete!"
# ============================================================================

echo ""
echo "  ╔══════════════════════════════════════════════════════════╗"
echo "  ║  Setup complete!                                         ║"
echo "  ║                                                          ║"
echo "  ║  Next steps:                                             ║"
echo "  ║    1. Run bench test:  sudo ./bench-test.sh              ║"
echo "  ║    2. Reboot:          sudo reboot                       ║"
echo "  ║    3. Plug JCR1440 into car OBD-II + USB to Pi           ║"
echo "  ║    4. Cluster auto-launches fullscreen                   ║"
echo "  ║                                                          ║"
echo "  ║  Manual launch:                                          ║"
echo "  ║    Mock:  ${INSTALL_DIR}/jcr1440-cluster --mock          ║"
echo "  ║    Live:  ${INSTALL_DIR}/jcr1440-cluster --fullscreen    ║"
echo "  ║                                                          ║"
echo "  ║  Nino's Tech · oyenino.com                               ║"
echo "  ╚══════════════════════════════════════════════════════════╝"
echo ""
