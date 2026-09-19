#!/usr/bin/env python3
"""
JCR1440 Telemetry & Diagnostic Monitor

Boot-time service that observes the JCR1440 OBD-II dongle and logs
everything it can detect — USB presence, interfaces, serial data,
network activity, GPS/NMEA, OBD evidence — without ever modifying
the device.

All output goes to timestamped session directories under /var/log/jcr1440/.
"""

import datetime
import glob
import json
import os
import re
import signal
import subprocess
import sys
import textwrap
import threading
import time
from pathlib import Path

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

LOG_ROOT = Path("/var/log/jcr1440")
STATE_FILE = Path("/run/jcr1440-monitor.state")
PID_FILE = Path("/run/jcr1440-monitor.pid")

# Known JCR1440 USB identifiers
KNOWN_VID = "05c6"
KNOWN_PID = "f00e"
KNOWN_MANUFACTURER = "qualcomm"
KNOWN_PRODUCT_HINTS = ["jcr1440", "fp3", "rndis", "qualcomm"]

# Keywords for kernel log filtering
KERNEL_KEYWORDS = [
    "usb", "tty", "cdc", "qmi", "mbim", "rndis", "wwan",
    "modem", "serial", "adb", "fastboot", "storage", "scsi",
]

# Serial read settings
SERIAL_SAMPLE_SIZE = 4096  # bytes per read attempt
SERIAL_MAX_CAPTURE = 10 * 1024 * 1024  # 10 MB per interface per session
SERIAL_READ_TIMEOUT = 2  # seconds

# Monitoring intervals
HEALTH_INTERVAL = 30  # seconds
NET_COUNTER_INTERVAL = 5  # seconds
UDEV_POLL_INTERVAL = 1  # seconds

# NMEA sentence prefixes
NMEA_PREFIXES = ["$GPGGA", "$GPRMC", "$GNGGA", "$GNRMC", "$GPGLL", "$GNGLL"]

# Redaction patterns
REDACT_PATTERNS = [
    (re.compile(r"(IMEI[:\s=]*)\d{11,15}", re.I), r"\g<1>***REDACTED***"),
    (re.compile(r"(ICCID[:\s=]*)\d{15,22}", re.I), r"\g<1>***REDACTED***"),
    (re.compile(r"(IMSI[:\s=]*)\d{10,15}", re.I), r"\g<1>***REDACTED***"),
    (re.compile(r"(MSISDN[:\s=]*)\d{8,15}", re.I), r"\g<1>***REDACTED***"),
    (re.compile(r"(phone[:\s=]*)\+?\d{8,15}", re.I), r"\g<1>***REDACTED***"),
    (re.compile(r"(password[:\s=]*)\S+", re.I), r"\g<1>***REDACTED***"),
    (re.compile(r"(token[:\s=]*)\S+", re.I), r"\g<1>***REDACTED***"),
]


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def ts():
    """Current timestamp string."""
    return datetime.datetime.now().strftime("%Y-%m-%d %H:%M:%S")


def ts_short():
    """Short timestamp for inline logs."""
    return datetime.datetime.now().strftime("%H:%M:%S")


def run(cmd, timeout=30):
    """Run a shell command, return (stdout, stderr, returncode)."""
    try:
        r = subprocess.run(
            cmd, shell=True, capture_output=True, text=True, timeout=timeout
        )
        return r.stdout, r.stderr, r.returncode
    except subprocess.TimeoutExpired:
        return "", f"TIMEOUT after {timeout}s", -1
    except Exception as e:
        return "", str(e), -1


def redact(text):
    """Redact sensitive identifiers from text."""
    for pat, repl in REDACT_PATTERNS:
        text = pat.sub(repl, text)
    return text


def safe_read_file(path):
    """Read a file, return contents or empty string."""
    try:
        return Path(path).read_text(errors="replace").strip()
    except Exception:
        return ""


# ---------------------------------------------------------------------------
# Logger — writes to per-topic log files in the session directory
# ---------------------------------------------------------------------------

class SessionLogger:
    def __init__(self, session_dir):
        self.session_dir = Path(session_dir)
        self.session_dir.mkdir(parents=True, exist_ok=True)
        self._files = {}
        self._locks = {}

    def _get(self, name):
        if name not in self._files:
            path = self.session_dir / f"{name}.log"
            self._files[name] = open(path, "a", buffering=1)
            self._locks[name] = threading.Lock()
        return self._files[name], self._locks[name]

    def log(self, name, msg, timestamp=True):
        f, lock = self._get(name)
        with lock:
            if timestamp:
                f.write(f"[{ts()}] {msg}\n")
            else:
                f.write(f"{msg}\n")
            f.flush()

    def log_block(self, name, title, content):
        f, lock = self._get(name)
        with lock:
            f.write(f"\n{'='*60}\n")
            f.write(f"[{ts()}] {title}\n")
            f.write(f"{'='*60}\n")
            f.write(redact(content))
            f.write(f"\n{'='*60}\n\n")
            f.flush()

    def close(self):
        for f in self._files.values():
            try:
                f.close()
            except Exception:
                pass


