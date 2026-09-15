#!/usr/bin/env bash
# ============================================================================
# JCR1440 Non-Destructive Diagnostic Script
# ============================================================================
#
# This script collects READ-ONLY diagnostic information from a connected
# JioMotive JCR1440 OBD-II telematics device.
#
# It does NOT:
#   - Write firmware
#   - Modify partitions
#   - Change modem configuration
#   - Change IMEI
#   - Reset eSIM
#   - Change carrier provisioning
#   - Send CAN commands
#   - Modify the JCR1440 in any way
#
# Usage: ./jcr1440-diagnostic.sh [output_dir]
#
# Output is saved to diagnostics/jcr1440/YYYYMMDD_HHMMSS/
# ============================================================================

set -euo pipefail

# --- Configuration ---
JCR1440_VID="05c6"
JCR1440_PID="f00e"
JCR1440_IP="192.168.1.1"
JCR1440_HOST="jiocarfi.local.html"

# --- Output directory ---
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
BASE_DIR="${1:-$(dirname "$(readlink -f "$0")")/../diagnostics/jcr1440}"
OUTPUT_DIR="${BASE_DIR}/${TIMESTAMP}"
mkdir -p "${OUTPUT_DIR}"

echo "=========================================="
echo "JCR1440 Diagnostic Script"
echo "Timestamp: ${TIMESTAMP}"
echo "Output:    ${OUTPUT_DIR}"
echo "=========================================="
echo ""

# --- Helper ---
run_cmd() {
    local label="$1"
    local file="$2"
    shift 2
    echo "[*] ${label}..."
    {
        echo "# Command: $*"
        echo "# Timestamp: $(date -Iseconds)"
        echo "---"
        "$@" 2>&1 || echo "[error] Command returned exit code $?"
    } > "${OUTPUT_DIR}/${file}"
}

# ============================================================================
# Section 1: USB Device Detection
# ============================================================================
echo "=== Section 1: USB Device Detection ==="

run_cmd "USB device list" "01_lsusb.txt" \
    lsusb

run_cmd "USB device tree" "02_lsusb_tree.txt" \
    lsusb -t

run_cmd "USB device details (full descriptors)" "03_lsusb_verbose.txt" \
    lsusb -v -d "${JCR1440_VID}:${JCR1440_PID}"

run_cmd "USB device info (usb-devices)" "04_usb_devices.txt" \
    usb-devices

# ============================================================================
# Section 2: Serial / Device Interfaces
# ============================================================================
echo ""
echo "=== Section 2: Serial / Device Interfaces ==="

echo "[*] Checking serial devices..."
{
    echo "# Serial device check"
    echo "# Timestamp: $(date -Iseconds)"
    echo "---"
    echo "=== /dev/ttyUSB* ==="
    ls -la /dev/ttyUSB* 2>&1 || echo "(none found)"
    echo ""
    echo "=== /dev/ttyACM* ==="
    ls -la /dev/ttyACM* 2>&1 || echo "(none found)"
    echo ""
    echo "=== /dev/serial/by-id/ ==="
    ls -la /dev/serial/by-id/ 2>&1 || echo "(none found)"
    echo ""
    echo "=== /dev/serial/by-path/ ==="
    ls -la /dev/serial/by-path/ 2>&1 || echo "(none found)"
    echo ""
    echo "=== /dev/cdc-wdm* ==="
    ls -la /dev/cdc-wdm* 2>&1 || echo "(none found)"
    echo ""
    echo "=== /dev/qcqmi* ==="
    ls -la /dev/qcqmi* 2>&1 || echo "(none found)"
} > "${OUTPUT_DIR}/05_serial_devices.txt"

# ============================================================================
# Section 3: Network Interfaces
# ============================================================================
echo ""
echo "=== Section 3: Network Interfaces ==="

run_cmd "Network links" "06_ip_link.txt" \
    ip link show

run_cmd "IP addresses" "07_ip_addr.txt" \
    ip addr show

run_cmd "Network routes" "08_ip_route.txt" \
    ip route show

