# JCR1440 Repurposing Modes

## Mode A: WiFi Hotspot with Client Logging & Timed Access

### What the device already provides
- WiFi 802.11bgn hotspot (SSID: `JCR1440_5A42DC`, currently `wifi=0` / disabled)
- DHCP server on 192.168.1.0/24
- NAT to Jio LTE (WAN IP via jionet APN)
- Connected client list via `/st_lan.w.xml`: hostname, IP, MAC, lease time, status
- Up to 7 client slots (based on user_list entries in LAN XML)
- Web UI at `set_wifi.html` for SSID, password, channel, encryption
- Max 7 simultaneous WiFi clients (typical for MDM9x07-based hotspots)

### Architecture

```
                        JCR1440 (192.168.1.1)
                    ┌──────────────────────────┐
Phone/Laptop ─WiFi──► 802.11bgn hotspot        │
                    │   DHCP → 192.168.1.x     │
                    │   NAT ──────────────► LTE ──► Internet (Jio)
                    │   Boa web server (:80)    │
                    └──────────┬───────────────┘
                               │ USB RNDIS
                               ▼
                    ┌──────────────────────────┐
                    │   Control Pi / Laptop     │
                    │                          │
                    │   hotspot-manager.py      │
                    │   ├─ polls st_lan.w.xml  │
                    │   ├─ logs clients (MAC,   │
                    │   │  hostname, connect/   │
                    │   │  disconnect times)    │
                    │   ├─ 30s timer per client │
                    │   └─ kicks via web UI?    │
                    │                          │
                    │   Optional: DNS proxy     │
                    │   (for URL logging)       │
                    └──────────────────────────┘
```

### What we CAN do (no shell access needed)

1. **Enable WiFi** — via `set_wifi.html` web UI (SSID, password, channel, encryption)
2. **Monitor clients** — poll `/st_lan.w.xml` every 5-10s to see who's connected
   - Each client: hostname, IP, MAC address, DHCP lease time, connection status
3. **Log connections** — record connect/disconnect events with timestamps
4. **Change WiFi password** — rotate password to effectively kick everyone

### What we CANNOT do (without shell access)

1. **Per-client kick** — no known API to disconnect a specific MAC address
2. **30-second timed access** — the device has no built-in captive portal or timer
3. **URL logging** — the device doesn't expose DNS query logs or HTTP proxy logs
4. **Bandwidth throttling** — no QoS controls exposed via web API
5. **Captive portal** — no redirect-to-login-page mechanism

### Workarounds for the gaps

#### Timed access (30-second sessions)
**Option 1 — Password rotation (crude but works):**
- Give out the current WiFi password
- After 30 seconds, change the password via the web API
- All clients get disconnected
- New password for the next user
- Downside: disconnects ALL clients, not just the one whose time expired

**Option 2 — External DNS gateway (proper solution):**
- Set JCR1440 DNS to point to your control Pi (192.168.1.100)
- Pi runs a DNS proxy (e.g. dnsmasq or dnsproxy)
- Pi controls which clients get real DNS resolution vs NXDOMAIN
- New client connects → Pi allows DNS for 30s → then blocks
- This gives per-client timed access AND URL logging for free
- Requires: changing the DHCP DNS setting on JCR1440 (if exposed in web UI)

**Option 3 — ARP spoofing (works but fragile):**
- Pi sends ARP replies to hijack the gateway for specific clients
- Allows per-client blocking after timeout
- Fragile, some devices have ARP spoofing protection

#### URL logging
**With DNS gateway (Option 2 above):**
- All DNS queries from WiFi clients go through your Pi
- Log every domain lookup: timestamp, client MAC/IP, domain requested
- You see what sites they visit (domain-level, not full URLs since HTTPS)
- Example log: `2026-09-15 12:00:01 | 192.168.1.102 | AA:BB:CC:DD:EE:FF | google.com`

**Without DNS gateway:**
- Only MAC, hostname, connect/disconnect times from st_lan.w.xml
- No URL visibility at all

### Recommended approach for hotspot mode

The **DNS gateway (Option 2)** is the cleanest path. It gives you:
- Per-client timed access (30s or whatever)
- Full domain-level URL logging
- No modification to the JCR1440 firmware
- Works from the same Pi/laptop that runs the poller

**What to build:**
1. `hotspot-manager.py` — polls st_lan.w.xml, tracks client sessions, enforces timeouts
2. DNS proxy config — dnsmasq on the Pi with logging + per-client allow/block
3. Dashboard — simple web UI showing active clients, time remaining, URL log

**Prerequisite we need to verify (when device is back online):**
- Can we change the DHCP-pushed DNS server via `set_net.html` or `set_adv.html`?
- If yes → DNS gateway works perfectly
- If no → we fall back to password rotation or ARP approach

---

## Mode B: GPS Tracking Device