# ---------------------------------------------------------------------------
# Phase 1 — Detect the JCR1440
# ---------------------------------------------------------------------------

class DeviceDetector:
    def __init__(self, logger):
        self.log = logger
        self.detected = False
        self.usb_info = {}

    def run_initial(self):
        self.log.log("summary", "=== PHASE 1: DEVICE DETECTION ===")

        # System info
        out, _, _ = run("uname -a")
        self.log.log("system", f"Kernel: {out.strip()}")
        out, _, _ = run("cat /proc/device-tree/model 2>/dev/null")
        self.log.log("system", f"Model: {out.strip()}")
        out, _, _ = run("date")
        self.log.log("system", f"Date: {out.strip()}")
        out, _, _ = run("uptime")
        self.log.log("system", f"Uptime: {out.strip()}")

        # USB enumeration
        lsusb_out, _, _ = run("lsusb")
        self.log.log_block("usb", "lsusb", lsusb_out)

        lsusb_t, _, _ = run("lsusb -t")
        self.log.log_block("usb", "lsusb -t", lsusb_t)

        usbdev, _, _ = run("usb-devices 2>/dev/null")
        self.log.log_block("usb", "usb-devices", usbdev)

        # Kernel logs
        dmesg, _, _ = run("dmesg")
        self.log.log_block("kernel", "dmesg (full)", dmesg)

        journalk, _, _ = run("journalctl -k --no-pager -b 2>/dev/null")
        self.log.log_block("kernel", "journalctl -k", journalk)

        # Filtered kernel lines
        combined = dmesg + "\n" + journalk
        for kw in KERNEL_KEYWORDS:
            filtered = "\n".join(
                l for l in combined.splitlines()
                if kw.lower() in l.lower()
            )
            if filtered:
                self.log.log_block("kernel", f"Filtered: {kw}", filtered)

        # Device detection
        self.detected = False
        self.usb_info = {}

        for line in lsusb_out.splitlines():
            ll = line.lower()
            if f"{KNOWN_VID}:{KNOWN_PID}" in ll or any(h in ll for h in KNOWN_PRODUCT_HINTS):
                self.detected = True
                self.usb_info["lsusb_line"] = line.strip()
                break

        # Try to get detailed descriptor
        if self.detected:
            out, _, _ = run(f"lsusb -v -d {KNOWN_VID}:{KNOWN_PID} 2>/dev/null")
            self.log.log_block("usb", "Device descriptor", out)
            self._parse_descriptor(out)

        # Also check sysfs
        self._check_sysfs()

        status = "YES" if self.detected else "NO"
        self.log.log("summary", f"JCR1440 DETECTED: {status}")
        self.log.log("usb", f"JCR1440 DETECTED: {status}")

        if self.usb_info:
            for k, v in self.usb_info.items():
                self.log.log("usb", f"  {k}: {v}")
                self.log.log("summary", f"  {k}: {v}")

        return self.detected

    def _parse_descriptor(self, desc):
        for line in desc.splitlines():
            line = line.strip()
            if line.startswith("idVendor"):
                self.usb_info["vid"] = line
            elif line.startswith("idProduct"):
                self.usb_info["pid"] = line
            elif line.startswith("iManufacturer"):
                self.usb_info["manufacturer"] = line
            elif line.startswith("iProduct"):
                self.usb_info["product"] = line
            elif line.startswith("iSerial"):
                self.usb_info["serial"] = redact(line)
            elif line.startswith("bInterfaceClass"):
                self.usb_info.setdefault("classes", []).append(line)

    def _check_sysfs(self):
        for dev_path in glob.glob("/sys/bus/usb/devices/*/idVendor"):
            base = os.path.dirname(dev_path)
            vid = safe_read_file(dev_path)
            pid = safe_read_file(os.path.join(base, "idProduct"))
            if vid == KNOWN_VID and pid == KNOWN_PID:
                self.detected = True
                self.usb_info["sysfs_path"] = base
                self.usb_info["sysfs_manufacturer"] = safe_read_file(
                    os.path.join(base, "manufacturer"))
                self.usb_info["sysfs_product"] = safe_read_file(
                    os.path.join(base, "product"))
                self.usb_info["sysfs_serial"] = redact(safe_read_file(
                    os.path.join(base, "serial")))
                break


