#!/usr/bin/env bash
# ============================================================================
# Saarthi / JCR1440 — Raspberry Pi First Boot Setup
# ============================================================================
# Run this ONCE on the Pi with internet access:
#   chmod +x first-boot.sh
#   sudo ./first-boot.sh
# ============================================================================

set -euo pipefail
export DEBIAN_FRONTEND=noninteractive

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

log()  { echo -e "${GREEN}[+]${NC} $*"; }
warn() { echo -e "${YELLOW}[!]${NC} $*"; }
err()  { echo -e "${RED}[x]${NC} $*"; exit 1; }
step() { echo -e "\n${CYAN}=== $* ===${NC}"; }

[ "$EUID" -eq 0 ] || err "Run with sudo: sudo $0"

RUN_USER="${SUDO_USER:-shambhu}"
RUN_HOME=$(eval echo "~${RUN_USER}")

echo ""
echo "  ╔══════════════════════════════════════════╗"
echo "  ║  Saarthi — Raspberry Pi First Boot       ║"
echo "  ╚══════════════════════════════════════════╝"
echo ""

# ============================================================================
step "1/7  SSH setup"
# ============================================================================

apt-get install -y --no-install-recommends openssh-server
sed -i 's/^#*PasswordAuthentication.*/PasswordAuthentication yes/' /etc/ssh/sshd_config
sed -i 's/^#*PubkeyAuthentication.*/PubkeyAuthentication yes/' /etc/ssh/sshd_config
systemctl enable --now ssh
log "SSH enabled (password + key auth)"

# ============================================================================
step "2/7  System dependencies"
# ============================================================================

apt-get update -qq

apt-get install -y --no-install-recommends \
    build-essential pkg-config curl git \
    libx11-dev libxcb1-dev libwayland-dev libxkbcommon-dev \
    libssl-dev libfontconfig1-dev \
    libgl1-mesa-dev libgles2-mesa-dev \
    libxrandr-dev libxi-dev libxcursor-dev \
    network-manager udev xdotool \
    htop lm-sensors cpufrequtils

log "System packages installed"

# ============================================================================
step "3/7  Rust toolchain"
# ============================================================================

if sudo -u "$RUN_USER" bash -c 'source ~/.cargo/env 2>/dev/null && rustc --version' &>/dev/null; then
    RUST_VER=$(sudo -u "$RUN_USER" bash -c 'source ~/.cargo/env && rustc --version')
    log "Rust already installed: $RUST_VER"
else
    log "Installing Rust for $RUN_USER..."
    sudo -u "$RUN_USER" bash -c 'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y'
    log "Rust installed"
fi

# ============================================================================
step "4/7  Thermal & power optimization"
# ============================================================================

# Boot config
BOOT_CONFIG=""
[ -f /boot/firmware/config.txt ] && BOOT_CONFIG="/boot/firmware/config.txt"
[ -f /boot/config.txt ] && BOOT_CONFIG="/boot/config.txt"

if [ -n "$BOOT_CONFIG" ]; then
    grep -q "^gpu_mem=" "$BOOT_CONFIG" || echo "gpu_mem=128" >> "$BOOT_CONFIG"
    grep -q "^hdmi_force_hotplug=" "$BOOT_CONFIG" || echo "hdmi_force_hotplug=1" >> "$BOOT_CONFIG"
    grep -q "^consoleblank=" "$BOOT_CONFIG" || echo "consoleblank=0" >> "$BOOT_CONFIG"
    log "Boot config updated (gpu_mem, hdmi, console)"
else
    warn "Boot config not found — skipping"
fi

# CPU governor: performance mode for consistent frame rates
if command -v cpufreq-set &>/dev/null; then
    for cpu in /sys/devices/system/cpu/cpu[0-9]*; do
        cpufreq-set -c "$(basename "$cpu" | tr -dc '0-9')" -g performance 2>/dev/null || true
    done
fi
[ -d /etc/default ] && echo "GOVERNOR=performance" > /etc/default/cpufrequtils
log "CPU governor set to performance"

# ============================================================================
step "5/7  Memory & swap optimization"
# ============================================================================