run_cmd "NetworkManager devices" "09_nmcli_device.txt" \
    nmcli device

run_cmd "Block devices" "10_lsblk.txt" \
    lsblk

# ============================================================================
# Section 4: RNDIS Interface Details
# ============================================================================
echo ""
echo "=== Section 4: RNDIS Interface Details ==="

# Find the JCR1440 network interface
RNDIS_IF=""

# Method 1: Check if JCR1440 USB device is present and trace its network interface
if lsusb -d "${JCR1440_VID}:${JCR1440_PID}" &>/dev/null; then
    # Find the USB bus/device path
    usb_bus=$(lsusb -d "${JCR1440_VID}:${JCR1440_PID}" | awk '{print $2}')
    usb_dev=$(lsusb -d "${JCR1440_VID}:${JCR1440_PID}" | awk '{print $4}' | tr -d ':')

    # Search for network interfaces created by rndis_host driver
    for iface in /sys/class/net/*; do
        if [ -d "$iface/device" ]; then
            driver=$(readlink "$iface/device/driver" 2>/dev/null | xargs basename 2>/dev/null || true)
            if [ "$driver" = "rndis_host" ]; then
                RNDIS_IF=$(basename "$iface")
                break
            fi
        fi
    done
fi

# Method 2: Fallback — look for any rndis_host interface
if [ -z "${RNDIS_IF}" ]; then
    for iface in /sys/class/net/enx* /sys/class/net/usb* /sys/class/net/eth*; do
        if [ -d "$iface/device" ]; then
            driver=$(readlink "$iface/device/driver" 2>/dev/null | xargs basename 2>/dev/null || true)
            if [ "$driver" = "rndis_host" ]; then
                RNDIS_IF=$(basename "$iface")
                break
            fi
        fi
    done
fi

# Method 3: Check uevent for DRIVER=rndis_host
if [ -z "${RNDIS_IF}" ]; then
    for iface in /sys/class/net/enx*; do
        if [ -d "$iface" ] && [ -f "$iface/device/uevent" ]; then
            if grep -q "rndis_host" "$iface/device/uevent" 2>/dev/null; then
                RNDIS_IF=$(basename "$iface")
                break
            fi
        fi
    done
fi

if [ -n "${RNDIS_IF}" ]; then
    echo "[*] Found RNDIS interface: ${RNDIS_IF}"

    run_cmd "RNDIS interface details" "11_rndis_details.txt" \
        ip addr show dev "${RNDIS_IF}"

    run_cmd "RNDIS routes" "12_rndis_routes.txt" \
        ip route show dev "${RNDIS_IF}"

    run_cmd "RNDIS ARP neighbors" "13_rndis_neighbors.txt" \
        ip neigh show dev "${RNDIS_IF}"

    echo "[*] Checking sysfs attributes..."
    {
        echo "# RNDIS sysfs attributes"
        echo "# Interface: ${RNDIS_IF}"
        echo "# Timestamp: $(date -Iseconds)"
        echo "---"
        for attr in address operstate speed mtu type carrier; do
            val=$(cat "/sys/class/net/${RNDIS_IF}/${attr}" 2>&1 || echo "N/A")
            echo "${attr}: ${val}"
        done
    } > "${OUTPUT_DIR}/14_rndis_sysfs.txt"
else
    echo "[!] No RNDIS interface found — device may not be connected"
    echo "No RNDIS interface found" > "${OUTPUT_DIR}/11_rndis_not_found.txt"
fi

# ============================================================================
# Section 5: Web API (read-only, no authentication)
# ============================================================================
echo ""
echo "=== Section 5: Web API (unauthenticated endpoints) ==="

if [ -n "${RNDIS_IF}" ]; then
    # Check if device is reachable
    if ping -c 1 -W 2 -I "${RNDIS_IF}" "${JCR1440_IP}" &>/dev/null; then
        echo "[*] Device reachable at ${JCR1440_IP}"

        run_cmd "HTTP headers" "15_http_headers.txt" \
            curl -sS --connect-timeout 5 --interface "${RNDIS_IF}" \
                -D - -o /dev/null "http://${JCR1440_IP}/"

        run_cmd "Device status XML" "16_st_dev.xml" \
            curl -sS --connect-timeout 5 --interface "${RNDIS_IF}" \
                -H "Host: ${JCR1440_HOST}" "http://${JCR1440_IP}/st_dev.w.xml"

        run_cmd "LTE status XML" "17_st_lte.xml" \
            curl -sS --connect-timeout 5 --interface "${RNDIS_IF}" \
                -H "Host: ${JCR1440_HOST}" "http://${JCR1440_IP}/st_lte.w.xml"

        run_cmd "Bluetooth status XML" "18_st_bt.xml" \
            curl -sS --connect-timeout 5 --interface "${RNDIS_IF}" \
                -H "Host: ${JCR1440_HOST}" "http://${JCR1440_IP}/st_bt.w.xml"

        run_cmd "Title status XML" "19_mark_title.xml" \
            curl -sS --connect-timeout 5 --interface "${RNDIS_IF}" \
                -H "Host: ${JCR1440_HOST}" "http://${JCR1440_IP}/mark_title.w.xml"

        run_cmd "Language/auth XML" "20_mark_lang.xml" \
            curl -sS --connect-timeout 5 --interface "${RNDIS_IF}" \
                -H "Host: ${JCR1440_HOST}" "http://${JCR1440_IP}/mark_lang.w.xml"

        # Port scan (limited, device only, interface-bound via curl)
        # NOTE: /dev/tcp cannot bind to an interface, so we use curl
        # to avoid hitting a home router at the same IP on another interface.
        echo "[*] Checking open ports on ${JCR1440_IP} (interface-bound)..."
        {
            echo "# Port scan of JCR1440 (${JCR1440_IP}) via ${RNDIS_IF}"
            echo "# Using curl --interface to avoid subnet collision"
            echo "# Timestamp: $(date -Iseconds)"
            echo "---"
            for port in 22 23 53 80 443 554 1883 5555 8080 8443 8883; do
                result=$(curl -s -o /dev/null -w "%{http_code}" --connect-timeout 2 \
                    --interface "${RNDIS_IF}" "http://${JCR1440_IP}:${port}/" 2>&1 || true)
                if [ "$result" != "000" ] && [ "$result" != "" ]; then
                    echo "Port ${port}: OPEN (HTTP response: ${result})"
                else
                    # Try raw TCP via ncat/nc with source binding if available
                    if command -v ncat &>/dev/null; then
                        if ncat -z -w 2 -s 192.168.1.100 "${JCR1440_IP}" "${port}" 2>/dev/null; then
                            echo "Port ${port}: OPEN (TCP)"
                        else
                            echo "Port ${port}: closed"
                        fi
                    else
                        echo "Port ${port}: ${result:-closed} (curl probe)"
                    fi
                fi
            done
        } > "${OUTPUT_DIR}/21_port_scan.txt"
    else
        echo "[!] Device not reachable at ${JCR1440_IP}"
        echo "Device not reachable" > "${OUTPUT_DIR}/15_device_unreachable.txt"
    fi
else
    echo "[!] Skipping web API checks (no RNDIS interface)"
fi

# ============================================================================
# Section 6: Kernel Messages
# ============================================================================
echo ""
echo "=== Section 6: Kernel Messages ==="

run_cmd "Kernel messages (JCR1440 related)" "22_dmesg_jcr1440.txt" \
    journalctl -k --no-pager -o short-iso

# Filter for relevant messages
{
    echo "# Filtered kernel messages for JCR1440"
    echo "# Timestamp: $(date -Iseconds)"
    echo "---"
    journalctl -k --no-pager 2>/dev/null | \
        grep -iE "05c6|f00e|qualcomm|rndis|mobile.router|cdc" || \
        echo "(no matching messages found)"
} > "${OUTPUT_DIR}/23_dmesg_filtered.txt"

# ============================================================================
# Section 7: ADB / Fastboot Check
# ============================================================================
echo ""
echo "=== Section 7: ADB / Fastboot Check ==="

echo "[*] Checking ADB..."
{
    echo "# ADB device check"
    echo "# Timestamp: $(date -Iseconds)"
    echo "---"
    if command -v adb &>/dev/null; then
        adb devices 2>&1
    else
        echo "adb not installed"
    fi
} > "${OUTPUT_DIR}/24_adb_devices.txt"

echo "[*] Checking Fastboot..."
{
    echo "# Fastboot device check"
    echo "# Timestamp: $(date -Iseconds)"
    echo "---"
    if command -v fastboot &>/dev/null; then
        fastboot devices 2>&1
    else
        echo "fastboot not installed"
    fi
} > "${OUTPUT_DIR}/25_fastboot_devices.txt"

# ============================================================================
# Section 8: ModemManager
# ============================================================================
echo ""
echo "=== Section 8: ModemManager ==="

{
    echo "# ModemManager device list"
    echo "# Timestamp: $(date -Iseconds)"
    echo "---"
    if command -v mmcli &>/dev/null; then
        mmcli -L 2>&1 || echo "ModemManager: no modems found or service not running"
    else
        echo "mmcli not installed"
    fi
} > "${OUTPUT_DIR}/26_modemmanager.txt"

# ============================================================================
# Section 9: System Information
# ============================================================================
echo ""
echo "=== Section 9: System Information ==="

{
    echo "# Host system information"
    echo "# Timestamp: $(date -Iseconds)"
    echo "---"
    echo "Hostname: $(hostname)"
    echo "Kernel: $(uname -r)"
    echo "Architecture: $(uname -m)"
    echo "Distribution: $(cat /etc/os-release 2>/dev/null | head -5)"
    echo ""
    echo "USB kernel modules loaded:"
    lsmod 2>/dev/null | grep -iE "rndis|cdc|usb|qualcomm|qmi|mbim" || echo "(none matching)"
} > "${OUTPUT_DIR}/27_system_info.txt"

# ============================================================================
# Summary
# ============================================================================
echo ""
echo "=========================================="
echo "Diagnostic collection complete"
echo "Output saved to: ${OUTPUT_DIR}"
echo ""
echo "Files collected:"
ls -1 "${OUTPUT_DIR}/"
echo ""
echo "Total size: $(du -sh "${OUTPUT_DIR}" | cut -f1)"
echo "=========================================="

# Create a summary file
{
    echo "JCR1440 Diagnostic Summary"
    echo "========================="
    echo "Date: $(date -Iseconds)"
    echo "Host: $(hostname)"
    echo "Kernel: $(uname -r)"
    echo ""

    # Check if device was found
    if lsusb -d "${JCR1440_VID}:${JCR1440_PID}" &>/dev/null; then
        echo "JCR1440 USB: DETECTED"
        echo "  VID:PID: ${JCR1440_VID}:${JCR1440_PID}"
        echo "  Product: $(lsusb -d ${JCR1440_VID}:${JCR1440_PID} 2>/dev/null)"
    else
        echo "JCR1440 USB: NOT DETECTED"
    fi

    if [ -n "${RNDIS_IF}" ]; then
        echo "RNDIS Interface: ${RNDIS_IF}"
        echo "  MAC: $(cat /sys/class/net/${RNDIS_IF}/address 2>/dev/null || echo 'N/A')"
        echo "  State: $(cat /sys/class/net/${RNDIS_IF}/operstate 2>/dev/null || echo 'N/A')"
        ip addr show dev "${RNDIS_IF}" 2>/dev/null | grep "inet " | awk '{print "  IPv4: " $2}'
    else
        echo "RNDIS Interface: NOT FOUND"
    fi

    echo ""
    echo "Files in this diagnostic run:"
    ls -la "${OUTPUT_DIR}/"
} > "${OUTPUT_DIR}/00_summary.txt"

echo "Done. Review ${OUTPUT_DIR}/00_summary.txt for a quick overview."
