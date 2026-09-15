# JioMotive JCR1440 — Reverse Engineering & Repurposing

Open-source tools for repurposing the JioMotive JCR1440 OBD-II telematics
dongle as a self-hosted GPS tracker, vehicle dashboard, and WiFi hotspot
manager.

**Nino's Tech** · [oyenino.com](https://www.oyenino.com/)

## What is the JCR1440?

The JCR1440 is a 4G LTE OBD-II telematics dongle sold by Reliance Jio
(manufactured by Qingdao Haier). It plugs into a vehicle's OBD-II port
and provides:

- Real-time GPS/AGPS tracking
- OBD-II vehicle diagnostics (CAN ISO 15765-4)
- LTE Cat-4 connectivity (Jio eSIM)
- WiFi hotspot (802.11 b/g/n, up to 8 clients)
- Bluetooth 4.0
- USB RNDIS Ethernet for direct PC/Pi connection

## Hardware Architecture

```
┌─────────────────────────────────────────────────────┐
│  JCR1440 PCB                                        │
│                                                     │
│  ┌──────────────────────┐  ┌──────────────────────┐ │
│  │ Qualcomm MDM9x07     │  │ NXP i.MX RT1051      │ │
│  │ (Main SoC)           │  │ (Vehicle MCU)         │ │
│  │                      │  │                       │ │
│  │ - Embedded Linux     │  │ - CAN controller      │ │
│  │ - LTE Cat-4 modem    │  │ - OBD-II protocols    │ │
│  │ - WiFi 802.11b/g/n   │  │ - ISO 15765-4         │ │
│  │ - Bluetooth 4.0      │  │ - Cortex-M7 @ 600MHz  │ │
│  │ - GPS/AGPS           │  │ - VCI firmware         │ │
│  │ - Boa web server     │  │                       │ │
│  │ - USB RNDIS          │  │                       │ │
│  └───────┬──────────────┘  └───────┬───────────────┘ │
│          └───── Internal bus ──────┘                  │
│                (UART/SPI)                             │
│  ┌──────────┐  ┌──────────┐  ┌──────────────────┐   │
│  │ Micro USB│  │ OBD-II   │  │ 5F super cap     │   │
│  │ (RNDIS)  │  │ connector│  │ (backup power)   │   │
│  └──────────┘  └──────────┘  └──────────────────┘   │
└─────────────────────────────────────────────────────┘
```

## USB Identity

| Field        | Value                        |
|--------------|------------------------------|
| USB VID      | 0x05C6 (Qualcomm, Inc.)      |
| USB PID      | 0xF00E (RNDIS mode)          |
| USB Speed    | High Speed (480 Mbps)        |
| Power        | 500 mA (bus powered)         |
| Linux Driver | rndis_host                   |
| Web Server   | Boa/0.94.14rc21              |

## Authentication

The device uses an MD5 challenge-response login:

1. `GET /mark_lang.w.xml` → extract `<rand>` nonce
2. `GET /login.htm` → extract CSRF token from hidden `csrf_token2` input
3. Compute `MD5(rand + password)` as hex digest
4. `POST /wxml/post_login.xml` with `Name`, `password` (hash), `rand`,
   plus `__RequestVerificationToken` header
5. Response sets `SessionID` cookie; `<login_check>3</login_check>` = success

Default credentials: `administrator` / `administrator`

The WiFi password is sent as **plaintext** (not hashed) when configuring
WiFi via `/wxml/setting_wifi.xml`.

## API Endpoints

### Status endpoints (GET, `st_*.w.xml`)

| Endpoint | Auth | Content |
|---|---|---|
| `/st_dev.w.xml` | No | Device info: model, firmware version, bands |
| `/st_lte.w.xml` | No | LTE status: carrier, signal strength, cell info |
| `/st_bt.w.xml` | No | Bluetooth status |
| `/st_gps.w.xml` | Yes | GPS + all OBD-II PIDs in one response |
| `/st_per.w.xml` | Yes | Performance: CPU%, memory, TX/RX rates |
| `/st_wan.w.xml` | Yes | WAN: IP, gateway, DNS, traffic counters |
| `/st_lan.w.xml` | Yes | LAN: connected clients, WiFi state |
| `/st_fota.w.xml` | Yes | FOTA update status |

### Config-read endpoints (GET, `mark_*.w.xml`)

| Endpoint | Content |
|---|---|
| `/mark_lang.w.xml` | Language + auth rand nonce |
| `/mark_title.w.xml` | UI title/branding |
| `/mark_set_wifi.w.xml` | WiFi config: SSID, password, channel, encryption |
| `/mark_network.w.xml` | LAN IP, subnet, MTU, MAC filter list |
| `/mark_set_net.w.xml` | APN config, DNS mode |
| `/mark_set_sd.w.xml` | SD card status, USB status |
| `/mark_set_nat.w.xml` | NAT timeouts |
| `/mark_conn.w.xml` | Connection state, traffic counters |

### Write endpoints (POST, `/wxml/*.xml`)

| Endpoint | Auth | Purpose |
|---|---|---|
| `/wxml/post_login.xml` | — | Login |
| `/wxml/login_exit.xml` | Yes | Logout |
| `/wxml/setting_wifi.xml` | Yes | WiFi config (plaintext password) |
| `/wxml/set_mac.xml` | Yes | MAC address filter list |
| `/wxml/set_mac_apply.xml` | Yes | Commit MAC filter changes |
| `/wxml/set_bt.xml` | Yes | Bluetooth settings |
| `/wxml/set_net.xml` | Yes | Network/APN settings |
| `/wxml/set_fota.xml` | Yes | FOTA settings |
| `/wxml/set_nat.xml` | Yes | NAT timeouts |
| `/wxml/set_sms.xml` | Yes | SMS settings |
| `/wxml/set_sys.xml` | Yes | System settings |
| `/wxml/clear_ts.xml` | Yes | Clear traffic statistics |
| `/wxml/reboot.xml` | Eng | Reboot (requires engineering auth) |
| `/wxml/eng_login.xml` | Yes | Engineering login (password-only) |