### What the device already provides
- GPS receiver (39 satellites, ~15m accuracy, 10s refresh)
- Jio LTE connectivity (always-on, jionet APN)
- Public WAN IP (CGNAT behind Jio, plus IPv6)
- Web API with GPS + OBD data at `/st_gps.w.xml`
- USB RNDIS ethernet for local data extraction
- 12V power from OBD-II port (vehicle ignition)

### Architecture options

#### Option 1: Pull model (Pi polls the device)
```
JCR1440 (in vehicle, OBD-II)
    │ USB RNDIS
    ▼
Raspberry Pi Zero W (in vehicle)
    │ runs jcr1440-poller.py
    │ stores data locally (SD card)
    │ forwards via Pi's own WiFi/LTE to server
    ▼
Home Server / VPS
    │ receives MQTT / HTTP POST
    ▼
Grafana / Home Assistant / Web dashboard
```

**Pros:** Already works with the poller we built. Reliable, Pi handles buffering.
**Cons:** Needs extra hardware (Pi) in the vehicle.

#### Option 2: Direct remote access via LTE
```
JCR1440 (in vehicle, OBD-II)
    │ Jio LTE (CGNAT)
    │
    ▼ (can't reach directly — behind CGNAT)
```

**Problem:** The JCR1440's WAN IP is behind Jio's CGNAT. You can't
reach port 80 from the internet. IPv6 might be reachable if Jio
doesn't firewall it, but the device only listens on port 80 on the LAN side.

**Verdict:** Direct remote access won't work without a tunnel or VPN.

#### Option 3: Phone-as-bridge (no extra hardware)
```
JCR1440 (in vehicle, OBD-II)
    │ WiFi hotspot
    ▼
Your phone (connected to JCR1440 WiFi)
    │ runs a lightweight polling app
    │ forwards GPS data via phone's mobile data
    ▼
Home Server / VPS
```

**Pros:** No extra hardware needed.
**Cons:** Only works when phone is in the car and connected.

#### Option 4: USB-only (simplest, local tracking)
```
JCR1440 (in vehicle, OBD-II)
    │ USB to laptop/Pi
    ▼
jcr1440-poller.py → data.jsonl
    │
    ▼
Post-trip upload / analysis
```

**Pros:** Already works today. Zero extra cost.
**Cons:** No real-time remote tracking, data only available after trip.

### Recommended approach for tracking mode

**Start with Option 4 (USB-only)** — it works today with the poller.

**Graduate to Option 1 (Pi in vehicle)** when you want real-time:
- Pi Zero W (~₹1500) + USB cable
- Pi connects to JCR1440 via USB RNDIS
- Pi connects to your home server via its own WiFi (when parked) or a separate LTE dongle
- MQTT for lightweight real-time telemetry
- SQLite on SD card for offline buffering

### GPS data available per poll (10-second resolution)

| Field       | Example           | Notes                          |
|-------------|-------------------|--------------------------------|
| latitude    | 19.076000         | Decimal degrees                |
| longitude   | 72.877000         | Decimal degrees                |
| altitude    | 2.3 m             | When 3D fix available          |
| speed       | 0.0 km/h          | From GPS, not OBD              |
| heading     | 68.2°             | Compass bearing                |
| satellites  | 39                | Visible satellites             |
| accuracy    | 13-18 m           | Horizontal accuracy            |
| hdop        | 2.4               | Horizontal dilution            |
| fix_mode    | 2                 | 2=2D, 3=3D                     |
| timestamp   | UTC epoch ms      | GPS time (not device clock)    |

### OBD data (when plugged into vehicle)

| Field          | XML tag    | Notes                         |
|----------------|------------|-------------------------------|
| VIN            | vin        | Vehicle Identification Number |
| Engine RPM     | erpm       | Revolutions per minute        |
| Vehicle speed  | vspeed     | From ECU, more accurate       |
| Coolant temp   | ect        | Engine coolant temperature    |
| Fuel level     | fl         | Percentage                    |
| Battery voltage| celv       | 12V system voltage            |
| DTC codes      | dtc        | Active diagnostic trouble codes|
| Air temp       | iat        | Intake air temperature        |
| MAF            | maf        | Mass airflow rate             |
| Throttle       | atp        | Absolute throttle position    |
| Oil temp       | eot        | Engine oil temperature        |
| MIL status     | mils       | Check engine light on/off     |

---

## What to do next (when device is back online)

1. **Explore WiFi settings** — fetch `set_wifi.html`, `set_net.html`, `set_adv.html`
   to see if we can enable WiFi and change DHCP DNS server
2. **Enable WiFi** — turn on the hotspot via the web UI
3. **Test DNS gateway feasibility** — check if DHCP DNS is configurable
4. **Build hotspot-manager.py** — client tracking + timed access logic
5. **Test with a phone** — connect to the hotspot, verify client appears in st_lan.w.xml