# ---------------------------------------------------------------------------
# Phase 2 — Detect exposed interfaces
# ---------------------------------------------------------------------------

class InterfaceDetector:
    def __init__(self, logger):
        self.log = logger
        self.serial_ports = []
        self.net_interfaces = []
        self.block_devices = []
        self.modem_interfaces = []
        self.adb_present = False

    def run_initial(self):
        self.log.log("summary", "=== PHASE 2: INTERFACE DETECTION ===")

        # Serial ports
        self.serial_ports = []
        for pattern in ["/dev/ttyUSB*", "/dev/ttyACM*"]:
            self.serial_ports.extend(glob.glob(pattern))
        serial_links = glob.glob("/dev/serial/by-id/*")

        self.log.log("serial", f"Serial ports found: {self.serial_ports}")
        self.log.log("serial", f"Serial by-id links: {serial_links}")

        # Detailed info per serial port
        for port in self.serial_ports:
            out, _, _ = run(f"udevadm info --query=all --name={port} 2>/dev/null")
            self.log.log_block("serial", f"udevadm info {port}", out)

        # Network interfaces
        ip_link, _, _ = run("ip link")
        self.log.log_block("network", "ip link", ip_link)

        ip_addr, _, _ = run("ip addr")
        self.log.log_block("network", "ip addr", ip_addr)

        ip_route, _, _ = run("ip route")
        self.log.log_block("network", "ip route", ip_route)

        nmcli, _, _ = run("nmcli device status 2>/dev/null")
        self.log.log_block("network", "nmcli device status", nmcli)

        # Find device-related network interfaces
        self.net_interfaces = []
        for iface_dir in glob.glob("/sys/class/net/*/device/driver"):
            drv = os.path.basename(os.readlink(iface_dir))
            iface = iface_dir.split("/")[4]
            if drv in ("rndis_host", "cdc_ether", "cdc_ncm", "qmi_wwan", "cdc_mbim"):
                self.net_interfaces.append((iface, drv))
                self.log.log("network", f"Device interface: {iface} (driver={drv})")

        # Block devices
        lsblk, _, _ = run("lsblk -o NAME,SIZE,TYPE,TRAN,MODEL 2>/dev/null")
        self.log.log_block("storage", "lsblk", lsblk)
        self.block_devices = []
        # Check for USB mass storage from the device
        for sd in glob.glob("/sys/block/sd*/device"):
            real = os.path.realpath(sd)
            if "usb" in real:
                name = sd.split("/")[3]
                self.block_devices.append(f"/dev/{name}")
                self.log.log("storage", f"USB block device: /dev/{name}")

        # Modem interfaces
        self.modem_interfaces = glob.glob("/dev/cdc-wdm*") + glob.glob("/dev/qcqmi*")
        self.log.log("modem", f"Modem control interfaces: {self.modem_interfaces}")

        # ADB
        adb_out, _, rc = run("adb devices 2>/dev/null")
        self.adb_present = rc == 0 and "device" in adb_out.replace("List of devices", "")
        self.log.log("adb", f"ADB output:\n{adb_out}")

        # Summary
        summary = textwrap.dedent(f"""\
            USB serial ports: {len(self.serial_ports)}
            Network interfaces from device: {len(self.net_interfaces)}
            Block devices from device: {len(self.block_devices)}
            Possible modem interfaces: {len(self.modem_interfaces)}
            ADB interface: {'YES' if self.adb_present else 'NO'}
        """)
        self.log.log("summary", summary, timestamp=False)
        return self


# ---------------------------------------------------------------------------
# Phase 3 — udev event monitor (runs in background thread)
# ---------------------------------------------------------------------------

