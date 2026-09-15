# JCR1440 Instrument Cluster

Racing-style instrument cluster for the JCR1440 OBD-II telematics device.

## Quick start

```bash
# Dev machine with mock data (no device needed)
cargo run -p jcr1440-cluster -- --mock

# Connected to real device
cargo run -p jcr1440-cluster -- --ip 192.168.1.1

# Fullscreen (for Pi deployment)
cargo run -p jcr1440-cluster --release -- --fullscreen --ip 192.168.1.1
```

## Gauges

All gauges are built only for fields confirmed present in `st_gps.w.xml`:

| Gauge | Source field | Type |
|---|---|---|
| RPM tachometer | `erpm` | Sweeping needle, 0-8000 |
| Shift lights | `erpm` | 10-segment bar, green→yellow→red |
| Speed | `vspeed` | Large digital readout |
| Coolant temp | `ect` | Vertical bar with color zones |
| Intake air temp | `iat` | Vertical bar with color zones |
| Battery voltage | `celv` | Digital readout with color |
| DTC indicator | `dtc`/`mils` | Hidden when 0, red badge when active |
| GPS info | `gps_data` | Satellite count, coordinates |

## Architecture

```
┌─────────────────────────────┐
│  jcr1440-client (tokio)     │
│  - MD5 challenge-response   │
│  - Polls st_gps.w.xml ~300ms│
│  - Auto re-auth on expiry   │
│  - Backoff on failures      │
├──── watch::channel ─────────┤
│  jcr1440-cluster (egui)     │
│  - 60fps render loop        │
│  - Exponential smoothing    │
│  - glow/OpenGL ES backend   │
└─────────────────────────────┘
```

The poller and renderer are decoupled: data arrives every ~300ms, but the
render loop runs at 60fps. Gauge needles use exponential smoothing
(`lerp` with time-based alpha) so motion looks fluid between data updates.

## Pi deployment

### Build for Pi (on Pi itself, or cross-compile)

```bash
# On Pi directly:
cargo build --release -p jcr1440-cluster

# Install
sudo cp target/release/jcr1440-cluster /opt/jcr1440/
sudo cp deploy/jcr1440-cluster.service /etc/systemd/system/

# Enable auto-start
sudo systemctl daemon-reload
sudo systemctl enable jcr1440-cluster
sudo systemctl start jcr1440-cluster
```

### GPU notes

eframe uses the **glow** backend (OpenGL ES 2.0/3.0), which runs natively
on the Pi 4/5 V3D GPU driver. No Vulkan needed. If frame times are high:

1. Ensure the Pi is using the hardware GL driver: `sudo raspi-config` →
   Advanced → GL Driver → GL (Fake KMS) or Full KMS
2. Allocate sufficient GPU memory: `gpu_mem=128` in `/boot/config.txt`
3. The cluster disables vsync by default; for 60fps on a 60Hz display this
   should be fine without tearing on the Pi's HDMI output.

## Mock mode

```bash
cargo run -p jcr1440-cluster -- --mock
```

Generates synthetic data: RPM sweeps 0→8000→0 in a sine wave, speed
scales with RPM, temperatures oscillate realistically. Useful for
iterating on gauge visuals without a device connected.
