#!/usr/bin/env python3
"""
JCR1440 WiFi Configuration Tool

Reads and writes WiFi settings on the JCR1440 via the web API.

The WiFi password is sent as plaintext — no MD5 hashing is needed (unlike
the login challenge-response). The Boa web server stores and returns it
in the clear via mark_set_wifi.w.xml.

Endpoints:
  Read:  GET  /mark_set_wifi.w.xml       → current WiFi config
  Write: POST /wxml/setting_wifi.xml     → apply new WiFi config

Usage:
    ./jcr1440-wifi-config.py show                   # show current config
    ./jcr1440-wifi-config.py set --password NEW      # change password only
    ./jcr1440-wifi-config.py set --ssid NEW          # change SSID only
    ./jcr1440-wifi-config.py set --ssid X --password Y --channel 6
    ./jcr1440-wifi-config.py set --wifi off          # disable WiFi
    ./jcr1440-wifi-config.py set --wifi on           # enable WiFi
"""

import argparse
import hashlib
import json
import re
import subprocess
import sys
import xml.etree.ElementTree as ET
from pathlib import Path
from urllib.parse import quote as urlquote

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------
DEVICE_IP = "192.168.1.1"
HOST_HEADER = "jiocarfi.local.html"
INTERFACE = "enxfcde56ff0106"

DEFAULT_USER = "administrator"
DEFAULT_PASS = "administrator"

# Encryption type mappings (from the set_wifi.html JS)
ENC_LABELS = {
    "0":    "Open (no encryption)",
    "1,0":  "WEP-64",
    "1,1":  "WEP-128",
    "2,0":  "WPA-PSK/TKIP",
    "2,1":  "WPA-PSK/AES",
    "2,2":  "WPA-PSK/TKIP+AES",
    "3,0":  "WPA2-PSK/TKIP",
    "3,1":  "WPA2-PSK/AES",
    "3,2":  "WPA2-PSK/TKIP+AES",
    "4,0":  "WPA/WPA2-PSK/TKIP",
    "4,1":  "WPA/WPA2-PSK/AES",
    "4,2":  "WPA/WPA2-PSK/TKIP+AES",
    "4,3":  "WPA/WPA2-PSK/auto",
}

BGN_LABELS = {
    "1":  "802.11b only",
    "3":  "802.11b/g",
    "11": "802.11b/g/n",
    "8":  "802.11n only",
}