class UdevMonitor:
    def __init__(self, logger, state):
        self.log = logger
        self.state = state
        self._stop = threading.Event()

    def start(self):
        t = threading.Thread(target=self._run, daemon=True, name="udev-monitor")
        t.start()
        return t

    def stop(self):
        self._stop.set()

    def _run(self):
        self.log.log("summary", "=== PHASE 3: UDEV MONITOR STARTED ===")
        try:
            proc = subprocess.Popen(
                ["udevadm", "monitor", "--udev", "--property"],
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
            )
            while not self._stop.is_set():
                line = proc.stdout.readline()
                if not line:
                    if proc.poll() is not None:
                        break
                    time.sleep(0.1)
                    continue

                line = line.strip()
                if not line:
                    continue

                # Parse udev events for USB/serial/network
                ll = line.lower()
                event_type = None

                if "add" in ll and ("usb" in ll or "tty" in ll or "net" in ll):
                    event_type = "CREATED"
                elif "remove" in ll and ("usb" in ll or "tty" in ll or "net" in ll):
                    event_type = "REMOVED"
                elif "change" in ll and "usb" in ll:
                    event_type = "CHANGED"

                if event_type:
                    msg = f"{ts_short()} {event_type}: {line}"
                    self.log.log("usb", msg)
                    self.log.log("summary", msg)

                    # Detect specific events
                    if "ttyusb" in ll or "ttyacm" in ll:
                        devname = ""
                        if "DEVNAME=" in line:
                            devname = line.split("DEVNAME=")[-1].split()[0]
                        if "add" in ll:
                            self.log.log("serial", f"{ts_short()} {devname} CREATED")
                            self.state["serial_events"].append(
                                (ts(), devname, "CREATED"))
                        elif "remove" in ll:
                            self.log.log("serial", f"{ts_short()} {devname} REMOVED")
                            self.state["serial_events"].append(
                                (ts(), devname, "REMOVED"))

                    if any(x in ll for x in ["wwan", "enx", "usb0", "eth"]) and "net" in ll:
                        if "add" in ll:
                            self.log.log("network", f"{ts_short()} NETWORK INTERFACE CREATED")
                        elif "remove" in ll:
                            self.log.log("network", f"{ts_short()} NETWORK INTERFACE REMOVED")

            proc.terminate()
        except Exception as e:
            self.log.log("summary", f"udev monitor error: {e}")


# ---------------------------------------------------------------------------
# Phase 4 — Serial data monitoring (runs in background threads)
# ---------------------------------------------------------------------------

class SerialMonitor:
    def __init__(self, logger, state):
        self.log = logger
        self.state = state
        self._stop = threading.Event()
        self._threads = []

    def start(self, ports):
        for port in ports:
            t = threading.Thread(
                target=self._monitor_port, args=(port,),
                daemon=True, name=f"serial-{os.path.basename(port)}"
            )
            t.start()
            self._threads.append(t)

    def stop(self):
        self._stop.set()

    def _monitor_port(self, port):
        port_name = os.path.basename(port)
        self.log.log("serial", f"Monitoring {port}")
        self.log.log("summary", f"=== PHASE 4: SERIAL MONITOR {port} ===")

        # Get udev info
        out, _, _ = run(f"udevadm info --query=all --name={port} 2>/dev/null")
        self.log.log("serial", f"{port} udev info:\n{redact(out)}")

        total_bytes = 0
        capture_path = self.log.session_dir / f"serial_{port_name}_capture.bin"
        ascii_sample_path = self.log.session_dir / f"serial_{port_name}_ascii.log"
        gps_detected = False
        data_formats = set()

        try:
            fd = os.open(port, os.O_RDONLY | os.O_NONBLOCK)
        except Exception as e:
            self.log.log("serial", f"Cannot open {port}: {e}")
            return

        try:
            while not self._stop.is_set() and total_bytes < SERIAL_MAX_CAPTURE:
                try:
                    data = os.read(fd, SERIAL_SAMPLE_SIZE)
                    if not data:
                        time.sleep(0.5)
                        continue

                    total_bytes += len(data)
                    self.state["serial_bytes"][port_name] = total_bytes
                    self.state["last_serial_data"] = time.time()

                    # Save raw capture (within limit)
                    with open(capture_path, "ab") as f:
                        f.write(data)

                    # Try to decode as text
                    try:
                        text = data.decode("ascii", errors="replace")
                        # Check for known formats
                        for prefix in NMEA_PREFIXES:
                            if prefix in text:
                                gps_detected = True
                                data_formats.add("NMEA_GPS")
                                self.state["gps_detected"] = True
                                self.log.log("data-flow",
                                             f"GPS DATA DETECTED on {port}")

                        if text.startswith("AT") or "\r\nOK\r\n" in text:
                            data_formats.add("AT_RESPONSE")
                        if text.startswith("{") or text.startswith("["):
                            data_formats.add("JSON")

                        # Save ASCII sample (redacted)
                        if text.isprintable() or "\n" in text:
                            with open(ascii_sample_path, "a") as f:
                                f.write(redact(text))

                    except Exception:
                        data_formats.add("BINARY")

                    self.log.log("serial",
                                 f"{port_name} bytes received: {total_bytes}")

                except BlockingIOError:
                    time.sleep(0.5)
                except OSError:
                    time.sleep(1)

        finally:
            os.close(fd)

        self.log.log("serial",
                      f"{port_name} total bytes: {total_bytes}, "
                      f"formats: {data_formats or 'none'}")
        self.state["serial_formats"][port_name] = list(data_formats)


