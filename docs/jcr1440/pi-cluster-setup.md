# Raspberry Pi Cluster Display Setup

Complete guide to connecting a JCR1440 OBD-II dongle to a Raspberry Pi
and running the Nino's Tech racing instrument cluster on an HDMI display
in a vehicle.

## What you need

| Item | Notes |
|---|---|
| JCR1440 OBD-II dongle | Plugged into the vehicle's OBD-II port (under dash) |
| Raspberry Pi 4 or 5 | 2GB+ RAM, Pi 5 preferred for smoother rendering |
| MicroSD card | 16GB+, flashed with Raspberry Pi OS (64-bit, with desktop) |
| HDMI display | 7" touchscreen or any HDMI monitor, 1024x600 or higher |
| USB-A to Micro-USB cable | Connects Pi to JCR1440 (the dongle's USB port) |
| 12V to 5V USB-C adapter | Powers the Pi from the car's 12V (cigarette lighter or hardwire) |
| (Optional) USB-C PD adapter | If using Pi 5, it wants 5V/5A — a good car USB-C PD adapter helps |

## How the pieces connect

```
Vehicle OBD-II Port (under dashboard)
        |
        |  OBD-II connector (16-pin)
        |  Pin 16: +12V battery power
        |  Pin 6/14: CAN bus (vehicle data)
        v
┌──────────────────────────┐
│   JCR1440 Dongle         │
│                          │
│   Reads CAN bus → OBD-II │
│   GPS antenna (built-in) │
│   LTE modem (Jio eSIM)  │
│   WiFi hotspot           │
│                          │
│   Web API on 192.168.1.1 │
│   ├── /st_gps.w.xml      │  <-- all OBD + GPS data in one endpoint
│   ├── /st_dev.w.xml      │
│   └── /st_lte.w.xml      │
│                          │
│   USB port (Micro-USB)   │
└──────────┬───────────────┘
           │
           │  USB cable (data + power to dongle)
           │  Shows up as RNDIS Ethernet on Pi
           │  Pi gets IP 192.168.1.100 via DHCP
           v
┌──────────────────────────┐
│   Raspberry Pi 4/5       │
│                          │
│   USB RNDIS interface    │
│   enxXXXXXXXXXXXX        │
│   IP: 192.168.1.100     │
│                          │
│   jcr1440-cluster binary │
│   ├── Authenticates to   │
│   │   JCR1440 web API    │
│   ├── Polls every ~300ms │
│   ├── Renders at 60fps   │
│   └── Fullscreen on HDMI │
│                          │
│   HDMI out               │
└──────────┬───────────────┘
           │
           │  HDMI cable
           v
┌──────────────────────────┐
│   7" Display / Monitor   │
│                          │
│   Mounted on dashboard   │
│   or A-pillar            │
│                          │
│   Shows live gauges:     │
│   RPM, Speed, Temps,     │
│   Gear, Voltage, etc.    │
└──────────────────────────┘
```

## Quick setup (automated)

The fastest path — one script does everything:

```bash
# 1. Flash Pi OS (64-bit with desktop), boot, SSH in
# 2. Clone the repo
git clone <your-repo-url> ~/JioMotive
cd ~/JioMotive

# 3. Run the setup script (installs deps, Rust, builds, configures auto-start)
sudo ./deploy/pi/setup.sh

# 4. Run the bench test (validates everything works)
sudo ./deploy/pi/bench-test.sh

# 5. Reboot, then plug JCR1440 into car + USB to Pi — cluster auto-launches
sudo reboot
```

That's it. After reboot, the Pi is plug-and-play: connect JCR1440 USB
cable and the cluster appears fullscreen within ~10 seconds.

The setup script handles: system packages, Rust, release build, udev
rules (auto-detect USB), systemd services (auto-launch), GPU config,
screen blanking, and routing (subnet collision fix).

## Manual step-by-step setup

If you prefer to do it manually, or the setup script doesn't work:

### 1. Flash the Pi

1. Download Raspberry Pi OS (64-bit, with desktop) from https://www.raspberrypi.com/software/
2. Flash to MicroSD using Raspberry Pi Imager
3. Enable SSH and set WiFi credentials in the imager (for headless setup)
4. Boot the Pi, SSH in: `ssh pi@raspberrypi.local`

### 2. Install Rust on the Pi

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
rustc --version  # should show 1.70+
```

Install build dependencies:

```bash
sudo apt-get update
sudo apt-get install -y \
    build-essential pkg-config \
    libx11-dev libxcb1-dev libwayland-dev libxkbcommon-dev \
    libssl-dev libfontconfig1-dev \
    libgl1-mesa-dev libgles2-mesa-dev
```

### 3. Clone and build the cluster

```bash
git clone <your-repo-url> ~/JioMotive
cd ~/JioMotive

# Build release binary (takes ~5-10 min on Pi 4, ~3 min on Pi 5)
cargo build --release -p jcr1440-cluster

# Test with mock data first (make sure display works)
cargo run -p jcr1440-cluster --release -- --mock --fullscreen
```

You should see the cluster fullscreen with synthetic sweeping gauges.
Press Escape or Alt+F4 to exit.

### 4. Connect the JCR1440

1. Plug the JCR1440 into the vehicle's OBD-II port
2. Wait 10-15 seconds for it to boot (USB PID switches from f000 to f00e)
3. Connect the USB cable from the JCR1440 to the Pi
4. Verify connection:

```bash
# Check USB device appeared
lsusb | grep 05c6:f00e
# Should show: Qualcomm, Inc. FP3

# Check network interface appeared
ip addr show | grep "192.168.1"
# Should show: inet 192.168.1.100/24 on an enxXXXX interface

# Test connectivity
ping -c 2 192.168.1.1
# Should respond in ~1ms
```

5. Run the cluster with real data:

```bash
cargo run -p jcr1440-cluster --release -- --fullscreen --ip 192.168.1.1
```

Gauges should now show live vehicle data (RPM, speed, temps, etc.).

### 5. Set up auto-start on boot

Install the systemd service so the cluster launches automatically:

```bash
# Copy binary to /opt
sudo mkdir -p /opt/jcr1440
sudo cp target/release/jcr1440-cluster /opt/jcr1440/

# Install service file
sudo cp deploy/jcr1440-cluster.service /etc/systemd/system/

# Edit service if your user is different from 'pi'
sudo nano /etc/systemd/system/jcr1440-cluster.service
# Change User=pi to your username if needed

# Enable and start
sudo systemctl daemon-reload
sudo systemctl enable jcr1440-cluster
sudo systemctl start jcr1440-cluster
```

The cluster will now launch fullscreen every time the Pi boots.

### 6. Pi display configuration

For best results with a 7" HDMI display:

```bash
sudo nano /boot/firmware/config.txt
```

Add or modify these lines:

```ini
# GPU memory for smooth rendering
gpu_mem=128

# Force HDMI output even if no display detected at boot
hdmi_force_hotplug=1

# For 7" 1024x600 displays (adjust for your panel)
hdmi_group=2
hdmi_mode=87
hdmi_cvt=1024 600 60 3 0 0 0

# Disable screen blanking
consoleblank=0

# Use full KMS driver for hardware OpenGL
dtoverlay=vc4-kms-v3d
```

Disable screen saver:

```bash
# Prevent screen from going blank
sudo apt-get install -y xdotool
echo '@xset s off' >> ~/.config/lxsession/LXDE-pi/autostart
echo '@xset -dpms' >> ~/.config/lxsession/LXDE-pi/autostart
echo '@xset s noblank' >> ~/.config/lxsession/LXDE-pi/autostart
```

## Data flow

When everything is connected and the engine is running:

```
Vehicle ECU (CAN bus)
    |
    |  OBD-II PIDs requested by JCR1440
    v
JCR1440 reads: RPM, speed, coolant temp, intake temp,
    battery voltage, throttle position, manifold pressure,
    fuel rate, fuel level, oil temp, DTC codes, MIL status
    |
    |  Also reads: GPS (lat, lon, speed, heading, altitude, satellites)
    v
JCR1440 web API (http://192.168.1.1/st_gps.w.xml)
    |
    |  HTTP GET every ~300ms (authenticated session)
    |  Auth: MD5 challenge-response + CSRF token + SessionID cookie
    v
jcr1440-client (Rust, async tokio)
    |
    |  Parses XML → TelemetryFrame struct
    |  Handles session expiry (auto re-auth)
    |  Exponential backoff on failures
    v
watch::channel (lock-free, non-blocking)
    |
    |  Latest frame available instantly
    v
jcr1440-cluster (Rust, egui/glow @ 60fps)
    |
    |  Exponential smoothing on all gauge values
    |  Needle animation interpolated between data updates
    v
HDMI display: RPM gauge, speedometer, gear indicator,
    CLT, IAT, OIL temps, TPS, battery voltage, MAP,
    fuel rate, DTC indicator, GPS info
```

## What each gauge shows

| Gauge | OBD field | What it means |
|---|---|---|
| RPM (left dial) | `erpm` | Engine revolutions per minute, 0-8000 scale |
| Speed (right dial) | `vspeed` | Vehicle speed from ECU in km/h, 0-240 scale |
| Gear (top bar + inside speedo) | Calculated | Derived from RPM/speed ratio, shows N or 1-6 |
| CLT (bottom box) | `ect` | Engine coolant temperature in °C, green <90, amber >95, red >105 |
| IAT (bottom box) | `iat` | Intake air temperature in °C, affects air density/power |
| OIL (bottom box) | `eot` | Engine oil temperature in °C, red above 120 |
| TPS (bottom box) | `atp` | Throttle pedal position 0-100%, green/amber/red |
| BATT (bottom box) | `celv` | 12V system voltage, red below 11.5V |
| MAP (bottom box) | `imap` | Intake manifold pressure in kPa, indicates engine load |
| Fuel rate (inside RPM dial) | `efr` | Instantaneous fuel consumption in L/h |
| DTC badge (top left) | `dtc`/`mils` | Check engine light — hidden when 0, red badge with count |
| GPS (footer) | `gps_data` | Satellite count, HDOP accuracy, lat/lon coordinates |

## Troubleshooting

### "NO SIGNAL" on the cluster display

1. Check USB cable is connected between Pi and JCR1440
2. Verify JCR1440 has power (needs OBD-II 12V — won't work from USB alone for long)
3. Check `lsusb | grep 05c6` shows the device
4. Check `ip addr` shows 192.168.1.100 on the RNDIS interface
5. Try `ping 192.168.1.1` — should respond
6. If ping works but cluster shows NO SIGNAL, check auth:
   ```bash
   # Quick auth test
   python3 scripts/jcr1440-poller.py --once
   ```

### Gauges show 0 / no data

- OBD fields only populate when the JCR1440 is plugged into a running vehicle
- On the bench (USB power only), all OBD fields return "-" — this is normal
- GPS will show data anywhere (doesn't need a vehicle)

### USB keeps disconnecting

- The JCR1440 needs 12V from the OBD-II port for stable operation
- On USB-only power, it disconnects every 2-5 minutes (supercapacitor drains)
- Solution: always use it plugged into the car's OBD-II port
- If still unstable, use a powered USB hub (>1A) between Pi and JCR1440

### Display issues on Pi

- If you see a black screen, check `gpu_mem=128` in config.txt
- If rendering is slow, ensure `dtoverlay=vc4-kms-v3d` is set (hardware GL)
- For Wayland (Pi 5 default): set `WAYLAND_DISPLAY=wayland-0` in the service file
- For X11 (Pi 4 default): set `DISPLAY=:0` in the service file

### Subnet collision (Pi also connected to a home router at 192.168.1.x)

If the Pi has another network interface (WiFi/Ethernet) on the same 192.168.1.0/24 subnet:

```bash
# Add a specific route so 192.168.1.1 always goes via the RNDIS interface
RNDIS_IF=$(ip -o link show | grep -i rndis | awk -F: '{print $2}' | tr -d ' ')
sudo ip route add 192.168.1.1/32 dev $RNDIS_IF metric 50
```

Or change your home router to a different subnet (e.g. 192.168.0.x or 10.0.0.x).

## Physical mounting suggestions

- **7" HDMI touchscreen**: Mount on the dashboard using a RAM mount or 3D-printed bracket
- **Pi enclosure**: Use a vented case, mount behind the display or under the dash
- **Cable routing**: Route USB and HDMI cables along the A-pillar trim, under the dash mat
- **Power**: Tap into a switched 12V source (only on when ignition is on) via a USB-C car adapter
- **OBD-II**: The JCR1440 plugs directly into the OBD-II port (usually under the steering column)

```
   Dashboard
   ┌─────────────────────────────────────┐
   │                                     │
   │    ┌───────────┐                    │
   │    │ 7" HDMI   │  ← mounted on     │
   │    │ Display   │    dash or vent    │
   │    │           │                    │
   │    └─────┬─────┘                    │
   │          │ HDMI cable               │
   │    ┌─────┴─────┐                    │
   │    │ Pi 4/5    │  ← behind display  │
   │    │           │    or under dash   │
   │    └──┬────┬───┘                    │
   │       │    │                        │
   │       │    └── USB-C power ← 12V adapter
   │       │
   │       └── USB cable
   │           │
   │    ┌──────┴─────┐
   │    │ JCR1440    │  ← plugged into
   │    │ Dongle     │    OBD-II port
   │    └────────────┘
   └─────────────────────────────────────┘
```