# zram swap — better than SD card swap for longevity and speed
apt-get install -y --no-install-recommends zram-tools 2>/dev/null || true
if [ -f /etc/default/zramswap ]; then
    cat > /etc/default/zramswap << 'EOF'
ALGO=zstd
PERCENT=25
PRIORITY=100
EOF
    log "zram swap configured (zstd, 25%)"
fi

# Reduce swappiness — prefer RAM over swap
echo "vm.swappiness=10" > /etc/sysctl.d/99-saarthi.conf
echo "vm.vfs_cache_pressure=50" >> /etc/sysctl.d/99-saarthi.conf
sysctl -p /etc/sysctl.d/99-saarthi.conf 2>/dev/null || true
log "Kernel memory tuning applied"

# ============================================================================
step "6/7  Disable unnecessary services"
# ============================================================================

DISABLE_SERVICES=(
    cups-browsed.service
    cups.service
    ModemManager.service
    bluetooth.service
    unattended-upgrades.service
    snapd.service
    snapd.socket
    snapd.apparmor.service
    whoopsie.service
    apport.service
    kerneloops.service
    gpu-manager.service
)

for svc in "${DISABLE_SERVICES[@]}"; do
    if systemctl is-enabled "$svc" &>/dev/null; then
        systemctl disable --now "$svc" 2>/dev/null && log "Disabled $svc"
    fi
done

# Disable screen blanking
if [ -d "${RUN_HOME}/.config" ]; then
    mkdir -p "${RUN_HOME}/.config/autostart"
    cat > "${RUN_HOME}/.config/autostart/disable-screensaver.desktop" << 'EOF'
[Desktop Entry]
Type=Application
Name=Disable Screensaver
Exec=sh -c "xset s off; xset -dpms; xset s noblank"
Hidden=false
NoDisplay=true
X-GNOME-Autostart-enabled=true
EOF
    chown -R "${RUN_USER}:${RUN_USER}" "${RUN_HOME}/.config/autostart"
    log "Screen blanking disabled"
fi

# ============================================================================
step "7/7  SSH key from desktop machine"
# ============================================================================

mkdir -p "${RUN_HOME}/.ssh"
chmod 700 "${RUN_HOME}/.ssh"
touch "${RUN_HOME}/.ssh/authorized_keys"
chmod 600 "${RUN_HOME}/.ssh/authorized_keys"
chown -R "${RUN_USER}:${RUN_USER}" "${RUN_HOME}/.ssh"

# Add the desktop machine's key if passed as argument
if [ -n "${1:-}" ] && [ -f "$1" ]; then
    cat "$1" >> "${RUN_HOME}/.ssh/authorized_keys"
    log "SSH key added from $1"
else
    echo ""
    echo "  To enable passwordless SSH from your desktop, run:"
    echo "    ssh-copy-id ${RUN_USER}@$(hostname -I | awk '{print $1}')"
    echo ""
fi

# ============================================================================
echo ""
echo "  ╔══════════════════════════════════════════════════════════╗"
echo "  ║  First boot setup complete!                              ║"
echo "  ║                                                          ║"
echo "  ║  What was done:                                          ║"
echo "  ║    ✓ SSH enabled (password + key auth)                   ║"
echo "  ║    ✓ Build deps installed (GL, X11, Wayland, SSL)        ║"
echo "  ║    ✓ Rust toolchain installed                            ║"
echo "  ║    ✓ GPU mem 128MB, HDMI forced, console unblanked       ║"
echo "  ║    ✓ CPU governor: performance                           ║"
echo "  ║    ✓ zram swap (zstd), swappiness=10                     ║"
echo "  ║    ✓ Disabled bloat (snap, cups, modem, bluetooth, etc)  ║"
echo "  ║                                                          ║"
echo "  ║  Next: clone JioMotive repo and run deploy/pi/setup.sh   ║"
echo "  ║  Or from desktop:  ssh ${RUN_USER}@$(hostname -I | awk '{print $1}')            ║"
echo "  ╚══════════════════════════════════════════════════════════╝"
echo ""
echo "  Reboot recommended:  sudo reboot"
echo ""
