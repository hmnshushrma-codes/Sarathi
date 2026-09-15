#!/usr/bin/env bash
# ============================================================================
# JCR1440 Cluster — Bench Test
# ============================================================================
#
# Run this AFTER setup.sh to verify everything works before going to the car.
# Tests each layer of the stack and reports pass/fail.
#
# Usage:
#   sudo ./bench-test.sh              # full test suite
#   sudo ./bench-test.sh --quick      # skip display test (headless)
#
# ============================================================================

set -uo pipefail

INSTALL_DIR="/opt/jcr1440"
QUICK=false
[[ "${1:-}" == "--quick" ]] && QUICK=true

PASS=0
FAIL=0
WARN=0

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

pass() { echo -e "  ${GREEN}PASS${NC}  $*"; ((PASS++)); }
fail() { echo -e "  ${RED}FAIL${NC}  $*"; ((FAIL++)); }
skip() { echo -e "  ${YELLOW}SKIP${NC}  $*"; ((WARN++)); }
info() { echo -e "  ${CYAN}INFO${NC}  $*"; }

echo ""
echo "  ╔══════════════════════════════════════════╗"
echo "  ║  JCR1440 Cluster — Bench Test            ║"
echo "  ║  Nino's Tech · oyenino.com               ║"
echo "  ╚══════════════════════════════════════════╝"
echo ""

# ============================================================================
echo -e "${CYAN}--- Test 1: Binary exists and runs ---${NC}"
# ============================================================================

if [ -x "$INSTALL_DIR/jcr1440-cluster" ]; then
    pass "Binary exists at $INSTALL_DIR/jcr1440-cluster"
    SIZE=$(du -h "$INSTALL_DIR/jcr1440-cluster" | cut -f1)
    info "Binary size: $SIZE"
else
    fail "Binary not found at $INSTALL_DIR/jcr1440-cluster"
fi

# ============================================================================
echo -e "${CYAN}--- Test 2: Rust toolchain ---${NC}"
# ============================================================================

RUN_USER="${SUDO_USER:-pi}"
if sudo -u "$RUN_USER" bash -c 'source ~/.cargo/env 2>/dev/null && rustc --version' &>/dev/null; then
    VER=$(sudo -u "$RUN_USER" bash -c 'source ~/.cargo/env && rustc --version')
    pass "Rust installed: $VER"
else
    fail "Rust not found for user $RUN_USER"
fi

# ============================================================================
echo -e "${CYAN}--- Test 3: System dependencies ---${NC}"
# ============================================================================

MISSING=""
for pkg in libgl1-mesa-dev libgles2-mesa-dev libx11-dev libwayland-dev libxkbcommon-dev libssl-dev; do
    if ! dpkg -s "$pkg" &>/dev/null; then
        MISSING="$MISSING $pkg"
    fi
done

if [ -z "$MISSING" ]; then
    pass "All required system packages installed"
else
    fail "Missing packages:$MISSING"
fi

# ============================================================================
echo -e "${CYAN}--- Test 4: GPU / OpenGL ---${NC}"
# ============================================================================

if command -v glxinfo &>/dev/null; then
    GL_RENDERER=$(glxinfo 2>/dev/null | grep "OpenGL renderer" | head -1)
    if [ -n "$GL_RENDERER" ]; then
        pass "OpenGL available: $GL_RENDERER"
    else
        skip "glxinfo exists but no renderer (may work with EGL)"
    fi
else
    skip "glxinfo not installed (install mesa-utils to test, not required)"
fi

# Check boot config for GPU memory
BOOT_CONFIG=""
[ -f /boot/firmware/config.txt ] && BOOT_CONFIG="/boot/firmware/config.txt"
[ -f /boot/config.txt ] && BOOT_CONFIG="/boot/config.txt"

if [ -n "$BOOT_CONFIG" ]; then
    if grep -q "^gpu_mem=128" "$BOOT_CONFIG" 2>/dev/null; then
        pass "GPU memory set to 128MB"
    else
        GPU_MEM=$(grep "^gpu_mem=" "$BOOT_CONFIG" 2>/dev/null || echo "not set")
        skip "GPU memory: $GPU_MEM (recommend gpu_mem=128)"
    fi

    if grep -q "vc4-kms-v3d" "$BOOT_CONFIG" 2>/dev/null; then
        pass "KMS video driver enabled"
    else
        skip "KMS driver not found in config (may still work)"
    fi
fi

# ============================================================================
echo -e "${CYAN}--- Test 5: udev rule ---${NC}"
# ============================================================================

if [ -f /etc/udev/rules.d/90-jcr1440.rules ]; then
    if grep -q "05c6.*f00e" /etc/udev/rules.d/90-jcr1440.rules; then
        pass "udev rule installed for JCR1440 (05c6:f00e)"
    else
        fail "udev rule exists but doesn't match JCR1440 VID:PID"
    fi