## OBD-II Fields

All vehicle data comes from a single endpoint (`/st_gps.w.xml`).
GPS data is in a semicolon-delimited `gps_data` string; OBD fields
are separate XML elements that return `-` when the device isn't
connected to a running vehicle.

### GPS fields (from `gps_data` string)

| Index | Field | Format | Example |
|---|---|---|---|
| 0 | speed | fix,km/h | `1,65` |
| 1 | longitude | fix,degrees | `1,72.877` |
| 2 | latitude | fix,degrees | `1,19.076` |
| 3 | altitude | fix,meters | `1,215.0` |
| 4 | heading | fix,degrees | `1,90.0` |
| 5 | satellites | fix,count | `1,12` |
| 6 | hdop | float | `1.2` |
| 7 | vdop | float | `0.9` |
| 8 | pdop | float | `1.5` |
| 9 | accuracy | float_m | `8.0` |
| 10 | timestamp | fix,epoch_ms | `1,1700000000000` |
| 11 | fix_mode | fix,mode | `1,3` (2=2D, 3=3D) |

### OBD-II fields (XML elements)

| XML Tag | Label | Unit |
|---|---|---|
| `erpm` | Engine RPM | rpm |
| `vspeed` | Vehicle Speed | km/h |
| `ect` | Engine Coolant Temperature | °C |
| `iat` | Intake Air Temperature | °C |
| `celv` | Battery Voltage | V |
| `atp` | Absolute Throttle Position | % |
| `maf` | Mass Air Flow | g/s |
| `fl` | Fuel Level | % |
| `efr` | Engine Fuel Rate | L/h |
| `eot` | Engine Oil Temperature | °C |
| `imap` | Intake Manifold Pressure | kPa |
| `am_air_temp` | Ambient Air Temperature | °C |
| `mils` | MIL Status (check engine) | 0/1 |
| `dtc` | DTC Codes | string |
| `vin` | VIN | string |
| `ert` | Engine Run Time | s |
| `tses` | Time Since Engine Start | s |
| `dr` | Driving Range | km |

**Not present**: engine load, fuel pressure, fuel rail pressure,
timing advance, barometric pressure, catalyst temp, O2 sensors, EGR,
turbo boost.

## Port Reality

Only **port 80** is open on the JCR1440 itself. If you see ports 53
or 443 responding at `192.168.1.1`, that's your home router on the
same subnet — not this device. Use `curl --interface <rndis_iface>`
to avoid the collision.

## USB Disconnect Behavior

The device periodically disconnects when powered only via USB (no 12V
from OBD-II port). This is a **hardware brownout**, not a software
timeout — the 5F super capacitor can't sustain LTE TX bursts on
USB-only power indefinitely. Fix: always use it plugged into the
car's OBD-II port, or use a powered USB hub (>1A).

## Engineering Login

An engineering login exists at `/wxml/eng_login.xml` (password-only,
same MD5 challenge-response). The reboot endpoint requires engineering
auth. The password is not the same as the main admin login and is
likely device-specific or carrier-provisioned. A UART/firmware dump
would be needed to extract it.

## Project Structure

```
JioMotive/
├── crates/
│   ├── jcr1440-client/      # Async Rust client (tokio + reqwest)
│   │   ├── src/lib.rs        # Auth, polling loop, mock data
│   │   └── src/parse.rs      # XML parsing, TelemetryFrame struct
│   └── jcr1440-cluster/      # Racing instrument cluster (egui/glow)
│       ├── src/main.rs        # App, smoothed gauges, CLI
│       └── src/gauges.rs      # All gauge rendering (Haltech-inspired)
├── scripts/
│   ├── jcr1440-poller.py      # Python GPS/OBD poller
│   ├── jcr1440-hotspot.py     # WiFi client manager with timed access
│   ├── jcr1440-wifi-config.py # WiFi settings read/write tool
│   ├── jcr1440-usb-monitor.py # USB disconnect investigator
│   └── jcr1440-diagnostic.sh  # Full diagnostic collection script
├── deploy/
│   ├── pi/setup.sh            # One-command Pi provisioning
│   ├── pi/bench-test.sh       # Pre-deployment validation
│   └── jcr1440-cluster.service
└── docs/jcr1440/
    ├── README.md               # This file
    ├── obd-can-analysis.md     # CAN bus architecture
    ├── pi-cluster-setup.md     # Pi deployment guide
    └── repurposing-modes.md    # Hotspot + tracker use cases
```

## Quick Start

```bash
# Instrument cluster with mock data (no device needed)
cargo run -p jcr1440-cluster --release -- --mock

# Instrument cluster with real device
cargo run -p jcr1440-cluster --release -- --fullscreen --ip 192.168.1.1

# Python poller
python3 scripts/jcr1440-poller.py --once

# Pi setup (run once, then plug-and-play)
sudo ./deploy/pi/setup.sh
sudo ./deploy/pi/bench-test.sh
```

## License

This is an independent research and repurposing project for a device
the author legally owns. The project does not distribute any Jio/Haier
firmware, copyrighted software, or proprietary data. All tools
communicate with the device via its standard HTTP API.
