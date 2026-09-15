#!/usr/bin/env python3
"""
JCR1440 USB Disconnect Investigator

Polls st_per.w.xml, st_dev.w.xml, and st_wan.w.xml at high frequency
to capture state changes leading up to a USB disconnect. Logs every
field change to help determine whether disconnects are:
  (a) software idle timeout / session management
  (b) hardware brownout from modem TX bursts on USB-only power

Also probes the engineering login endpoint (eng_login.xml) with a short
list of common default passwords for this modem family.

Usage:
    ./jcr1440-usb-monitor.py                 # monitor + eng_login probe
    ./jcr1440-usb-monitor.py --monitor-only  # skip eng_login probing
    ./jcr1440-usb-monitor.py --eng-only      # only probe eng_login
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

# Endpoints to monitor for pre-disconnect state changes
MONITOR_ENDPOINTS = {
    "per": "/st_per.w.xml",     # CPU, memory, TX/RX rates
    "dev": "/st_dev.w.xml",     # device status, battery
    "wan": "/st_wan.w.xml",     # WAN state, traffic counters
    "lte": "/st_lte.w.xml",    # signal strength, attach state
    "lan": "/st_lan.w.xml",    # connected clients, WiFi state
    "fota": "/st_fota.w.xml",  # firmware update state
}

# Engineering login password candidates — common defaults for
# MDM9x07 / Qualcomm-based hotspot platforms and Haier ODMs
ENG_PASSWORDS = [
    "administrator",      # same as main login
    "admin",
    "admin123",
    "password",
    "12345678",
    "1234567890",
    "87654321",
    "haier",
    "haier123",
    "Haier@123",
    "qingdao",
    "engineer",
    "Engineer",
    "engineer123",
    "jio",
    "jio123",
    "Jio@1234",
    "jcr1440",
    "JCR1440",
    "qualcomm",
    "root",
    "test",
    "default",
    "mobile",
    "MobileRouter",
    "9x07",
    "",                   # empty password
]


# ---------------------------------------------------------------------------
# JCR1440 API client
# ---------------------------------------------------------------------------
class JCR1440:

    def __init__(self, ip, interface, username, password):
        self.base = f"http://{ip}"
        self.interface = interface
        self.username = username
        self.password = password
        self.cookie_jar = Path("/tmp/jcr1440_usbmon_cookies.txt")
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

    def _refresh_csrf(self, page="/to_engineer_login.html"):
        body = self._curl(page)
        if "login.htm" in body and len(body) < 300:
            self.login()
            body = self._curl(page)
        m = re.search(r'id="csrf_token2"[^>]*value="([^"]+)"', body)
        if m:
            self.csrf_token = m.group(1)
        return body

    def fetch_xml(self, endpoint):
        body = self._curl(endpoint)
        if "login.htm" in body and len(body) < 300:
            self.login()
            body = self._curl(endpoint)
        return ET.fromstring(body)

    def fetch_xml_raw(self, endpoint):
        """Return both parsed XML and raw text."""
        body = self._curl(endpoint)
        if "login.htm" in body and len(body) < 300:
            self.login()
            body = self._curl(endpoint)
        return ET.fromstring(body), body

    def try_eng_login(self, password):
        """Attempt engineering login. Returns (success: bool, response_text)."""
        # Get fresh rand + CSRF
        body = self._curl("/mark_lang.w.xml")
        rand = ET.fromstring(body).findtext("rand")

        self._refresh_csrf("/to_engineer_login.html")

        # The eng_login JS shows: password = hex_md5(rand + password)
        # Same MD5 challenge-response as main login
        pass_hash = hashlib.md5((rand + password).encode()).hexdigest()

        body = self._curl(
            "/wxml/eng_login.xml", method="POST",
            data=f"password={pass_hash}",
            extra_headers={"__RequestVerificationToken": self.csrf_token},
        )

        try:
            root = ET.fromstring(body)
            eng_result = root.findtext("eng")
            token = root.findtext("token")
            return eng_result, body
        except ET.ParseError:
            return None, body

    def logout(self):
        try:
            self._curl("/wxml/login_exit.xml", timeout=3)
        except Exception:
            pass
        self.cookie_jar.unlink(missing_ok=True)


# ---------------------------------------------------------------------------
# XML diff helper
# ---------------------------------------------------------------------------
def xml_to_dict(root, prefix=""):
    """Flatten an XML tree into a dict of dotted-path keys."""
    result = {}
    for child in root:
        key = f"{prefix}{child.tag}" if prefix else child.tag
        if len(child) > 0:
            result.update(xml_to_dict(child, key + "."))
        else:
            result[key] = (child.text or "").strip()
    return result


def diff_dicts(old, new):
    """Return dict of changed keys with (old_val, new_val) tuples."""
    changes = {}
    all_keys = set(old.keys()) | set(new.keys())
    for k in all_keys:
        old_val = old.get(k)
        new_val = new.get(k)
        if old_val != new_val:
            changes[k] = (old_val, new_val)
    return changes


# ---------------------------------------------------------------------------
# USB disconnect monitor
# ---------------------------------------------------------------------------
def monitor_loop(device, interval, outdir):
    """Poll all endpoints at high frequency, log every field change."""
    outfile = open(outdir / "usb_monitor_log.jsonl", "a")

    # Baseline: fetch all endpoints once
    baselines = {}
    for name, ep in MONITOR_ENDPOINTS.items():
        try:
            root = device.fetch_xml(ep)
            baselines[name] = xml_to_dict(root)
        except Exception as e:
            print(f"  [WARN] Could not fetch {ep}: {e}", file=sys.stderr)

    print(f"Baseline captured for: {', '.join(baselines.keys())}", file=sys.stderr)
    print(f"Polling every {interval}s — watching for field changes...", file=sys.stderr)
    print(f"Log: {outfile.name}", file=sys.stderr)

    poll_count = 0
    try:
        while True:
            time.sleep(interval)
            poll_count += 1
            now = datetime.now(timezone.utc).isoformat()

            for name, ep in MONITOR_ENDPOINTS.items():
                try:
                    root = device.fetch_xml(ep)
                    current = xml_to_dict(root)
                except Exception as e:
                    record = {"time": now, "poll": poll_count,
                              "event": "fetch_error", "endpoint": name, "error": str(e)}
                    line = json.dumps(record, separators=(",", ":"))
                    print(line, flush=True)
                    outfile.write(line + "\n")
                    outfile.flush()

                    # If this is a connection error, the USB might have disconnected
                    if "curl failed" in str(e) or "No such device" in str(e):
                        record = {"time": now, "poll": poll_count,
                                  "event": "USB_DISCONNECT_DETECTED",
                                  "last_baselines": {k: baselines.get(k, {}) for k in baselines}}
                        line = json.dumps(record, separators=(",", ":"))
                        print(line, flush=True)
                        outfile.write(line + "\n")
                        outfile.flush()
                        print("\n*** USB DISCONNECT DETECTED ***", file=sys.stderr)
                        print(f"Last known state saved to log.", file=sys.stderr)
                    continue

                if name in baselines:
                    changes = diff_dicts(baselines[name], current)
                    if changes:
                        record = {
                            "time": now,
                            "poll": poll_count,
                            "event": "field_change",
                            "endpoint": name,
                            "changes": {k: {"from": v[0], "to": v[1]} for k, v in changes.items()},
                        }
                        line = json.dumps(record, separators=(",", ":"))
                        print(line, flush=True)
                        outfile.write(line + "\n")
                        outfile.flush()

                baselines[name] = current

    except KeyboardInterrupt:
        print(f"\nStopped after {poll_count} polls.", file=sys.stderr)
    finally:
        outfile.close()


# ---------------------------------------------------------------------------
# Engineering login probe
# ---------------------------------------------------------------------------
def probe_eng_login(device, outdir):
    """Try a short list of common engineering passwords."""
    outfile = open(outdir / "eng_login_probe.txt", "w")

    print("Engineering login probe — /wxml/eng_login.xml", file=sys.stderr)
    print(f"Testing {len(ENG_PASSWORDS)} candidate passwords...", file=sys.stderr)
    outfile.write(f"Engineering Login Probe — {datetime.now().isoformat()}\n")
    outfile.write(f"Endpoint: /wxml/eng_login.xml (password-only, MD5 challenge-response)\n")
    outfile.write(f"Candidates: {len(ENG_PASSWORDS)}\n")
    outfile.write("=" * 60 + "\n\n")

    found = False
    for i, pw in enumerate(ENG_PASSWORDS, 1):
        display_pw = repr(pw) if pw else "(empty)"
        try:
            result, raw = device.try_eng_login(pw)
            status = f"eng={result}"
            if result and result != "0":
                status += " *** POSSIBLE SUCCESS ***"
                found = True
        except Exception as e:
            result = None
            raw = str(e)
            status = f"ERROR: {e}"

        line = f"  [{i:2d}/{len(ENG_PASSWORDS)}] {display_pw:25s} → {status}"
        print(line, file=sys.stderr)
        outfile.write(line + "\n")
        outfile.write(f"         Raw: {raw.strip()[:200]}\n\n")

        if found:
            print(f"\n*** Engineering password found: {display_pw} ***", file=sys.stderr)
            outfile.write(f"\n*** SUCCESS: password = {display_pw} ***\n")
            break

        # Small delay to avoid triggering "hacking detected" lockout
        time.sleep(1)

    if not found:
        msg = "No engineering password found in candidate list."
        print(f"\n{msg}", file=sys.stderr)
        outfile.write(f"\n{msg}\n")

    outfile.write(f"\nNotes:\n")
    outfile.write(f"  - eng=0 means password incorrect\n")
    outfile.write(f"  - eng=1 (or non-zero) means success\n")
    outfile.write(f"  - The password is hashed as MD5(rand + password), same as main login\n")
    outfile.write(f"  - 'hacking detected' (login_check=5) may trigger after too many failures\n")
    outfile.close()
    print(f"Results saved to: {outfile.name}", file=sys.stderr)
    return found


# ---------------------------------------------------------------------------
# Endpoint enumeration
# ---------------------------------------------------------------------------
def enumerate_endpoints(device, outdir):
    """Discover additional wxml endpoints not yet documented."""
    print("Enumerating wxml endpoints...", file=sys.stderr)

    # Known read endpoints (st_*.w.xml pattern)
    known_read = [
        "st_dev", "st_lte", "st_bt", "st_gps", "st_per",
        "st_wan", "st_lan", "st_fota",
    ]
    # Known mark (config-read) endpoints
    known_mark = [
        "mark_lang", "mark_title", "mark_set_wifi", "mark_network",
        "mark_set_adv",
    ]
    # Known write endpoints
    known_write = [
        "setting_wifi", "set_mac", "set_mac_apply", "post_login",
        "login_exit", "login_reset", "eng_login", "set_adv",
        "set_pin",
    ]

    # Candidates to probe — common patterns on Qualcomm/Haier hotspot platforms
    read_candidates = [
        "st_sys", "st_usb", "st_pwr", "st_power", "st_batt", "st_battery",
        "st_temp", "st_thermal", "st_acc", "st_accel", "st_gyro",
        "st_can", "st_obd", "st_obd2", "st_trip", "st_wifi",
        "st_sms", "st_phonebook", "st_call", "st_sim",
        "st_storage", "st_sd", "st_log", "st_debug",
        "st_conn", "st_data", "st_nat", "st_upnp", "st_firewall",
        "st_secu", "st_security", "st_user", "st_admin",
        "st_voice", "st_ota", "st_update", "st_diag",
    ]
    mark_candidates = [
        "mark_set_lte", "mark_set_bt", "mark_set_dev", "mark_set_net",
        "mark_set_fota", "mark_set_sd", "mark_set_user",
        "mark_set_nat", "mark_set_upnp", "mark_set_secu",
        "mark_conn", "mark_status", "mark_sys",
    ]
    write_candidates = [
        "set_wifi", "set_lte", "set_bt", "set_dev", "set_net",
        "set_fota", "set_sd", "set_user", "set_nat", "set_upnp",
        "set_secu", "set_sms", "set_conn", "set_password",
        "set_power", "set_reboot", "reboot", "reset",
        "set_gps", "set_usb", "set_sleep", "set_wps",
        "clear_ts", "set_sys", "get_log",
    ]

    results = {"new_read": [], "new_mark": [], "new_write": []}
    outfile = open(outdir / "endpoint_enumeration.txt", "w")
    outfile.write(f"Endpoint Enumeration — {datetime.now().isoformat()}\n")
    outfile.write("=" * 60 + "\n\n")

    # Probe read endpoints: /st_*.w.xml
    outfile.write("--- Read endpoints (st_*.w.xml) ---\n")
    for name in read_candidates:
        if name in known_read:
            continue
        ep = f"/{name}.w.xml"
        try:
            body = device._curl(ep, timeout=3)
            if "404 Not Found" not in body and "login.htm" not in body[:300]:
                results["new_read"].append((name, ep, len(body)))
                outfile.write(f"  FOUND: {ep} ({len(body)} bytes)\n")
                # Save the content
                with open(outdir / f"discovered_{name}.xml", "w") as f:
                    f.write(body)
            elif "login.htm" in body[:300] and len(body) < 300:
                results["new_read"].append((name, ep, "auth_required"))
                outfile.write(f"  FOUND: {ep} (auth required)\n")
        except Exception:
            pass

    # Probe mark endpoints: /mark_*.w.xml
    outfile.write("\n--- Config-read endpoints (mark_*.w.xml) ---\n")
    for name in mark_candidates:
        if name in known_mark:
            continue
        ep = f"/{name}.w.xml"
        try:
            body = device._curl(ep, timeout=3)
            if "404 Not Found" not in body and len(body) > 50:
                is_redirect = "login.htm" in body[:300] and len(body) < 300
                if not is_redirect:
                    results["new_mark"].append((name, ep, len(body)))
                    outfile.write(f"  FOUND: {ep} ({len(body)} bytes)\n")
                    with open(outdir / f"discovered_{name}.xml", "w") as f:
                        f.write(body)
                else:
                    results["new_mark"].append((name, ep, "auth_required"))
                    outfile.write(f"  FOUND: {ep} (auth required)\n")
        except Exception:
            pass

    # Probe write endpoints: /wxml/*.xml
    outfile.write("\n--- Write endpoints (/wxml/*.xml) ---\n")
    for name in write_candidates:
        if name in known_write:
            continue
        ep = f"/wxml/{name}.xml"
        try:
            # GET probe only — do NOT POST to unknown endpoints
            body = device._curl(ep, timeout=3)
            if "404 Not Found" not in body:
                results["new_write"].append((name, ep, len(body)))
                outfile.write(f"  FOUND: {ep} ({len(body)} bytes) [GET only — not called with POST]\n")
        except Exception:
            pass

    # Summary
    total_new = sum(len(v) for v in results.values())
    outfile.write(f"\n--- Summary ---\n")
    outfile.write(f"New read endpoints:  {len(results['new_read'])}\n")
    outfile.write(f"New mark endpoints:  {len(results['new_mark'])}\n")
    outfile.write(f"New write endpoints: {len(results['new_write'])}\n")
    outfile.write(f"Total new:           {total_new}\n")
    outfile.close()

    print(f"Found {total_new} new endpoints.", file=sys.stderr)
    for category, items in results.items():
        for item in items:
            print(f"  {category}: {item[1]} ({item[2]})", file=sys.stderr)

    return results


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
def main():
    parser = argparse.ArgumentParser(description="JCR1440 USB Disconnect Investigator")
    parser.add_argument("-i", "--interval", type=int, default=3,
                        help="Monitor poll interval in seconds (default: 3)")
    parser.add_argument("--monitor-only", action="store_true",
                        help="Only run the disconnect monitor, skip probes")
    parser.add_argument("--eng-only", action="store_true",
                        help="Only probe engineering login")
    parser.add_argument("--enum-only", action="store_true",
                        help="Only enumerate endpoints")
    parser.add_argument("--ip", default=DEVICE_IP)
    parser.add_argument("--iface", default=INTERFACE)
    parser.add_argument("--user", default=DEFAULT_USER)
    parser.add_argument("--password", default=DEFAULT_PASS)
    args = parser.parse_args()

    # Output directory
    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    outdir = Path(f"diagnostics/jcr1440/{timestamp}")
    outdir.mkdir(parents=True, exist_ok=True)

    device = JCR1440(args.ip, args.iface, args.user, args.password)
    print("Connecting to JCR1440...", file=sys.stderr)
    device.login()
    print("Authenticated.", file=sys.stderr)

    try:
        if args.eng_only:
            probe_eng_login(device, outdir)
            return

        if args.enum_only:
            enumerate_endpoints(device, outdir)
            return

        # Default: run endpoint enumeration, then eng_login probe, then monitor
        if not args.monitor_only:
            print("\n=== Phase 1: Endpoint Enumeration ===", file=sys.stderr)
            enumerate_endpoints(device, outdir)

            print("\n=== Phase 2: Engineering Login Probe ===", file=sys.stderr)
            probe_eng_login(device, outdir)

        print("\n=== Phase 3: USB Disconnect Monitor ===", file=sys.stderr)
        monitor_loop(device, args.interval, outdir)

    finally:
        device.logout()
        print(f"\nAll output saved to: {outdir}", file=sys.stderr)


if __name__ == "__main__":
    main()