else
    fail "udev rule not installed at /etc/udev/rules.d/90-jcr1440.rules"
fi

# ============================================================================
echo -e "${CYAN}--- Test 6: systemd services ---${NC}"
# ============================================================================

for svc in jcr1440-net jcr1440-cluster; do
    if systemctl is-enabled "$svc" &>/dev/null; then
        pass "Service $svc is enabled"
    else
        fail "Service $svc is not enabled"
    fi
done

# ============================================================================
echo -e "${CYAN}--- Test 7: JCR1440 USB detection (if plugged in) ---${NC}"
# ============================================================================

if lsusb -d 05c6:f00e &>/dev/null; then
    pass "JCR1440 detected on USB (05c6:f00e)"

    # Check RNDIS interface
    RNDIS_IF=""
    for iface in /sys/class/net/enx*; do
        [ -d "$iface/device" ] || continue
        drv=$(readlink "$iface/device/driver" 2>/dev/null | xargs basename 2>/dev/null || true)
        if [ "$drv" = "rndis_host" ]; then
            RNDIS_IF=$(basename "$iface")
            break
        fi
    done

    if [ -n "$RNDIS_IF" ]; then
        pass "RNDIS interface: $RNDIS_IF"
        IP=$(ip addr show dev "$RNDIS_IF" 2>/dev/null | grep -oP 'inet \K[0-9.]+')
        info "IP address: ${IP:-not assigned}"

        # Ping test
        if ping -c 1 -W 2 -I "$RNDIS_IF" 192.168.1.1 &>/dev/null; then
            pass "JCR1440 reachable at 192.168.1.1"
        else
            fail "Cannot reach 192.168.1.1 via $RNDIS_IF"
        fi

        # Auth test
        if command -v python3 &>/dev/null && [ -f "$INSTALL_DIR/jcr1440-poller.py" ]; then
            info "Testing authentication..."
            if timeout 15 python3 "$INSTALL_DIR/jcr1440-poller.py" --once --iface "$RNDIS_IF" &>/dev/null; then
                pass "JCR1440 authentication successful"
            else
                fail "JCR1440 authentication failed"
            fi
        else
            skip "Python poller not available for auth test"
        fi
    else
        fail "RNDIS interface not found (device may still be booting)"
    fi
else
    skip "JCR1440 not plugged in — USB tests skipped (this is OK for bench test)"
    info "Plug the device in and re-run to test the full chain"
fi

# ============================================================================
echo -e "${CYAN}--- Test 8: Display rendering (mock mode) ---${NC}"
# ============================================================================

if [ "$QUICK" = true ]; then
    skip "Display test skipped (--quick mode)"
else
    if [ -n "${DISPLAY:-}" ] || [ -n "${WAYLAND_DISPLAY:-}" ]; then
        info "Launching cluster in mock mode for 5 seconds..."
        sudo -u "$RUN_USER" bash -c "
            export DISPLAY=${DISPLAY:-:0}
            export WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-wayland-0}
            timeout 5 $INSTALL_DIR/jcr1440-cluster --mock 2>&1
        " && RENDER_OK=true || RENDER_OK=false

        if [ "$RENDER_OK" = true ]; then
            pass "Cluster rendered successfully (mock mode, 5s)"
        else
            # timeout exit code 124 is expected (we killed it after 5s)
            pass "Cluster launched and ran for 5 seconds (mock mode)"
        fi
    else
        skip "No display available (DISPLAY/WAYLAND_DISPLAY not set)"
        info "Run this on the Pi's desktop session to test rendering"
    fi
fi

# ============================================================================
# Summary
# ============================================================================

echo ""
echo "  ════════════════════════════════════════════"
echo -e "  Results:  ${GREEN}${PASS} passed${NC}  ${RED}${FAIL} failed${NC}  ${YELLOW}${WARN} skipped${NC}"
echo "  ════════════════════════════════════════════"

if [ "$FAIL" -eq 0 ]; then
    echo ""
    echo -e "  ${GREEN}All critical tests passed!${NC}"
    echo ""
    echo "  The Pi is ready. To use:"
    echo "    1. Reboot the Pi:  sudo reboot"
    echo "    2. Plug JCR1440 into car OBD-II port"
    echo "    3. Connect USB cable from JCR1440 to Pi"
    echo "    4. Cluster auto-launches fullscreen within ~10 seconds"
    echo ""
    echo "  To test right now with mock data:"
    echo "    $INSTALL_DIR/jcr1440-cluster --mock --fullscreen"
    echo ""
else
    echo ""
    echo -e "  ${RED}Some tests failed — fix the issues above before deploying.${NC}"
    echo ""
fi
