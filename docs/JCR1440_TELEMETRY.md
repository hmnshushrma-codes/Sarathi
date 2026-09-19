# JCR1440 Telemetry & Diagnostic Monitor

## Overview

A boot-time observational service that monitors the JCR1440 OBD-II dongle
connected to a Raspberry Pi over USB. It records device presence, exposed
interfaces, data flow, and any evidence of GPS/OBD telemetry — all without
modifying the device.

## Architecture

```
Raspberry Pi Boot
       │
       ▼
systemd starts jcr1440-telemetry.service
       │
       ▼
monitor.py launches
       │
       ├── Phase 1:  USB detection (lsusb, sysfs, dmesg)
       ├── Phase 2:  Interface enumeration (serial, network, block, modem)
       ├── Phase 3:  udev event monitor (connect/disconnect/mode changes)
       ├── Phase 4:  Serial port monitoring (read-only, per-port threads)
       ├── Phase 5:  ModemManager detection (read-only mmcli)
       ├── Phase 6:  Network counter tracking (RX/TX every 5s)
       ├── Phase 7:  USB activity observation (URB counters via sysfs)
       ├── Phase 8:  GPS/NMEA detection (from serial data)
       ├── Phase 9:  OBD/CAN evidence detection (pattern matching)
       └── Phase 10: Health status (summary every 30s)
```

## Service Startup

The systemd service starts after:
- Local filesystems are available
- USB subsystem is settled
- NetworkManager is wanted (but not required)

```ini
[Unit]
After=local-fs.target usb.mount systemd-udev-settle.service
Wants=NetworkManager.service
```

It restarts on failure with a 5-second delay, max 5 restarts per minute.

## What Is Monitored

| Category | Method | Invasive? |
|---|---|---|
| USB presence | lsusb, sysfs | No |
| USB descriptors | lsusb -v | No |
| Kernel logs | dmesg, journalctl -k | No |
| Serial ports | /dev/ttyUSB*, udevadm | No |
| Serial data | Read-only open, no writes | No |
| Network interfaces | ip, sysfs, nmcli | No |
| Network counters | /sys/class/net/*/statistics | No |
| Modem | mmcli -L, mmcli -m (read-only) | No |
| USB activity | sysfs urbnum counter | No |
| udev events | udevadm monitor | No |

**The monitor NEVER writes to the device, sends AT commands, transmits
CAN frames, modifies firmware, or changes modem/SIM configuration.**

## Log Structure

Each boot creates a new timestamped session:

```
/var/log/jcr1440/
├── 2026-09-19_16-45-10/
│   ├── summary.log          # Overall status and health reports
│   ├── usb.log              # USB detection, descriptors, sysfs
│   ├── kernel.log           # dmesg, filtered kernel messages
│   ├── serial.log           # Serial port detection and activity
│   ├── network.log          # Network interfaces, counters
│   ├── modem.log            # ModemManager detection
│   ├── adb.log              # ADB device check
│   ├── storage.log          # Block device detection
│   ├── obd.log              # OBD/CAN evidence
│   ├── data-flow.log        # Data flow and health reports
│   ├── system.log           # System info (kernel, model, uptime)
│   ├── serial_ttyUSB0_capture.bin   # Raw serial capture (≤10MB)
│   └── serial_ttyUSB0_ascii.log     # ASCII-decoded serial data
```

Old sessions are never overwritten.

## Redaction

The following are automatically masked in all logs:

- IMEI → `***REDACTED***`
- ICCID → `***REDACTED***`
- IMSI → `***REDACTED***`
- Phone numbers → `***REDACTED***`
- Passwords → `***REDACTED***`
- Tokens → `***REDACTED***`

The `create-bundle.sh` script applies an additional redaction pass before
packaging logs for sharing.

## Commands

| Command | Description |
|---|---|
| `jcr1440-status` | Print current device and monitor status |
| `jcr1440-logs` | Print path to the latest log session |
| `jcr1440-tail` | Tail the live summary.log |
| `sudo /opt/jcr1440-monitor/create-bundle.sh` | Package logs for analysis |

## Health Status

Every 30 seconds, the monitor writes a health summary. Possible states:

| State | Meaning |
|---|---|
| DEVICE NOT DETECTED | USB device not found |
| DEVICE DETECTED - NO INTERFACES | USB present but no serial/network |
| DEVICE DETECTED - INTERFACES PRESENT - NO DATA | Interfaces exist but no data flow |
| DEVICE DETECTED - SERIAL DATA FLOWING | Bytes being received on serial |
| DEVICE DETECTED - NETWORK DATA FLOWING | RX/TX counters increasing |
| DEVICE DETECTED - POSSIBLE VEHICLE TELEMETRY | OBD-like patterns detected |
| DEVICE DISCONNECTED | Was connected, now unplugged |

The state is written to `/run/jcr1440-monitor.state` as JSON for
`jcr1440-status` to read.

## Installation

```bash
cd /path/to/JioMotive/monitor
sudo ./install.sh
```

## Uninstallation

```bash
cd /path/to/JioMotive/monitor
sudo ./uninstall.sh
```

Logs are preserved at `/var/log/jcr1440/` after uninstall.

## Updating

After installing git on the Pi and cloning the repo:

```bash
cd /path/to/JioMotive/monitor
sudo ./update.sh              # update everything (monitor + cluster)
sudo ./update.sh --monitor-only  # update only the monitor
sudo ./update.sh --cluster-only  # rebuild only the cluster binary
```

The update script:
1. Pulls latest code from git
2. Copies updated monitor scripts to /opt/jcr1440-monitor/
3. Rebuilds the cluster binary (if Rust is installed)
4. Restarts affected services

## Troubleshooting

| Problem | Solution |
|---|---|
| Service not starting | `journalctl -u jcr1440-telemetry -f` |
| No logs created | Check `/var/log/jcr1440/` exists and is writable |
| Device not detected | Check `lsusb \| grep 05c6:f00e` |
| Serial ports not monitored | Ensure user has permission or run as root |
| State file missing | Service may not be running: `systemctl status jcr1440-telemetry` |

## Diagnostic Bundle

To create a shareable archive:

```bash
sudo /opt/jcr1440-monitor/create-bundle.sh
```

This creates `~/jcr1440-diagnostics-YYYY-MM-DD-HHMMSS.tar.gz` with:
- All logs from the current session
- Sensitive identifiers redacted
- Binary captures truncated to 1MB
- No credentials or full GPS tracks

## Test Procedure

1. Boot Pi **without** JCR1440 → wait 30s
2. Plug JCR1440 USB → wait 60s
3. Unplug JCR1440 → wait 10s
4. Replug JCR1440
5. Connect JCR1440 to vehicle OBD-II port
6. Turn ignition ON → wait 2-3 min
7. Turn ignition OFF

Review logs:
```bash
jcr1440-logs
jcr1440-tail
```

The summary.log should capture the entire sequence with timestamps.
