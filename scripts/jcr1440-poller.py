#!/usr/bin/env python3
"""
JCR1440 GPS + OBD Poller

Authenticates to the JCR1440 web API and polls GPS/OBD/LTE data at a
configurable interval. Outputs JSON lines to stdout and optionally to a
log file.

Usage:
    ./jcr1440-poller.py                     # defaults: 10s interval, stdout only
    ./jcr1440-poller.py -i 5                # poll every 5 seconds
    ./jcr1440-poller.py -o data.jsonl       # also append to file
    ./jcr1440-poller.py --once              # single poll, then exit
    ./jcr1440-poller.py --csv -o data.csv   # CSV output mode
"""

import argparse
import hashlib
import json
import sys
import time
import xml.etree.ElementTree as ET
from datetime import datetime, timezone
from pathlib import Path

import re
import subprocess

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------
DEVICE_IP = "192.168.1.1"
HOST_HEADER = "jiocarfi.local.html"
INTERFACE = "enxfcde56ff0106"  # RNDIS interface — override with --iface

DEFAULT_USER = "administrator"
DEFAULT_PASS = "administrator"

ENDPOINTS = {
    "gps": "/st_gps.w.xml",      # GPS + OBD (auth required)
    "dev": "/st_dev.w.xml",       # device info (no auth)
    "lte": "/st_lte.w.xml",      # LTE signal (no auth)
    "wan": "/st_wan.w.xml",       # WAN status (auth required)
    "per": "/st_per.w.xml",       # performance (auth required)
    "lan": "/st_lan.w.xml",       # LAN clients (auth required)
    "bt":  "/st_bt.w.xml",        # bluetooth (no auth)
    "fota": "/st_fota.w.xml",     # firmware update (auth required)
}

# GPS data field order inside the semicolon-delimited gps_data string
GPS_FIELDS = [
    "speed", "longitude", "latitude", "altitude", "heading",
    "satellites", "hdop", "vdop", "pdop", "accuracy",
    "timestamp_ms", "fix_mode",
]

# OBD fields from st_gps.w.xml (value is "-" when car is off / not connected)
OBD_FIELDS = [
    "vin", "am_air_temp", "celv", "dtwma", "ddtc", "ect", "efr", "eot",
    "erpm", "ert", "fl", "iat", "pmsmc", "tses", "tsdtcc", "tert", "toff",
    "vspeed", "mils", "dtc", "maf", "capab", "atp", "imap", "dr",
]

# Human-readable OBD field names
OBD_LABELS = {
    "vin": "VIN",
    "am_air_temp": "ambient_air_temp",
    "celv": "battery_voltage",
    "dtwma": "distance_traveled_with_mil",
    "ddtc": "distance_since_dtc_cleared",
    "ect": "coolant_temp",
    "efr": "engine_fuel_rate",
    "eot": "engine_oil_temp",
    "erpm": "engine_rpm",
    "ert": "engine_run_time",
    "fl": "fuel_level",
    "iat": "intake_air_temp",
    "pmsmc": "pid_monitor_status",
    "tses": "time_since_engine_start",
    "tsdtcc": "time_since_dtc_cleared",
    "tert": "total_engine_run_time",
    "toff": "time_off",
    "vspeed": "vehicle_speed",
    "mils": "mil_status",
    "dtc": "dtc_codes",
    "maf": "mass_airflow",
    "capab": "obd_capability",
    "atp": "absolute_throttle_pos",
    "imap": "intake_manifold_pressure",
    "dr": "driving_range",
}