# ---------------------------------------------------------------------------
# JCR1440 API client (curl-based, interface-bound)
# ---------------------------------------------------------------------------
class JCR1440:

    def __init__(self, ip, interface, username, password):
        self.base = f"http://{ip}"
        self.interface = interface
        self.username = username
        self.password = password
        self.cookie_jar = Path("/tmp/jcr1440_wificfg_cookies.txt")
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
        if method == "POST" and data is not None:
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
        if not rand:
            raise RuntimeError("Failed to get rand nonce")
        body = self._curl("/login.htm")
        m = re.search(r'id="csrf_token2"[^>]*value="([^"]+)"', body)
        if not m:
            raise RuntimeError("Failed to extract CSRF token")
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

    def _refresh_csrf(self, page="/set_wifi.html"):
        body = self._curl(page)
        if "login.htm" in body and len(body) < 300:
            self.login()
            body = self._curl(page)
        m = re.search(r'id="csrf_token2"[^>]*value="([^"]+)"', body)
        if m:
            self.csrf_token = m.group(1)
        else:
            raise RuntimeError("Could not extract CSRF token from settings page")

    def fetch_xml(self, endpoint):
        body = self._curl(endpoint)
        if "login.htm" in body and len(body) < 300:
            self.login()
            body = self._curl(endpoint)
        return ET.fromstring(body)

    def get_wifi_config(self):
        """Read current WiFi configuration."""
        root = self.fetch_xml("/mark_set_wifi.w.xml")
        wifi = root.find("wifi")
        if wifi is None:
            raise RuntimeError("No <wifi> element in mark_set_wifi.w.xml")
        return {
            "ssid": wifi.findtext("ssid", ""),
            "key": wifi.findtext("key", ""),
            "enc_type": wifi.findtext("enc_type", ""),
            "bgn": wifi.findtext("bgn", ""),
            "wmm": wifi.findtext("wmm", ""),
            "power": wifi.findtext("power", ""),
            "channel": wifi.findtext("ch", ""),
            "broadcast_ssid": wifi.findtext("brd", ""),
            "wifi_on": wifi.findtext("ind", ""),   # 0=off, 1=on
            "max_clients": wifi.findtext("max", ""),
        }

    def set_wifi_config(self, ssid=None, key=None, channel=None,
                        enc_type=None, wifi_on=None, power=None):
        """Write WiFi configuration. Only specified fields are changed;
        others are kept at their current values.

        The key (password) is sent as PLAINTEXT — no hashing required.
        """
        # Read current config as baseline
        current = self.get_wifi_config()

        ssid = ssid if ssid is not None else current["ssid"]
        key = key if key is not None else current["key"]
        ch = channel if channel is not None else current["channel"]
        enc = enc_type if enc_type is not None else current["enc_type"]
        bgn = current["bgn"]
        wmm = current["wmm"]
        pwr = power if power is not None else current["power"]
        brd = current["broadcast_ssid"]

        # sw: 0=off, 1=on
        if wifi_on is not None:
            sw = "1" if wifi_on else "0"
        else:
            sw = current["wifi_on"]

        # Parse enc_type "3,2" → enc_type=3,2 and wpa=2
        if "," in enc:
            enc_main, wpa = enc.split(",", 1)
        else:
            enc_main, wpa = enc, "2"

        # Get fresh CSRF
        self._refresh_csrf()

        # Build POST body — key is URL-encoded but NOT hashed
        post_data = (
            f"ssid={urlquote(ssid)}"
            f"&ch={ch}"
            f"&enc_type={urlquote(enc)}"
            f"&wpa={wpa}"
            f"&bgn={bgn}"
            f"&wmm={wmm}"
            f"&power={pwr}"
            f"&key={urlquote(key)}"
            f"&brd={brd}"
            f"&sw={sw}"
            f"&restart=1"
        )

        body = self._curl(
            "/wxml/setting_wifi.xml", method="POST", data=post_data,
            extra_headers={"__RequestVerificationToken": self.csrf_token},
            timeout=15,
        )
        root = ET.fromstring(body)
        new_token = root.findtext("token")
        if new_token == "-1":
            raise RuntimeError("CSRF token rejected — session expired during write")
        if new_token:
            self.csrf_token = new_token

        # Check result fields — each returns 1 on success
        results = {}
        for field in ["ssid", "ch", "enc_type", "key", "brd", "bgn", "sw", "wmm", "power"]:
            val = root.findtext(field)
            if val:
                results[field] = val

        return results

    def logout(self):
        try:
            self._curl("/wxml/login_exit.xml", timeout=3)
        except Exception:
            pass
        self.cookie_jar.unlink(missing_ok=True)