# ---------------------------------------------------------------------------
# Phase 5 — Safe modem detection
# ---------------------------------------------------------------------------

class ModemDetector:
    def __init__(self, logger):
        self.log = logger
        self.modem_detected = False
        self.modem_info = {}

    def run(self):
        self.log.log("summary", "=== PHASE 5: MODEM DETECTION ===")

        out, _, rc = run("mmcli -L 2>/dev/null")
        self.log.log("modem", f"mmcli -L:\n{out}")

        if rc != 0 or "No modems" in out or not out.strip():
            self.log.log("modem", "ModemManager: no modems detected")
            self.log.log("summary", "ModemManager detected: NO")
            return self

        # Extract modem index
        match = re.search(r"/Modem/(\d+)", out)
        if not match:
            self.log.log("modem", "Could not parse modem index")
            return self

        modem_id = match.group(1)
        self.modem_detected = True

        # Get modem details (read-only)
        detail, _, _ = run(f"mmcli -m {modem_id} 2>/dev/null")
        detail = redact(detail)
        self.log.log_block("modem", f"mmcli -m {modem_id}", detail)

        # Parse key fields
        for line in detail.splitlines():
            line = line.strip()
            for field in ["manufacturer", "model", "firmware revision",
                          "state", "signal quality", "access tech"]:
                if field in line.lower():
                    self.modem_info[field] = line
                    self.log.log("modem", f"  {line}")

        self.log.log("summary", f"ModemManager detected: YES")
        for k, v in self.modem_info.items():
            self.log.log("summary", f"  {v}")

        return self


# ---------------------------------------------------------------------------
# Phase 6 — Network interface monitoring (background thread)
# ---------------------------------------------------------------------------

class NetworkMonitor:
    def __init__(self, logger, state):
        self.log = logger
        self.state = state
        self._stop = threading.Event()

    def start(self, interfaces):
        if not interfaces:
            self.log.log("network", "No device network interfaces to monitor")
            return None
        t = threading.Thread(
            target=self._run, args=(interfaces,),
            daemon=True, name="net-monitor"
        )
        t.start()
        return t

    def stop(self):
        self._stop.set()

    def _run(self, interfaces):
        self.log.log("summary", "=== PHASE 6: NETWORK MONITOR STARTED ===")

        # Initial detailed capture per interface
        for iface, drv in interfaces:
            out, _, _ = run(f"ethtool {iface} 2>/dev/null")
            self.log.log_block("network", f"ethtool {iface}", out)
            out, _, _ = run(f"nmcli device show {iface} 2>/dev/null")
            self.log.log_block("network", f"nmcli device show {iface}", out)

        prev_counters = {}

        while not self._stop.is_set():
            for iface, drv in interfaces:
                rx = self._read_counter(iface, "rx_bytes")
                tx = self._read_counter(iface, "tx_bytes")

                key = iface
                prev = prev_counters.get(key, (0, 0))
                rx_increasing = rx > prev[0]
                tx_increasing = tx > prev[1]
                prev_counters[key] = (rx, tx)

                self.state["net_counters"][iface] = {
                    "rx": rx, "tx": tx,
                    "rx_increasing": rx_increasing,
                    "tx_increasing": tx_increasing,
                    "timestamp": time.time(),
                }

                self.log.log("network",
                             f"{ts_short()} {iface} RX={rx} TX={tx}")

                if rx_increasing or tx_increasing:
                    self.state["net_data_flowing"] = True
                    self.state["last_net_data"] = time.time()

            self._stop.wait(NET_COUNTER_INTERVAL)

    def _read_counter(self, iface, counter):
        path = f"/sys/class/net/{iface}/statistics/{counter}"
        try:
            return int(Path(path).read_text().strip())
        except Exception:
            return 0


# ---------------------------------------------------------------------------
# Phase 7 — USB activity observation
# ---------------------------------------------------------------------------

class USBActivityMonitor:
    def __init__(self, logger, state):
        self.log = logger
        self.state = state
        self._stop = threading.Event()

    def start(self, sysfs_path):
        if not sysfs_path:
            return None
        t = threading.Thread(
            target=self._run, args=(sysfs_path,),
            daemon=True, name="usb-activity"
        )
        t.start()
        return t

    def stop(self):
        self._stop.set()

    def _run(self, sysfs_path):
        self.log.log("summary", "=== PHASE 7: USB ACTIVITY MONITOR ===")

        # Capture sysfs info
        for fname in ["idVendor", "idProduct", "manufacturer", "product",
                       "serial", "speed", "bNumConfigurations",
                       "bDeviceClass", "bDeviceSubClass", "bDeviceProtocol",
                       "bMaxPower", "urbnum"]:
            val = safe_read_file(os.path.join(sysfs_path, fname))
            if val:
                self.log.log("usb", f"sysfs {fname}: {redact(val)}")

        prev_urbnum = 0
        while not self._stop.is_set():
            urbnum_str = safe_read_file(os.path.join(sysfs_path, "urbnum"))
            try:
                urbnum = int(urbnum_str)
            except (ValueError, TypeError):
                urbnum = 0

            if urbnum > prev_urbnum and prev_urbnum > 0:
                delta = urbnum - prev_urbnum
                self.state["usb_active"] = True
                self.state["usb_urbnum_delta"] = delta
                self.log.log("usb",
                             f"{ts_short()} USB URBs: {urbnum} (+{delta})")
            prev_urbnum = urbnum

            self._stop.wait(5)