# ---------------------------------------------------------------------------
# Session management
# ---------------------------------------------------------------------------
class JCR1440Session:
    """Manages authentication and HTTP session with the JCR1440.

    Uses curl --interface under the hood to guarantee the request goes
    out via the RNDIS interface, avoiding the subnet collision with
    the home router (both are 192.168.1.1).
    """

    def __init__(self, ip, interface, username, password, **_):
        self.base = f"http://{ip}"
        self.ip = ip
        self.interface = interface
        self.username = username
        self.password = password
        self.cookie_jar = Path("/tmp/jcr1440_poller_cookies.txt")
        self.authenticated = False

    def _curl(self, path, method="GET", data=None, extra_headers=None, timeout=5):
        """Run a curl command bound to the RNDIS interface. Returns body text."""
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
            cmd += ["-d", "&".join(f"{k}={v}" for k, v in data.items())]
        cmd.append(url)
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout + 5)
        if result.returncode != 0:
            raise RuntimeError(f"curl failed (exit {result.returncode}): {result.stderr}")
        return result.stdout

    def login(self):
        """Authenticate using the MD5 challenge-response flow."""
        # Clear old cookies
        self.cookie_jar.unlink(missing_ok=True)

        # Step 1: get rand nonce
        body = self._curl("/mark_lang.w.xml")
        root = ET.fromstring(body)
        rand = root.findtext("rand")
        if not rand:
            raise RuntimeError("Failed to get rand nonce from mark_lang.w.xml")

        # Step 2: get CSRF token from login.htm
        body = self._curl("/login.htm")
        m = re.search(r'id="csrf_token2"[^>]*value="([^"]+)"', body)
        if not m:
            raise RuntimeError("Failed to extract CSRF token from login.htm")
        csrf = m.group(1)

        # Step 3: compute MD5(rand + password)
        pass_hash = hashlib.md5((rand + self.password).encode()).hexdigest()

        # Step 4: POST login
        body = self._curl(
            "/wxml/post_login.xml",
            method="POST",
            data={"Name": self.username, "password": pass_hash, "rand": rand},
            extra_headers={"__RequestVerificationToken": csrf},
        )
        root = ET.fromstring(body)
        check = root.findtext("login_check")
        if check != "3":
            codes = {"0": "bad username+password", "1": "bad password",
                     "2": "bad username", "4": "timeout", "5": "hacking detected"}
            raise RuntimeError(f"Login failed: {codes.get(check, f'unknown code {check}')}")

        self.authenticated = True

    def fetch_xml(self, endpoint):
        """Fetch an XML endpoint and return parsed ElementTree root."""
        body = self._curl(endpoint)
        # Check for login redirect
        if "login.htm" in body and len(body) < 300:
            self.login()
            body = self._curl(endpoint)
        return ET.fromstring(body)

    def logout(self):
        """Clean logout."""
        try:
            self._curl("/wxml/login_exit.xml", timeout=3)
        except Exception:
            pass
        self.cookie_jar.unlink(missing_ok=True)


# ---------------------------------------------------------------------------
# Data parsing
# ---------------------------------------------------------------------------
def parse_gps_data(raw):
    """Parse the semicolon-delimited gps_data string into a dict."""
    parts = raw.split(";")
    result = {}
    for i, field in enumerate(GPS_FIELDS):
        if i >= len(parts):
            break
        val = parts[i]
        # Most fields are "fix,value" pairs
        if "," in val:
            fix_str, value_str = val.split(",", 1)
            result[f"{field}_fix"] = int(fix_str)
        else:
            value_str = val
        try:
            if field == "timestamp_ms":
                result[field] = int(value_str)
                ts_sec = int(value_str) / 1000
                result["time_utc"] = datetime.fromtimestamp(
                    ts_sec, tz=timezone.utc
                ).isoformat()
            elif field in ("satellites", "fix_mode"):
                result[field] = int(value_str)
            else:
                result[field] = float(value_str)
        except (ValueError, OSError):
            result[field] = value_str
    return result


def parse_obd_fields(root):
    """Extract OBD fields from the GPS XML root."""
    obd = {}
    for field in OBD_FIELDS:
        el = root.findtext(field)
        if el and el.strip() != "-":
            label = OBD_LABELS.get(field, field)
            obd[label] = el.strip()
    return obd


def parse_lte(root):
    """Parse LTE status XML."""
    return {
        "carrier": root.findtext("cspn"),
        "attached": root.findtext("attach") == "1",
        "home_plmn": root.findtext("hplmn") == "1",
        "uptime": root.findtext("time"),
        "freq_info": root.findtext("freq_info"),
        "cell_info": root.findtext("cell_info"),
        "apn": root.findtext("apn"),
        "rssi_info": root.findtext("rssi_info"),
    }


def parse_dev(root):
    """Parse device info XML."""
    return {
        "model": root.findtext("model"),
        "serial": root.findtext("serial"),
        "firmware": root.findtext("swver"),
        "vci_version": root.findtext("vciver"),
        "imei": root.findtext("imei"),
        "iccid": root.findtext("iccid"),
        "bands": root.findtext("freq"),
    }


