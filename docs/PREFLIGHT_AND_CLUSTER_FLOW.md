# Saarthi — Preflight & Cluster Flow

## Boot Sequence

```
Raspberry Pi Boot
       │
       ├── jcr1440-telemetry.service (monitor daemon)
       │
       └── [optional] saarthi-ui.service
                │
                ▼
       start-saarthi launcher
                │
                ▼
       Preflight Screen
                │
       ┌────────┴────────────────────────────┐
       │  ✓ Pi System        READY           │
       │  ✓ JCR1440 USB      CONNECTED       │
       │  ✓ OBD Interface    CONNECTED       │
       │  ✓ Vehicle Data     RECEIVING       │
       │  ⚠ GPS              NO FIX          │
       │  ✓ Network          CONNECTED       │
       │  ✓ Telemetry        RUNNING         │
       │  ✓ Storage          HEALTHY         │
       │                                     │
       │  READY WITH WARNINGS                │
       │                                     │
       │         [ START CLUSTER ]           │
       └─────────────────────────────────────┘
                │
       User presses START
                │
                ▼
       Fullscreen Racing Cluster
                │
       Ctrl+Alt+Q → back to Preflight
```

## Preflight Checks

### Required Systems

| Check | Source | Failure = blocks start? |
|---|---|---|
| Raspberry Pi System | CPU, RAM, temp, disk | No (warning only) |
| JCR1440 USB Device | lsusb, sysfs VID:PID 05c6:f00e | Yes if failed after 10s |
| OBD Interface | Serial/RNDIS detection + telemetry | Yes if no interface |
| Vehicle Data Stream | Live TelemetryFrame from poller | Yes if no data after 15s |

### Optional Systems

| Check | Source | Notes |
|---|---|---|
| GPS | Fix status from TelemetryFrame | Warning if no fix |
| Network | ip addr, nmcli | Warning if offline; cluster works offline |
| Telemetry Service | systemctl is-active jcr1440-telemetry | Info only |
| Storage | df, /var/log/jcr1440 | Warning if >85% full |

### Overall States

| State | Meaning |
|---|---|
| CHECKING | Initial checks in progress |
| READY | All systems operational |
| READY WITH WARNINGS | Required OK, optional have warnings |
| NOT READY | Required system failed |

## Preflight UI

- Dark automotive theme (1024x600 target)
- Large touch-friendly check rows
- Tap any check row for detailed info
- Live updates from telemetry poller
- Color-coded status + text icons (no color-only indicators)

### Detail Panel

Tap a check to see expanded info:

```
Vehicle Data Stream
─────────────────────
Source          JCR1440
Packets         1,284
RPM             812
Speed           0 km/h
Battery         13.8 V
Coolant         85 °C
Throttle        12%
```

## Start Cluster Button

- Appears after initial checks complete (~5-15 seconds)
- Disabled during CHECKING state
- Green when READY or READY WITH WARNINGS
- Gray when NOT READY (but still clickable for override)
- Keyboard: Enter key also activates

## Fullscreen Cluster Mode

- Full-screen rendering at 60fps
- Exponential smoothing on all gauge values
- Data loss banner: "VEHICLE DATA LOST — RECONNECTING..."
- Data restored banner: "VEHICLE DATA RESTORED" (3s flash)
- Return to preflight: **Ctrl+Alt+Q**

### Data Recovery

If telemetry disconnects while cluster is running:
1. Top banner shows "VEHICLE DATA LOST — RECONNECTING..."
2. Gauges smoothly decay to zero
3. Poller attempts reconnection with exponential backoff
4. When data returns: "VEHICLE DATA RESTORED" banner for 3s
5. Gauges smoothly resume from live values

The cluster never crashes or closes on disconnect.

## API Contract

The preflight reads from the telemetry monitor state file:

```
/run/jcr1440-monitor.state
```

JSON format:

```json
{
  "timestamp": "2026-09-19 16:50:00",
  "usb_detected": true,
  "usb_stable": true,
  "serial_interfaces": ["/dev/ttyUSB0"],
  "network_interfaces": ["enxfcde56ff0106"],
  "modem_detected": false,
  "gps_data": true,
  "obd_telemetry": false,
  "serial_rx_activity": true,
  "network_rx_activity": true,
  "network_tx_activity": true,
  "overall": "DEVICE DETECTED - NETWORK DATA FLOWING",
  "session_dir": "/var/log/jcr1440/2026-09-19_16-45-10"
}
```

Additionally, the preflight receives live data from the same `watch::Receiver<DeviceState>` channel that the cluster uses, so it shows real-time telemetry status without polling.

## Desktop Launcher

```
~/.local/share/applications/saarthi-cluster.desktop
~/Desktop/Saarthi Cluster.desktop
```

Launches: `/opt/saarthi/bin/start-saarthi`

The launcher:
1. Verifies cluster binary exists
2. Starts telemetry monitor if not running
3. Sets display environment variables
4. Launches cluster with preflight
5. Logs session to `/var/log/saarthi/`

## Service Dependencies

```
jcr1440-telemetry.service  (monitor daemon, always-on)
     │
     └── saarthi-ui.service  (optional auto-start)
              │
              └── start-saarthi script
                       │
                       └── jcr1440-cluster binary
```

## Files

| Path | Purpose |
|---|---|
| `/opt/jcr1440/jcr1440-cluster` | Cluster binary |
| `/opt/saarthi/bin/start-saarthi` | Launch script |
| `/etc/systemd/system/saarthi-ui.service` | Auto-start service |
| `~/.local/share/applications/saarthi-cluster.desktop` | App menu entry |
| `/var/log/saarthi/` | Launch session logs |
| `crates/jcr1440-cluster/src/preflight.rs` | Preflight UI code |
| `crates/jcr1440-cluster/src/main.rs` | App with screen switching |
| `crates/jcr1440-cluster/src/gauges.rs` | Racing cluster rendering |

## Commands

```bash
# Launch manually (with preflight):
/opt/saarthi/bin/start-saarthi

# Launch directly (skip preflight):
/opt/jcr1440/jcr1440-cluster --skip-preflight --fullscreen

# Launch in mock mode (no device needed):
/opt/jcr1440/jcr1440-cluster --mock

# Enable auto-start on boot:
sudo systemctl enable saarthi-ui

# Disable auto-start:
sudo systemctl disable saarthi-ui
```

## Developer Mode

- **Ctrl+Alt+D**: Toggle developer panel (from preflight)
- **Ctrl+Alt+Q**: Return from cluster to preflight
- **--mock**: Use synthetic telemetry data
- **--skip-preflight**: Go directly to cluster