# ---------------------------------------------------------------------------
# Phase 8 & 9 — GPS/NMEA and OBD/CAN detection (checked from serial data)
# ---------------------------------------------------------------------------

class TelemetryAnalyzer:
    """Periodically checks state for GPS and OBD evidence."""

    def __init__(self, logger, state):
        self.log = logger
        self.state = state
        self._stop = threading.Event()

    def start(self):
        t = threading.Thread(target=self._run, daemon=True, name="telemetry-analyzer")
        t.start()
        return t

    def stop(self):
        self._stop.set()

    def _run(self):
        reported_gps = False
        reported_obd = False

        while not self._stop.is_set():
            # Phase 8: GPS
            if self.state.get("gps_detected") and not reported_gps:
                self.log.log("data-flow", "GPS DATA DETECTED: YES")
                self.log.log("summary", "GPS DATA DETECTED: YES")
                reported_gps = True

            # Phase 9: OBD/CAN evidence
            # Check serial captures for OBD-like patterns
            if not reported_obd:
                for cap in self.log.session_dir.glob("serial_*_ascii.log"):
                    try:
                        text = cap.read_text(errors="replace")[-4096:]
                        obd_evidence = self._check_obd_evidence(text)
                        if obd_evidence:
                            self.state["obd_detected"] = True
                            self.log.log("obd",
                                         f"OBD/CAN DATA DETECTED: POSSIBLE")
                            self.log.log("obd",
                                         f"Evidence: {obd_evidence}")
                            self.log.log("summary",
                                         f"OBD telemetry: POSSIBLE ({obd_evidence})")
                            reported_obd = True
                            break
                    except Exception:
                        pass

            # Also check binary captures for changing payloads
            if not reported_obd:
                for cap in self.log.session_dir.glob("serial_*_capture.bin"):
                    try:
                        data = cap.read_bytes()[-2048:]
                        if len(data) > 100 and self._has_varying_payload(data):
                            self.state["obd_detected"] = True
                            self.log.log("obd",
                                         "OBD/CAN DATA DETECTED: POSSIBLE")
                            self.log.log("obd",
                                         "Evidence: changing binary payload")
                            reported_obd = True
                            break
                    except Exception:
                        pass

            self._stop.wait(10)

    def _check_obd_evidence(self, text):
        indicators = []
        # RPM-like values
        if re.search(r"RPM[:\s=]*\d{3,5}", text, re.I):
            indicators.append("RPM values")
        # Speed
        if re.search(r"(speed|vspeed|kph|mph)[:\s=]*\d{1,3}", text, re.I):
            indicators.append("speed values")
        # Voltage
        if re.search(r"(volt|batt|celv)[:\s=]*\d{1,2}\.\d", text, re.I):
            indicators.append("voltage values")
        # VIN pattern
        if re.search(r"[A-HJ-NPR-Z0-9]{17}", text):
            indicators.append("possible VIN")
        # CAN frame hex patterns
        if re.search(r"[0-9A-F]{3,4}\s*#\s*[0-9A-F]{2,16}", text, re.I):
            indicators.append("CAN frame patterns")
        return ", ".join(indicators) if indicators else None

    def _has_varying_payload(self, data):
        """Check if binary data has varying content (not just zeros/static)."""
        if len(data) < 64:
            return False
        chunks = [data[i:i+16] for i in range(0, len(data)-16, 16)]
        unique = len(set(chunks))
        return unique > len(chunks) * 0.3


# ---------------------------------------------------------------------------
# Phase 10 — Health status writer
# ---------------------------------------------------------------------------