# ---------------------------------------------------------------------------
# Polling loop
# ---------------------------------------------------------------------------
def poll_once(sess, endpoints_to_poll):
    """Single poll cycle. Returns a dict with all collected data."""
    record = {"poll_time": datetime.now(timezone.utc).isoformat()}

    # GPS + OBD (requires auth)
    if "gps" in endpoints_to_poll:
        try:
            root = sess.fetch_xml(ENDPOINTS["gps"])
            gps_raw = root.findtext("gps_data")
            if gps_raw and "login.htm" not in gps_raw:
                record["gps"] = parse_gps_data(gps_raw)
                obd = parse_obd_fields(root)
                if obd:
                    record["obd"] = obd
        except Exception as e:
            record["gps_error"] = str(e)

    # LTE (no auth needed, but fetch anyway)
    if "lte" in endpoints_to_poll:
        try:
            root = sess.fetch_xml(ENDPOINTS["lte"])
            record["lte"] = parse_lte(root)
        except Exception as e:
            record["lte_error"] = str(e)

    # Device info (no auth needed, rarely changes — poll less often)
    if "dev" in endpoints_to_poll:
        try:
            root = sess.fetch_xml(ENDPOINTS["dev"])
            record["device"] = parse_dev(root)
        except Exception as e:
            record["dev_error"] = str(e)

    return record


def record_to_csv_row(record):
    """Flatten a poll record into a CSV row."""
    gps = record.get("gps", {})
    obd = record.get("obd", {})
    lte = record.get("lte", {})
    return [
        record.get("poll_time", ""),
        gps.get("time_utc", ""),
        gps.get("latitude", ""),
        gps.get("longitude", ""),
        gps.get("altitude", ""),
        gps.get("speed", ""),
        gps.get("heading", ""),
        gps.get("satellites", ""),
        gps.get("accuracy", ""),
        gps.get("hdop", ""),
        obd.get("engine_rpm", ""),
        obd.get("vehicle_speed", ""),
        obd.get("coolant_temp", ""),
        obd.get("fuel_level", ""),
        obd.get("battery_voltage", ""),
        obd.get("dtc_codes", ""),
        lte.get("rssi_info", ""),
        lte.get("carrier", ""),
    ]


CSV_HEADER = [
    "poll_time", "gps_time", "latitude", "longitude", "altitude",
    "speed_kmh", "heading", "satellites", "accuracy_m", "hdop",
    "engine_rpm", "vehicle_speed", "coolant_temp", "fuel_level",
    "battery_voltage", "dtc_codes", "lte_rssi", "carrier",
]


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
def main():
    parser = argparse.ArgumentParser(description="JCR1440 GPS/OBD Poller")
    parser.add_argument("-i", "--interval", type=int, default=10,
                        help="Poll interval in seconds (default: 10)")
    parser.add_argument("-o", "--output", type=str,
                        help="Output file (JSONL or CSV, appended)")
    parser.add_argument("--once", action="store_true",
                        help="Single poll then exit")
    parser.add_argument("--csv", action="store_true",
                        help="Output CSV instead of JSONL")
    parser.add_argument("--ip", default=DEVICE_IP,
                        help=f"Device IP (default: {DEVICE_IP})")
    parser.add_argument("--iface", default=INTERFACE,
                        help=f"Network interface (default: {INTERFACE})")
    parser.add_argument("--user", default=DEFAULT_USER,
                        help="Login username")
    parser.add_argument("--password", default=DEFAULT_PASS,
                        help="Login password")
    parser.add_argument("--endpoints", default="gps,lte",
                        help="Comma-separated endpoints to poll (default: gps,lte)")
    parser.add_argument("-q", "--quiet", action="store_true",
                        help="Suppress stdout output (only write to file)")
    args = parser.parse_args()

    endpoints = set(args.endpoints.split(","))

    # Add dev info on first poll only
    first_poll_endpoints = endpoints | {"dev"}

    sess = JCR1440Session(args.ip, args.iface, args.user, args.password)

    print(f"Connecting to JCR1440 at {args.ip}...", file=sys.stderr)
    sess.login()
    print("Authenticated.", file=sys.stderr)

    outfile = None
    if args.output:
        outpath = Path(args.output)
        is_new = not outpath.exists() or outpath.stat().st_size == 0
        outfile = open(outpath, "a")
        if args.csv and is_new:
            outfile.write(",".join(CSV_HEADER) + "\n")
            outfile.flush()

    poll_count = 0
    try:
        while True:
            ep = first_poll_endpoints if poll_count == 0 else endpoints
            record = poll_once(sess, ep)
            poll_count += 1

            if args.csv:
                line = ",".join(str(v) for v in record_to_csv_row(record))
            else:
                line = json.dumps(record, separators=(",", ":"))

            if not args.quiet:
                print(line, flush=True)
            if outfile:
                outfile.write(line + "\n")
                outfile.flush()

            if args.once:
                break

            time.sleep(args.interval)

    except KeyboardInterrupt:
        print(f"\nStopped after {poll_count} polls.", file=sys.stderr)
    finally:
        sess.logout()
        if outfile:
            outfile.close()


if __name__ == "__main__":
    main()
