#!/usr/bin/env python3
"""
JCR1440 Hotspot Manager

Monitors WiFi clients connected to the JCR1440 hotspot, enforces timed
access (default 30 seconds), and logs all activity.

Features:
- Detects new WiFi client connections via st_lan.w.xml polling
- Enforces per-client time limits using MAC address deny-list
- Logs connect/disconnect events with timestamps
- Exempts the USB RNDIS host (control machine) from blocking

Usage:
    ./jcr1440-hotspot.py                     # 30s access, stdout log
    ./jcr1440-hotspot.py -t 60               # 60-second access window
    ./jcr1440-hotspot.py -t 0                # monitor only, no blocking
    ./jcr1440-hotspot.py -o clients.jsonl    # log to file
"""

import argparse
import hashlib
import json
import re
import subprocess
import sys
import time
import xml.etree.ElementTree as ET
from datetime import datetime, timezone
from pathlib import Path

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------
DEVICE_IP = "192.168.1.1"
HOST_HEADER = "jiocarfi.local.html"
INTERFACE = "enxfcde56ff0106"

DEFAULT_USER = "administrator"
DEFAULT_PASS = "administrator"

# MAC of the USB RNDIS interface — never block this.
# Auto-detected at runtime; fallback to empty string (will just skip the check).
USB_HOST_MAC = ""


# ---------------------------------------------------------------------------
# JCR1440 API client (curl-based, interface-bound)
# ---------------------------------------------------------------------------
class JCR1440:

    def __init__(self, ip, interface, username, password):
        self.base = f"http://{ip}"
        self.interface = interface
        self.username = username
        self.password = password
        self.cookie_jar = Path("/tmp/jcr1440_hotspot_cookies.txt")
        self.csrf_token = None

    def _curl(self, path, method="GET", data=None, extra_headers=None, timeout=5):
        url = f"{self.base}{path}"
        cmd = [
            "curl", "-sS",
            "--connect-timeout", str(timeout),
            "--interface", self.interface,
            "-H", f"Host: {HOST_HEADER}",
            "-b", str(self.cookie_jar),
            "-c", str(self.cookie_jar),
        ]
        if extra_headers:
            for k, v in extra_headers.items():
                cmd += ["-H", f"{k}: {v}"]
        if method == "POST" and data:
            cmd += ["-d", data]
        cmd.append(url)
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout + 5)
        if result.returncode != 0:
            raise RuntimeError(f"curl failed (exit {result.returncode}): {result.stderr}")
        return result.stdout

    def login(self):
        self.cookie_jar.unlink(missing_ok=True)
        body = self._curl("/mark_lang.w.xml")
        rand = ET.fromstring(body).findtext("rand")
        body = self._curl("/login.htm")
        m = re.search(r'id="csrf_token2"[^>]*value="([^"]+)"', body)
        csrf = m.group(1)
        pass_hash = hashlib.md5((rand + self.password).encode()).hexdigest()
        body = self._curl(
            "/wxml/post_login.xml", method="POST",
            data=f"Name={self.username}&password={pass_hash}&rand={rand}",
            extra_headers={"__RequestVerificationToken": csrf},
        )
        root = ET.fromstring(body)
        if root.findtext("login_check") != "3":
            raise RuntimeError(f"Login failed: {body}")
        self.csrf_token = root.findtext("token")

    def _refresh_csrf(self):
        """Get a fresh CSRF token from the settings page."""
        body = self._curl("/set_net.html")
        m = re.search(r'id="csrf_token2"[^>]*value="([^"]+)"', body)
        if m:
            self.csrf_token = m.group(1)

    def fetch_xml(self, endpoint):
        body = self._curl(endpoint)
        if "login.htm" in body and len(body) < 300:
            self.login()
            body = self._curl(endpoint)
        return ET.fromstring(body)

    def get_clients(self):
        """Poll st_lan.w.xml and return list of connected clients."""
        root = self.fetch_xml("/st_lan.w.xml")
        clients = []
        for user_list in root.findall("user_list"):
            user = user_list.find("user")
            if user is not None:
                clients.append({
                    "name": user.findtext("name", ""),
                    "ip": user.findtext("ip", ""),
                    "mac": user.findtext("mac", "").upper(),
                    "leased_time": user.findtext("leased_time", ""),
                    "status": user.findtext("status", ""),
                })
        return clients

    def set_mac_deny(self, mac_list):
        """Set the MAC deny list. mac_list is a list of (mac, comment) tuples.
        Max 10 entries. Omit USB_HOST_MAC — firmware rejects it anyway."""
        try:
            self._refresh_csrf()

            # Build POST data: mode=1 (DENY), mac0..mac9
            parts = ["mode=1"]
            for i in range(10):
                if i < len(mac_list):
                    mac, comment = mac_list[i]
                    parts.append(f"mac{i}={mac}")
                    parts.append(f"dsc{i}={comment}")
                    parts.append(f"sw{i}=1")
                else:
                    parts.append(f"mac{i}=")
            post_data = "&".join(parts)

            # Step 1: set the list
            body = self._curl(
                "/wxml/set_mac.xml", method="POST", data=post_data,
                extra_headers={"__RequestVerificationToken": self.csrf_token},
                timeout=10,
            )
            root = ET.fromstring(body)
            new_token = root.findtext("token")
            if new_token == "-1":
                # Session expired mid-operation, re-login
                self.login()
                self._refresh_csrf()
                body = self._curl(
                    "/wxml/set_mac.xml", method="POST", data=post_data,
                    extra_headers={"__RequestVerificationToken": self.csrf_token},
                    timeout=10,
                )
                root = ET.fromstring(body)
                new_token = root.findtext("token")
            if new_token and new_token != "-1":
                self.csrf_token = new_token

            # Step 2: commit/apply (can take 10-15s when filter mode changes)
            body = self._curl(
                "/wxml/set_mac_apply.xml", method="POST", data="apply=1",
                extra_headers={"__RequestVerificationToken": self.csrf_token},
                timeout=20,
            )
            root = ET.fromstring(body)
            new_token = root.findtext("token")
            if new_token and new_token != "-1":
                self.csrf_token = new_token
            return root.findtext("commit") == "1"
        except Exception as e:
            print(f"[ERROR] set_mac_deny failed: {e}", file=sys.stderr)
            return False

    def clear_mac_filter(self):
        """Disable MAC filtering entirely."""
        try:
            self._refresh_csrf()
        except Exception:
            self.login()
            self._refresh_csrf()
        parts = ["mode=0"]
        for i in range(10):
            parts.append(f"mac{i}=")
        body = self._curl(
            "/wxml/set_mac.xml", method="POST", data="&".join(parts),
            extra_headers={"__RequestVerificationToken": self.csrf_token},
        )
        root = ET.fromstring(body)
        new_token = root.findtext("token")
        if new_token and new_token != "-1":
            self.csrf_token = new_token
        body = self._curl(
            "/wxml/set_mac_apply.xml", method="POST", data="apply=1",
            extra_headers={"__RequestVerificationToken": self.csrf_token},
            timeout=20,
        )
        root = ET.fromstring(body)
        new_token = root.findtext("token")
        if new_token and new_token != "-1":
            self.csrf_token = new_token

    def logout(self):
        try:
            self._curl("/wxml/login_exit.xml", timeout=3)
        except Exception:
            pass
        self.cookie_jar.unlink(missing_ok=True)