class HealthReporter:
    def __init__(self, logger, state, detector, iface_detector):
        self.log = logger
        self.state = state
        self.detector = detector
        self.iface_detector = iface_detector
        self._stop = threading.Event()

    def start(self):
        t = threading.Thread(target=self._run, daemon=True, name="health")
        t.start()
        return t

    def stop(self):
        self._stop.set()

    def get_status_dict(self):
        now = time.time()
        serial_active = (now - self.state.get("last_serial_data", 0)) < 30
        net_active = (now - self.state.get("last_net_data", 0)) < 30

        # Re-check device presence
        _, _, rc = run(f"lsusb -d {KNOWN_VID}:{KNOWN_PID} 2>/dev/null")
        usb_present = rc == 0
        usb_stable = usb_present and self.state.get("usb_stable", True)

        serial_ports = glob.glob("/dev/ttyUSB*") + glob.glob("/dev/ttyACM*")
        net_ifaces = [
            iface_dir.split("/")[4]
            for iface_dir in glob.glob("/sys/class/net/*/device/driver")
            if os.path.basename(os.readlink(iface_dir))
            in ("rndis_host", "cdc_ether", "cdc_ncm", "qmi_wwan", "cdc_mbim")
        ]

        # Determine overall status
        if not usb_present:
            if self.state.get("was_connected"):
                overall = "DEVICE DISCONNECTED"
            else:
                overall = "DEVICE NOT DETECTED"
        elif not serial_ports and not net_ifaces:
            overall = "DEVICE DETECTED - NO INTERFACES"
        elif not serial_active and not net_active:
            overall = "DEVICE DETECTED - INTERFACES PRESENT - NO DATA"
        elif serial_active and not net_active:
            overall = "DEVICE DETECTED - SERIAL DATA FLOWING"
        elif net_active:
            if self.state.get("obd_detected"):
                overall = "DEVICE DETECTED - POSSIBLE VEHICLE TELEMETRY"
            else:
                overall = "DEVICE DETECTED - NETWORK DATA FLOWING"
        else:
            overall = "DEVICE DETECTED - SERIAL DATA FLOWING"

        if usb_present:
            self.state["was_connected"] = True

        # Net counters
        rx_inc = any(
            v.get("rx_increasing") for v in self.state.get("net_counters", {}).values()
        )
        tx_inc = any(
            v.get("tx_increasing") for v in self.state.get("net_counters", {}).values()
        )

        # Modem
        mmcli_out, _, mmrc = run("mmcli -L 2>/dev/null")
        modem_detected = mmrc == 0 and "Modem" in mmcli_out

        return {
            "timestamp": ts(),
            "usb_detected": usb_present,
            "usb_stable": usb_stable,
            "serial_interfaces": serial_ports,
            "network_interfaces": net_ifaces,
            "modem_detected": modem_detected,
            "gps_data": self.state.get("gps_detected", False),
            "obd_telemetry": self.state.get("obd_detected", False),
            "serial_rx_activity": serial_active,
            "network_rx_activity": rx_inc,
            "network_tx_activity": tx_inc,
            "overall": overall,
            "session_dir": str(self.log.session_dir),
        }

    def format_status(self, d):
        return textwrap.dedent(f"""\
            === JCR1440 HEALTH ===
            Timestamp: {d['timestamp']}

            USB detected: {'YES' if d['usb_detected'] else 'NO'}
            USB stable: {'YES' if d['usb_stable'] else 'NO'}
            Serial interfaces: {' '.join(d['serial_interfaces']) or 'none'}
            Network interface: {' '.join(d['network_interfaces']) or 'none'}
            ModemManager detected: {'YES' if d['modem_detected'] else 'NO'}
            GPS data: {'YES' if d['gps_data'] else 'NO'}
            OBD telemetry: {'YES' if d['obd_telemetry'] else 'UNKNOWN'}

            Serial RX activity: {'YES' if d['serial_rx_activity'] else 'NO'}
            Network RX activity: {'YES' if d['network_rx_activity'] else 'NO'}
            Network TX activity: {'YES' if d['network_tx_activity'] else 'NO'}

            Overall status:
            {d['overall']}
        """)

    def _run(self):
        while not self._stop.is_set():
            d = self.get_status_dict()
            report = self.format_status(d)

            self.log.log("summary", report, timestamp=False)
            self.log.log("data-flow", report, timestamp=False)

            # Write state file for jcr1440-status command
            try:
                STATE_FILE.write_text(json.dumps(d, indent=2))
            except Exception:
                pass

            self._stop.wait(HEALTH_INTERVAL)


# ---------------------------------------------------------------------------
# Main monitor
# ---------------------------------------------------------------------------

