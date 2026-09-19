# Saarthi / JCR1440 — Raspberry Pi Setup Guide

## What's on this drive

```
SANDISK/
├── deploy/pi/
│   ├── first-boot.sh      ← Run this first (installs everything)
│   ├── setup.sh            ← Run this second (builds & installs the cluster)
│   └── bench-test.sh       ← Run this to verify everything works
├── crates/
│   ├── jcr1440-client/     ← Async OBD-II/GPS telemetry client
│   └── jcr1440-cluster/    ← Racing instrument cluster (egui/eframe)
├── scripts/                ← Diagnostic & config scripts
└── docs/                   ← Documentation
```

## Step-by-step setup

### 1. Plug the flash drive into the Pi

Mount it:
```bash
sudo mkdir -p /mnt/usb
sudo mount /dev/sda1 /mnt/usb
```

### 2. Copy the project to the Pi

```bash
cp -r /mnt/usb /home/shambhu/JioMotive
cd /home/shambhu/JioMotive
```

### 3. Run first-boot setup

This installs all system dependencies, Rust, SSH, and optimizes thermals/performance:
```bash
cd /home/shambhu/JioMotive/deploy/pi
chmod +x first-boot.sh
sudo ./first-boot.sh
```

Wait for it to complete. This will:
- Enable SSH (password + key auth)
- Install build deps (GL, X11, Wayland, SSL, fonts)
- Install Rust toolchain
- Set GPU memory to 128MB
- Set CPU governor to performance
- Configure zram swap (zstd, saves SD card wear)
- Disable bloat services (snap, cups, bluetooth, ModemManager)

### 4. Reboot

```bash
sudo reboot
```

### 5. Build and install the cluster

After reboot, SSH in from your desktop (`ssh shambhu@<pi-ip>`) or use the Pi directly:
```bash
cd /home/shambhu/JioMotive/deploy/pi
chmod +x setup.sh
sudo ./setup.sh
```

This builds the Rust binary (takes ~5-10 min on Pi 4, ~3 min on Pi 5) and installs:
- The cluster binary at `/opt/jcr1440/`
- udev rules for auto-detecting JCR1440 USB
- systemd services for auto-launching the cluster

### 6. Verify with bench test

```bash
sudo ./bench-test.sh
```

### 7. Done

Plug the JCR1440 OBD-II dongle into the car and connect USB to the Pi.
The cluster auto-launches fullscreen.

**Manual launch:**
```bash
# Mock mode (no device needed):
/opt/jcr1440/jcr1440-cluster --mock

# Live mode:
/opt/jcr1440/jcr1440-cluster --fullscreen --ip 192.168.1.1
```

## Troubleshooting

| Problem | Fix |
|---|---|
| SSH refused | `sudo systemctl enable --now ssh` |
| No display | Check `gpu_mem=128` in `/boot/firmware/config.txt` |
| Cluster won't start | `journalctl -u jcr1440-cluster -f` |
| JCR1440 not detected | `lsusb \| grep 05c6:f00e` — check USB cable |
| High temps | `vcgencmd measure_temp` — ensure fan is connected |
| Slow build | Normal on Pi 4 — cross-compile from desktop instead |