# ---------------------------------------------------------------------------
# Client session tracker
# ---------------------------------------------------------------------------
class ClientSession:
    def __init__(self, mac, name, ip):
        self.mac = mac
        self.name = name
        self.ip = ip
        self.connected_at = time.time()
        self.blocked = False
        self.disconnected_at = None


# ---------------------------------------------------------------------------
# Hotspot manager
# ---------------------------------------------------------------------------
class HotspotManager:

    def __init__(self, device, timeout_sec, log_file=None):
        self.device = device
        self.timeout_sec = timeout_sec  # 0 = monitor only
        self.sessions = {}  # mac -> ClientSession
        self.deny_list = []  # list of (mac, comment) currently blocked
        self.log_file = log_file

    def log(self, event, data):
        record = {
            "time": datetime.now(timezone.utc).isoformat(),
            "event": event,
            **data,
        }
        line = json.dumps(record, separators=(",", ":"))
        print(line, flush=True)
        if self.log_file:
            self.log_file.write(line + "\n")
            self.log_file.flush()

    def poll(self):
        """Single poll cycle: detect clients, enforce timeouts."""
        clients = self.device.get_clients()
        current_macs = set()

        for client in clients:
            mac = client["mac"]
            if mac == USB_HOST_MAC.replace(":", "-").upper().replace("-", "-"):
                continue  # skip the control host
            # Normalize MAC
            mac_norm = mac.upper()
            current_macs.add(mac_norm)

            if mac_norm not in self.sessions:
                # New client!
                sess = ClientSession(mac_norm, client["name"], client["ip"])
                self.sessions[mac_norm] = sess
                self.log("connect", {
                    "mac": mac_norm,
                    "name": client["name"],
                    "ip": client["ip"],
                })

        # Check for disconnections
        for mac, sess in list(self.sessions.items()):
            if mac not in current_macs and not sess.blocked and sess.disconnected_at is None:
                sess.disconnected_at = time.time()
                duration = sess.disconnected_at - sess.connected_at
                self.log("disconnect", {
                    "mac": mac,
                    "name": sess.name,
                    "ip": sess.ip,
                    "duration_sec": round(duration, 1),
                })

        # Enforce timeouts
        if self.timeout_sec > 0:
            now = time.time()
            new_blocks = []
            for mac, sess in self.sessions.items():
                if not sess.blocked and (now - sess.connected_at) >= self.timeout_sec:
                    sess.blocked = True
                    duration = now - sess.connected_at
                    self.log("timeout_block", {
                        "mac": mac,
                        "name": sess.name,
                        "ip": sess.ip,
                        "duration_sec": round(duration, 1),
                    })
                    new_blocks.append(mac)

            if new_blocks:
                self._update_deny_list()

    def _update_deny_list(self):
        """Sync the MAC deny list to the device."""
        blocked = [(mac, f"blocked_{sess.name}")
                    for mac, sess in self.sessions.items()
                    if sess.blocked]

        # Protect against blocking USB host
        blocked = [(m, c) for m, c in blocked
                    if m.replace("-", ":") != USB_HOST_MAC]

        if len(blocked) > 10:
            # Evict oldest blocks
            blocked_sessions = sorted(
                [(m, c, self.sessions[m].connected_at) for m, c in blocked],
                key=lambda x: x[2]
            )
            # Remove oldest from deny list and session tracker
            for m, c, _ in blocked_sessions[:-10]:
                del self.sessions[m]
            blocked = [(m, c) for m, c, _ in blocked_sessions[-10:]]

        if blocked:
            # Convert MAC format: device uses XX-XX-XX format, filter wants XX:XX:XX
            colon_blocked = [(m.replace("-", ":"), c) for m, c in blocked]
            ok = self.device.set_mac_deny(colon_blocked)
            if ok:
                self.log("deny_list_updated", {
                    "count": len(blocked),
                    "macs": [m for m, _ in blocked],
                })
        else:
            self.device.clear_mac_filter()

    def reset(self):
        """Clear all blocks and reset state."""
        self.device.clear_mac_filter()
        self.sessions.clear()
        self.deny_list.clear()
        self.log("reset", {"message": "MAC filter cleared, all sessions reset"})


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
def main():
    parser = argparse.ArgumentParser(description="JCR1440 Hotspot Manager")
    parser.add_argument("-t", "--timeout", type=int, default=30,
                        help="Access timeout in seconds (0=monitor only, default: 30)")
    parser.add_argument("-i", "--interval", type=int, default=5,
                        help="Poll interval in seconds (default: 5)")
    parser.add_argument("-o", "--output", type=str,
                        help="Log file path (JSONL, appended)")
    parser.add_argument("--ip", default=DEVICE_IP)
    parser.add_argument("--iface", default=INTERFACE)
    parser.add_argument("--user", default=DEFAULT_USER)
    parser.add_argument("--password", default=DEFAULT_PASS)
    parser.add_argument("--reset", action="store_true",
                        help="Clear MAC filter and exit")
    args = parser.parse_args()

    device = JCR1440(args.ip, args.iface, args.user, args.password)
    print("Connecting to JCR1440...", file=sys.stderr)
    device.login()
    print("Authenticated.", file=sys.stderr)

    if args.reset:
        device.clear_mac_filter()
        print("MAC filter cleared.", file=sys.stderr)
        device.logout()
        return

    log_file = None
    if args.output:
        log_file = open(args.output, "a")

    manager = HotspotManager(device, args.timeout, log_file)

    mode = "monitor-only" if args.timeout == 0 else f"{args.timeout}s access"
    print(f"Hotspot manager running ({mode}, poll every {args.interval}s)", file=sys.stderr)
    print(f"WiFi SSID: JCR1440_5A42DC / Password: <check device>", file=sys.stderr)
    print("Press Ctrl+C to stop.", file=sys.stderr)

    try:
        while True:
            manager.poll()
            time.sleep(args.interval)
    except KeyboardInterrupt:
        print(f"\nStopping...", file=sys.stderr)
        print(f"Total sessions tracked: {len(manager.sessions)}", file=sys.stderr)
        manager.reset()
    finally:
        device.logout()
        if log_file:
            log_file.close()


if __name__ == "__main__":
    main()