# ---------------------------------------------------------------------------
# Display helpers
# ---------------------------------------------------------------------------
def show_config(cfg, file=None):
    out = file or sys.stdout
    enc = cfg["enc_type"]
    enc_label = ENC_LABELS.get(enc, enc)
    bgn_label = BGN_LABELS.get(cfg["bgn"], cfg["bgn"])
    status = "ON" if cfg["wifi_on"] == "1" else "OFF"

    print(f"  WiFi:         {status}", file=out)
    print(f"  SSID:         {cfg['ssid']}", file=out)
    print(f"  Password:     {cfg['key']}", file=out)
    print(f"  Security:     {enc_label} ({enc})", file=out)
    print(f"  WiFi Mode:    {bgn_label}", file=out)
    print(f"  Channel:      {'auto' if cfg['channel'] == '0' else cfg['channel']}", file=out)
    print(f"  TX Power:     {cfg['power']}%", file=out)
    print(f"  Broadcast:    {'yes' if cfg['broadcast_ssid'] == '1' else 'hidden'}", file=out)
    print(f"  WMM:          {'on' if cfg['wmm'] == '1' else 'off'}", file=out)
    print(f"  Max Clients:  {cfg['max_clients']}", file=out)


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
def main():
    parser = argparse.ArgumentParser(
        description="JCR1440 WiFi Configuration Tool",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=(
            "examples:\n"
            "  %(prog)s show\n"
            "  %(prog)s set --password 'MyNewPass123'\n"
            "  %(prog)s set --ssid 'MyHotspot' --password 'Secret'\n"
            "  %(prog)s set --wifi off\n"
            "  %(prog)s set --channel 6\n"
        ),
    )
    parser.add_argument("action", choices=["show", "set"],
                        help="'show' to read config, 'set' to change it")
    parser.add_argument("--ssid", help="New SSID (4-32 ASCII chars)")
    parser.add_argument("--password", help="New WiFi password (8-15 chars, plaintext)")
    parser.add_argument("--channel", help="WiFi channel (0=auto, 1-13)")
    parser.add_argument("--wifi", choices=["on", "off"], help="Enable/disable WiFi")
    parser.add_argument("--power", help="TX power percentage (e.g. 100, 75, 50, 25)")
    parser.add_argument("--json", action="store_true", help="Output as JSON")
    parser.add_argument("--ip", default=DEVICE_IP)
    parser.add_argument("--iface", default=INTERFACE)
    parser.add_argument("--user", default=DEFAULT_USER)
    parser.add_argument("--pass", dest="passwd", default=DEFAULT_PASS,
                        help="Login password (not WiFi password)")
    args = parser.parse_args()

    device = JCR1440(args.ip, args.iface, args.user, args.passwd)
    print("Connecting to JCR1440...", file=sys.stderr)
    device.login()
    print("Authenticated.", file=sys.stderr)

    try:
        if args.action == "show":
            cfg = device.get_wifi_config()
            if args.json:
                print(json.dumps(cfg, indent=2))
            else:
                print("Current WiFi configuration:")
                show_config(cfg)

        elif args.action == "set":
            if not any([args.ssid, args.password, args.channel, args.wifi, args.power]):
                parser.error("set requires at least one of: --ssid, --password, --channel, --wifi, --power")

            # Validate password length
            if args.password is not None:
                if len(args.password) < 8 or len(args.password) > 15:
                    parser.error("WiFi password must be 8-15 characters")

            # Validate SSID
            if args.ssid is not None:
                if len(args.ssid) < 4 or len(args.ssid) > 32:
                    parser.error("SSID must be 4-32 characters")

            print("Before:", file=sys.stderr)
            before = device.get_wifi_config()
            show_config(before, file=sys.stderr)

            wifi_on = None
            if args.wifi == "on":
                wifi_on = True
            elif args.wifi == "off":
                wifi_on = False

            print("\nApplying changes (WiFi module will restart)...", file=sys.stderr)
            result = device.set_wifi_config(
                ssid=args.ssid,
                key=args.password,
                channel=args.channel,
                wifi_on=wifi_on,
                power=args.power,
            )
            print(f"Write result: {result}", file=sys.stderr)

            # Wait for WiFi restart, then show new config
            import time
            time.sleep(3)

            print("\nAfter:", file=sys.stderr)
            after = device.get_wifi_config()
            show_config(after, file=sys.stderr)

            # Summary of what changed
            changes = []
            if before["ssid"] != after["ssid"]:
                changes.append(f"SSID: {before['ssid']} → {after['ssid']}")
            if before["key"] != after["key"]:
                changes.append(f"Password: {before['key']} → {after['key']}")
            if before["channel"] != after["channel"]:
                changes.append(f"Channel: {before['channel']} → {after['channel']}")
            if before["wifi_on"] != after["wifi_on"]:
                changes.append(f"WiFi: {'OFF' if before['wifi_on']=='1' else 'ON'} → {'ON' if after['wifi_on']=='1' else 'OFF'}")
            if before["power"] != after["power"]:
                changes.append(f"Power: {before['power']}% → {after['power']}%")

            if changes:
                print(f"\nChanges applied:", file=sys.stderr)
                for c in changes:
                    print(f"  ✓ {c}", file=sys.stderr)
            else:
                print("\nNo changes detected (values may have been identical).", file=sys.stderr)

    finally:
        device.logout()


if __name__ == "__main__":
    main()