class JCR1440Monitor:
    def __init__(self):
        self._stop = threading.Event()
        self._components = []

        # Shared mutable state
        self.state = {
            "serial_bytes": {},
            "serial_formats": {},
            "serial_events": [],
            "net_counters": {},
            "net_data_flowing": False,
            "gps_detected": False,
            "obd_detected": False,
            "usb_active": False,
            "usb_stable": True,
            "was_connected": False,
            "last_serial_data": 0,
            "last_net_data": 0,
        }

    def run(self):
        # Create session directory
        session_name = datetime.datetime.now().strftime("%Y-%m-%d_%H-%M-%S")
        session_dir = LOG_ROOT / session_name
        self.logger = SessionLogger(session_dir)

        self.logger.log("summary", f"Session started: {session_dir}")
        self.logger.log("summary", f"Monitor PID: {os.getpid()}")

        # Write PID file
        try:
            PID_FILE.write_text(str(os.getpid()))
        except Exception:
            pass

        # Phase 1: Device detection
        detector = DeviceDetector(self.logger)
        device_found = detector.run_initial()

        # Phase 2: Interface detection
        iface_detector = InterfaceDetector(self.logger)
        iface_detector.run_initial()

        # Phase 3: udev monitor
        udev_mon = UdevMonitor(self.logger, self.state)
        udev_mon.start()
        self._components.append(udev_mon)

        # Phase 4: Serial monitoring
        serial_mon = SerialMonitor(self.logger, self.state)
        if iface_detector.serial_ports:
            serial_mon.start(iface_detector.serial_ports)
        self._components.append(serial_mon)

        # Phase 5: Modem detection
        modem = ModemDetector(self.logger)
        modem.run()

        # Phase 6: Network monitoring
        net_mon = NetworkMonitor(self.logger, self.state)
        if iface_detector.net_interfaces:
            net_mon.start(iface_detector.net_interfaces)
        self._components.append(net_mon)

        # Phase 7: USB activity
        usb_mon = USBActivityMonitor(self.logger, self.state)
        sysfs = detector.usb_info.get("sysfs_path")
        if sysfs:
            usb_mon.start(sysfs)
        self._components.append(usb_mon)

        # Phase 8 & 9: Telemetry analysis
        telemetry = TelemetryAnalyzer(self.logger, self.state)
        telemetry.start()
        self._components.append(telemetry)

        # Phase 10: Health reporter
        health = HealthReporter(self.logger, self.state, detector, iface_detector)
        health.start()
        self._components.append(health)
        self.health = health

        self.logger.log("summary", "All monitors started. Observing...")

        # Wait for stop signal
        try:
            while not self._stop.is_set():
                self._stop.wait(1)

                # Re-check for new serial ports that appeared after boot
                current_ports = set(
                    glob.glob("/dev/ttyUSB*") + glob.glob("/dev/ttyACM*"))
                known_ports = set(iface_detector.serial_ports)
                new_ports = current_ports - known_ports
                if new_ports:
                    self.logger.log("serial",
                                    f"New serial ports detected: {new_ports}")
                    serial_mon.start(list(new_ports))
                    iface_detector.serial_ports.extend(new_ports)

                # Re-check for new network interfaces
                current_net = []
                for idir in glob.glob("/sys/class/net/*/device/driver"):
                    drv = os.path.basename(os.readlink(idir))
                    iface = idir.split("/")[4]
                    if drv in ("rndis_host", "cdc_ether", "cdc_ncm",
                               "qmi_wwan", "cdc_mbim"):
                        current_net.append((iface, drv))

                known_net = set(i[0] for i in iface_detector.net_interfaces)
                new_net = [(i, d) for i, d in current_net if i not in known_net]
                if new_net:
                    self.logger.log("network",
                                    f"New network interfaces: {new_net}")
                    net_mon2 = NetworkMonitor(self.logger, self.state)
                    net_mon2.start(new_net)
                    self._components.append(net_mon2)
                    iface_detector.net_interfaces.extend(new_net)

        except KeyboardInterrupt:
            pass

        self.shutdown()

    def shutdown(self):
        self.logger.log("summary", "Monitor shutting down...")
        self._stop.set()
        for c in self._components:
            c.stop()
        time.sleep(1)

        # Final health report
        if hasattr(self, "health"):
            d = self.health.get_status_dict()
            self.logger.log("summary", "=== FINAL STATUS ===")
            self.logger.log("summary", self.health.format_status(d),
                            timestamp=False)

        self.logger.close()

        # Clean PID file
        try:
            PID_FILE.unlink(missing_ok=True)
        except Exception:
            pass

    def stop(self):
        self._stop.set()


def main():
    LOG_ROOT.mkdir(parents=True, exist_ok=True)

    monitor = JCR1440Monitor()

    def handle_signal(signum, frame):
        monitor.stop()

    signal.signal(signal.SIGTERM, handle_signal)
    signal.signal(signal.SIGINT, handle_signal)

    monitor.run()


if __name__ == "__main__":
    main()
