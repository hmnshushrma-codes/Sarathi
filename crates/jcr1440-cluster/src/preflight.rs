//! Automotive-style preflight diagnostic screen.
//!
//! Validates system health before showing the main cluster:
//!   - Raspberry Pi health (CPU, RAM, temp, storage)
//!   - JCR1440 USB detection and connectivity
//!   - OBD telemetry stream status
//!   - GPS fix status
//!   - Network connectivity
//!   - Telemetry monitor service
//!
//! Required systems must pass for READY; optional systems produce warnings.

use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use egui::{
    Align2, Color32, FontId, Painter, Pos2, Rect, Stroke,
    StrokeKind, Ui, Vec2,
};
use jcr1440_client::DeviceState;
use tokio::sync::watch;

use crate::clusters::ClusterRenderer;
use crate::theme;

// ---------------------------------------------------------------------------
// Colors — NinoDash themed
// ---------------------------------------------------------------------------

const BG_DARK: Color32 = Color32::from_rgb(5, 5, 5);
const CARD_BG: Color32 = Color32::from_rgb(17, 17, 20);
const CARD_BORDER: Color32 = Color32::from_rgb(36, 36, 40);
const TITLE_COLOR: Color32 = Color32::from_rgb(255, 98, 0);
const SUBTITLE_COLOR: Color32 = Color32::from_rgb(140, 145, 155);
const TEXT_WHITE: Color32 = Color32::from_rgb(244, 244, 244);
const TEXT_DIM: Color32 = Color32::from_rgb(118, 118, 118);
const STATUS_READY: Color32 = Color32::from_rgb(30, 200, 80);    // green = healthy
const STATUS_WARNING: Color32 = Color32::from_rgb(255, 160, 0);  // orange = warning
const STATUS_FAILED: Color32 = Color32::from_rgb(227, 24, 24);   // red = failed
const STATUS_CHECKING: Color32 = Color32::from_rgb(168, 168, 168);
const BTN_BG: Color32 = Color32::from_rgb(255, 98, 0);
const BTN_BG_HOVER: Color32 = Color32::from_rgb(255, 120, 0);
const BTN_BG_DISABLED: Color32 = Color32::from_rgb(36, 36, 40);
const BTN_TEXT: Color32 = Color32::from_rgb(5, 5, 5);
const BTN_TEXT_DISABLED: Color32 = Color32::from_rgb(80, 80, 85);
const CLOCK_COLOR: Color32 = Color32::from_rgb(168, 168, 168);
const DETAIL_BG: Color32 = Color32::from_rgb(12, 12, 16);

// ---------------------------------------------------------------------------
// Check status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CheckStatus {
    Checking,
    Ready,
    Connected,
    Receiving,
    Warning,
    Failed,
    NotAvailable,
}

impl CheckStatus {
    fn label(&self) -> &'static str {
        match self {
            Self::Checking => "CHECKING",
            Self::Ready => "READY",
            Self::Connected => "CONNECTED",
            Self::Receiving => "RECEIVING",
            Self::Warning => "WARNING",
            Self::Failed => "FAILED",
            Self::NotAvailable => "NOT AVAILABLE",
        }
    }

    fn icon(&self) -> &'static str {
        match self {
            Self::Checking => "...",
            Self::Ready | Self::Connected | Self::Receiving => "[OK]",
            Self::Warning => "[!!]",
            Self::Failed | Self::NotAvailable => "[XX]",
        }
    }

    fn color(&self) -> Color32 {
        match self {
            Self::Checking => STATUS_CHECKING,
            Self::Ready | Self::Connected | Self::Receiving => STATUS_READY,
            Self::Warning => STATUS_WARNING,
            Self::Failed | Self::NotAvailable => STATUS_FAILED,
        }
    }

    fn is_ok(&self) -> bool {
        matches!(self, Self::Ready | Self::Connected | Self::Receiving)
    }

    fn is_done(&self) -> bool {
        !matches!(self, Self::Checking)
    }
}

// ---------------------------------------------------------------------------
// System check data
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SystemCheck {
    pub name: String,
    pub status: CheckStatus,
    pub detail: String,
    pub required: bool,
    /// Extra details for the expanded view
    pub extra: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OverallStatus {
    Checking,
    Ready,
    ReadyWithWarnings,
    NotReady,
}

impl OverallStatus {
    fn label(&self) -> &'static str {
        match self {
            Self::Checking => "CHECKING",
            Self::Ready => "READY",
            Self::ReadyWithWarnings => "READY WITH WARNINGS",
            Self::NotReady => "NOT READY",
        }
    }

    fn color(&self) -> Color32 {
        match self {
            Self::Checking => STATUS_CHECKING,
            Self::Ready => STATUS_READY,
            Self::ReadyWithWarnings => STATUS_WARNING,
            Self::NotReady => STATUS_FAILED,
        }
    }
}

// ---------------------------------------------------------------------------
// Preflight state
// ---------------------------------------------------------------------------

pub struct PreflightState {
    pub checks: Vec<SystemCheck>,
    pub overall: OverallStatus,
    pub status_message: String,
    pub started_at: Instant,
    pub checks_complete: bool,
    pub expanded_check: Option<usize>,
    pub dev_panel_open: bool,
    /// Selected cluster index for preflight cluster selector
    pub selected_cluster_idx: usize,
    /// Background check thread result channel
    bg_rx: Option<mpsc::Receiver<Vec<SystemCheck>>>,
    /// Last telemetry state for live updates
    last_device_state: DeviceState,
    last_telemetry_update: Instant,
    telemetry_packets: u64,
}

impl PreflightState {
    pub fn new() -> Self {
        Self {
            checks: vec![
                SystemCheck {
                    name: "Raspberry Pi System".into(),
                    status: CheckStatus::Checking,
                    detail: String::new(),
                    required: true,
                    extra: vec![],
                },
                SystemCheck {
                    name: "JCR1440 USB Device".into(),
                    status: CheckStatus::Checking,
                    detail: String::new(),
                    required: true,
                    extra: vec![],
                },
                SystemCheck {
                    name: "OBD Interface".into(),
                    status: CheckStatus::Checking,
                    detail: String::new(),
                    required: true,
                    extra: vec![],
                },
                SystemCheck {
                    name: "Vehicle Data Stream".into(),
                    status: CheckStatus::Checking,
                    detail: String::new(),
                    required: true,
                    extra: vec![],
                },
                SystemCheck {
                    name: "GPS".into(),
                    status: CheckStatus::Checking,
                    detail: String::new(),
                    required: false,
                    extra: vec![],
                },
                SystemCheck {
                    name: "Network".into(),
                    status: CheckStatus::Checking,
                    detail: String::new(),
                    required: false,
                    extra: vec![],
                },
                SystemCheck {
                    name: "Telemetry Service".into(),
                    status: CheckStatus::Checking,
                    detail: String::new(),
                    required: false,
                    extra: vec![],
                },
                SystemCheck {
                    name: "Storage".into(),
                    status: CheckStatus::Checking,
                    detail: String::new(),
                    required: false,
                    extra: vec![],
                },
            ],
            overall: OverallStatus::Checking,
            status_message: "Running preflight checks...".into(),
            started_at: Instant::now(),
            checks_complete: false,
            expanded_check: None,
            dev_panel_open: false,
            selected_cluster_idx: 0,
            bg_rx: None,
            last_device_state: DeviceState::Disconnected,
            last_telemetry_update: Instant::now(),
            telemetry_packets: 0,
        }
    }

    /// Kick off background system checks (non-blocking).
    pub fn start_checks(&mut self) {
        let (tx, rx) = mpsc::channel();
        self.bg_rx = Some(rx);

        std::thread::spawn(move || {
            let checks = run_system_checks();
            let _ = tx.send(checks);
        });
    }

    /// Poll background checks and update telemetry state.
    pub fn update(&mut self, device_rx: &watch::Receiver<DeviceState>) {
        // Check if background system checks have completed
        if let Some(ref rx) = self.bg_rx {
            if let Ok(system_checks) = rx.try_recv() {
                // Merge system checks with our check list
                for sc in &system_checks {
                    if let Some(check) = self.checks.iter_mut().find(|c| c.name == sc.name) {
                        check.status = sc.status;
                        check.detail = sc.detail.clone();
                        check.extra = sc.extra.clone();
                    }
                }
                self.bg_rx = None;
            }
        }

        // Update from telemetry poller state
        let state = device_rx.borrow().clone();
        match &state {
            DeviceState::Live(frame) => {
                self.telemetry_packets += 1;
                self.last_telemetry_update = Instant::now();

                // JCR1440 USB — must be connected if we're getting data
                if let Some(c) = self.checks.iter_mut().find(|c| c.name == "JCR1440 USB Device") {
                    if !matches!(c.status, CheckStatus::Connected) {
                        c.status = CheckStatus::Connected;
                        c.detail = "Device responding".into();
                    }
                }

                // OBD Interface
                if let Some(c) = self.checks.iter_mut().find(|c| c.name == "OBD Interface") {
                    let has_obd = frame.obd.engine_rpm.is_some()
                        || frame.obd.vehicle_speed.is_some()
                        || frame.obd.battery_voltage.is_some();
                    if has_obd {
                        c.status = CheckStatus::Connected;
                        c.detail = "OBD data available".into();
                    } else if frame.device_connected {
                        c.status = CheckStatus::Warning;
                        c.detail = "Device connected, no OBD data".into();
                    }
                }

                // Vehicle Data Stream
                if let Some(c) = self.checks.iter_mut().find(|c| c.name == "Vehicle Data Stream") {
                    let has_data = frame.obd.engine_rpm.is_some()
                        || frame.obd.vehicle_speed.is_some();
                    if has_data {
                        c.status = CheckStatus::Receiving;
                        c.detail = format!("{} packets", self.telemetry_packets);

                        let mut extras = vec![];
                        extras.push(("Source".into(), "JCR1440".into()));
                        extras.push(("Packets".into(), format!("{}", self.telemetry_packets)));
                        if let Some(rpm) = frame.obd.engine_rpm {
                            extras.push(("RPM".into(), format!("{:.0}", rpm)));
                        }
                        if let Some(spd) = frame.obd.vehicle_speed {
                            extras.push(("Speed".into(), format!("{:.0} km/h", spd)));
                        }
                        if let Some(v) = frame.obd.battery_voltage {
                            extras.push(("Battery".into(), format!("{:.1} V", v)));
                        }
                        if let Some(t) = frame.obd.coolant_temp {
                            extras.push(("Coolant".into(), format!("{:.0} °C", t)));
                        }
                        if let Some(tp) = frame.obd.throttle_position {
                            extras.push(("Throttle".into(), format!("{:.0}%", tp)));
                        }
                        c.extra = extras;
                    } else {
                        c.status = CheckStatus::Warning;
                        c.detail = "Waiting for vehicle data...".into();
                    }
                }

                // GPS — show as ready if satellites visible, even without position fix
                if let Some(c) = self.checks.iter_mut().find(|c| c.name == "GPS") {
                    if frame.gps.fix_valid && frame.gps.satellites > 0 {
                        c.status = CheckStatus::Ready;
                        c.detail = format!("Fix: {} sats, HDOP {:.1}",
                            frame.gps.satellites, frame.gps.hdop);
                        c.extra = vec![
                            ("Fix".into(), "YES".into()),
                            ("Satellites".into(), format!("{}", frame.gps.satellites)),
                            ("HDOP".into(), format!("{:.1}", frame.gps.hdop)),
                            ("Accuracy".into(), format!("{:.0} m", frame.gps.accuracy)),
                        ];
                    } else if frame.gps.satellites > 0 {
                        // GPS module active, has satellites but no position fix yet
                        c.status = CheckStatus::Ready;
                        c.detail = format!("Active: {} sats, no fix", frame.gps.satellites);
                        c.extra = vec![
                            ("Fix".into(), "NO (searching)".into()),
                            ("Satellites".into(), format!("{}", frame.gps.satellites)),
                            ("Module".into(), "ACTIVE".into()),
                        ];
                    } else {
                        c.status = CheckStatus::Warning;
                        c.detail = "No GPS signal".into();
                        c.extra = vec![
                            ("Fix".into(), "NO".into()),
                            ("Satellites".into(), "0".into()),
                        ];
                    }
                }
            }
            DeviceState::Connecting => {
                if let Some(c) = self.checks.iter_mut().find(|c| c.name == "JCR1440 USB Device") {
                    c.status = CheckStatus::Checking;
                    c.detail = "Connecting...".into();
                }
            }
            DeviceState::Error { message, failures } => {
                if let Some(c) = self.checks.iter_mut().find(|c| c.name == "JCR1440 USB Device") {
                    if *failures > 5 {
                        c.status = CheckStatus::Failed;
                    } else {
                        c.status = CheckStatus::Checking;
                    }
                    c.detail = format!("{} ({}x)", message, failures);
                }
                if let Some(c) = self.checks.iter_mut().find(|c| c.name == "Vehicle Data Stream") {
                    c.status = CheckStatus::Warning;
                    c.detail = "No data yet".into();
                }
            }
            DeviceState::Disconnected => {
                if let Some(c) = self.checks.iter_mut().find(|c| c.name == "JCR1440 USB Device") {
                    if self.started_at.elapsed() > Duration::from_secs(10) {
                        c.status = CheckStatus::Failed;
                        c.detail = "Not detected".into();
                    }
                }
            }
        }
        self.last_device_state = state;

        // Compute overall status
        let all_done = self.checks.iter().all(|c| c.status.is_done());
        let _required_ok = self.checks.iter()
            .filter(|c| c.required)
            .all(|c| c.status.is_ok());
        let any_warning = self.checks.iter()
            .any(|c| matches!(c.status, CheckStatus::Warning));
        let required_failed = self.checks.iter()
            .filter(|c| c.required)
            .any(|c| matches!(c.status, CheckStatus::Failed | CheckStatus::NotAvailable));

        if !all_done && self.started_at.elapsed() < Duration::from_secs(15) {
            self.overall = OverallStatus::Checking;
            self.status_message = "Running preflight checks...".into();
        } else if required_failed {
            self.overall = OverallStatus::NotReady;
            let failed: Vec<_> = self.checks.iter()
                .filter(|c| c.required && !c.status.is_ok())
                .map(|c| c.name.as_str())
                .collect();
            self.status_message = format!("Required systems unavailable: {}",
                failed.join(", "));
        } else if any_warning {
            self.overall = OverallStatus::ReadyWithWarnings;
            let warns: Vec<_> = self.checks.iter()
                .filter(|c| matches!(c.status, CheckStatus::Warning))
                .map(|c| c.name.as_str())
                .collect();
            self.status_message = format!("{} have warnings. Cluster can run.",
                warns.join(", "));
        } else {
            self.overall = OverallStatus::Ready;
            self.status_message = "All systems operational.".into();
        }

        self.checks_complete = all_done || self.started_at.elapsed() > Duration::from_secs(15);
    }

    pub fn can_start(&self) -> bool {
        self.checks_complete && !matches!(self.overall, OverallStatus::Checking)
    }
}

// ---------------------------------------------------------------------------
// Background system checks (blocking, runs in thread)
// ---------------------------------------------------------------------------

fn run_system_checks() -> Vec<SystemCheck> {
    let mut results = vec![];

    // -- Raspberry Pi System --
    {
        let mut check = SystemCheck {
            name: "Raspberry Pi System".into(),
            status: CheckStatus::Ready,
            detail: String::new(),
            required: true,
            extra: vec![],
        };

        // CPU
        let cpu_count = std::thread::available_parallelism()
            .map(|n| n.get()).unwrap_or(1);
        check.extra.push(("CPU cores".into(), format!("{}", cpu_count)));

        // Memory
        if let Ok(out) = Command::new("free").arg("-m").output() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                if line.starts_with("Mem:") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 3 {
                        check.extra.push(("RAM total".into(), format!("{} MB", parts[1])));
                        check.extra.push(("RAM used".into(), format!("{} MB", parts[2])));
                    }
                }
            }
        }

        // Temperature
        if let Ok(out) = Command::new("vcgencmd").arg("measure_temp").output() {
            let text = String::from_utf8_lossy(&out.stdout);
            let temp_str = text.trim().replace("temp=", "").replace("'C", " °C");
            check.extra.push(("Temperature".into(), temp_str.clone()));

            // Parse temp value for warning
            if let Some(t) = temp_str.split_whitespace().next()
                .and_then(|s| s.parse::<f32>().ok())
            {
                if t > 80.0 {
                    check.status = CheckStatus::Warning;
                    check.detail = format!("High temperature: {:.0}°C", t);
                }
            }
        } else {
            // Not a Pi or vcgencmd not available
            check.extra.push(("Temperature".into(), "N/A".into()));
        }

        // Storage
        if let Ok(out) = Command::new("df").args(["-h", "/"]).output() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines().skip(1) {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 5 {
                    check.extra.push(("Disk total".into(), parts[1].into()));
                    check.extra.push(("Disk used".into(), parts[2].into()));
                    check.extra.push(("Disk free".into(), parts[3].into()));

                    let pct_str = parts[4].trim_end_matches('%');
                    if let Ok(pct) = pct_str.parse::<u32>() {
                        if pct > 90 {
                            check.status = CheckStatus::Warning;
                            check.detail = format!("Storage {}% full", pct);
                        }
                    }
                }
            }
        }

        // Uptime
        if let Ok(out) = Command::new("uptime").arg("-p").output() {
            check.extra.push(("Uptime".into(),
                String::from_utf8_lossy(&out.stdout).trim().into()));
        }

        if check.detail.is_empty() {
            check.detail = format!("{} cores, healthy", cpu_count);
        }
        results.push(check);
    }

    // -- JCR1440 USB Device (initial check via lsusb) --
    {
        let mut check = SystemCheck {
            name: "JCR1440 USB Device".into(),
            status: CheckStatus::Checking,
            detail: "Waiting for poller...".into(),
            required: true,
            extra: vec![],
        };

        if let Ok(out) = Command::new("lsusb").output() {
            let text = String::from_utf8_lossy(&out.stdout);
            let found = text.lines().any(|l| {
                let ll = l.to_lowercase();
                ll.contains("05c6:f00e") || ll.contains("qualcomm")
            });
            if found {
                check.status = CheckStatus::Connected;
                check.detail = "USB device detected".into();
                check.extra.push(("VID:PID".into(), "05c6:f00e".into()));

                // Find sysfs info
                for entry in std::fs::read_dir("/sys/bus/usb/devices/").into_iter().flatten().flatten() {
                    let p = entry.path();
                    let vid = std::fs::read_to_string(p.join("idVendor"))
                        .unwrap_or_default().trim().to_string();
                    let pid = std::fs::read_to_string(p.join("idProduct"))
                        .unwrap_or_default().trim().to_string();
                    if vid == "05c6" && pid == "f00e" {
                        let mfr = std::fs::read_to_string(p.join("manufacturer"))
                            .unwrap_or_default().trim().to_string();
                        let prod = std::fs::read_to_string(p.join("product"))
                            .unwrap_or_default().trim().to_string();
                        if !mfr.is_empty() { check.extra.push(("Manufacturer".into(), mfr)); }
                        if !prod.is_empty() { check.extra.push(("Product".into(), prod)); }
                        check.extra.push(("Sysfs".into(), p.display().to_string()));
                        break;
                    }
                }
            } else {
                check.status = CheckStatus::Failed;
                check.detail = "Not detected on USB".into();
            }
        }
        results.push(check);
    }

    // -- OBD Interface (checked via serial ports) --
    {
        let mut check = SystemCheck {
            name: "OBD Interface".into(),
            status: CheckStatus::Checking,
            detail: "Waiting for data...".into(),
            required: true,
            extra: vec![],
        };

        // Check for serial ports
        let serial_ports: Vec<_> = glob_paths("/dev/ttyUSB*")
            .into_iter()
            .chain(glob_paths("/dev/ttyACM*"))
            .collect();
        if !serial_ports.is_empty() {
            check.extra.push(("Serial ports".into(),
                serial_ports.iter().map(|p| p.to_string_lossy().to_string())
                    .collect::<Vec<_>>().join(", ")));
        }

        // Check for RNDIS/network interface to device
        let mut rndis_found = false;
        for entry in std::fs::read_dir("/sys/class/net/").into_iter().flatten().flatten() {
            let p = entry.path();
            let driver_link = p.join("device/driver");
            if let Ok(target) = std::fs::read_link(&driver_link) {
                let drv = target.file_name()
                    .unwrap_or_default().to_string_lossy().to_string();
                if drv == "rndis_host" {
                    let iface = p.file_name().unwrap_or_default().to_string_lossy().to_string();
                    check.extra.push(("RNDIS interface".into(), iface));
                    rndis_found = true;
                }
            }
        }

        if rndis_found || !serial_ports.is_empty() {
            check.status = CheckStatus::Connected;
            check.detail = "Interface available".into();
        }
        // Will be updated by telemetry poller
        results.push(check);
    }

    // -- Vehicle Data Stream (set by telemetry updates, just placeholder) --
    results.push(SystemCheck {
        name: "Vehicle Data Stream".into(),
        status: CheckStatus::Checking,
        detail: "Waiting for data...".into(),
        required: true,
        extra: vec![],
    });

    // -- GPS (set by telemetry updates, just placeholder) --
    results.push(SystemCheck {
        name: "GPS".into(),
        status: CheckStatus::Checking,
        detail: "Waiting for fix...".into(),
        required: false,
        extra: vec![],
    });

    // -- Network --
    {
        let mut check = SystemCheck {
            name: "Network".into(),
            status: CheckStatus::Warning,
            detail: "Offline".into(),
            required: false,
            extra: vec![],
        };

        // Check for any non-loopback interface with an IP
        if let Ok(out) = Command::new("ip").args(["-4", "addr", "show"]).output() {
            let text = String::from_utf8_lossy(&out.stdout);
            let has_ip = text.lines().any(|l| {
                l.contains("inet ") && !l.contains("127.0.0.1") && !l.contains("scope host")
            });
            if has_ip {
                check.status = CheckStatus::Ready;
                check.detail = "Connected".into();
            }
        }

        // WiFi status
        if let Ok(out) = Command::new("nmcli").args(["-t", "-f", "DEVICE,STATE", "device", "status"]).output() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() >= 2 {
                    check.extra.push((parts[0].into(), parts[1].into()));
                }
            }
        }

        results.push(check);
    }

    // -- Telemetry Service --
    {
        let mut check = SystemCheck {
            name: "Telemetry Service".into(),
            status: CheckStatus::NotAvailable,
            detail: "Not running".into(),
            required: false,
            extra: vec![],
        };

        if let Ok(out) = Command::new("systemctl")
            .args(["is-active", "jcr1440-telemetry"])
            .output()
        {
            let active = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if active == "active" {
                check.status = CheckStatus::Ready;
                check.detail = "Running".into();
            } else {
                check.detail = format!("State: {}", active);
            }
            check.extra.push(("Service state".into(), active));
        }

        // Check if monitor state file exists
        if std::path::Path::new("/run/jcr1440-monitor.state").exists() {
            check.extra.push(("State file".into(), "Present".into()));
            if let Ok(text) = std::fs::read_to_string("/run/jcr1440-monitor.state") {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                    if let Some(overall) = val.get("overall").and_then(|v| v.as_str()) {
                        check.extra.push(("Monitor status".into(), overall.into()));
                    }
                    if let Some(session) = val.get("session_dir").and_then(|v| v.as_str()) {
                        check.extra.push(("Session".into(), session.into()));
                    }
                }
            }
        }

        results.push(check);
    }

    // -- Storage --
    {
        let mut check = SystemCheck {
            name: "Storage".into(),
            status: CheckStatus::Ready,
            detail: "Healthy".into(),
            required: false,
            extra: vec![],
        };

        if let Ok(out) = Command::new("df").args(["-h", "/"]).output() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines().skip(1) {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 5 {
                    check.detail = format!("{} free of {}", parts[3], parts[1]);
                    check.extra.push(("Total".into(), parts[1].into()));
                    check.extra.push(("Free".into(), parts[3].into()));

                    let pct_str = parts[4].trim_end_matches('%');
                    if let Ok(pct) = pct_str.parse::<u32>() {
                        if pct > 95 {
                            check.status = CheckStatus::Failed;
                            check.detail = format!("Critical: {}% full", pct);
                        } else if pct > 85 {
                            check.status = CheckStatus::Warning;
                            check.detail = format!("Low space: {}% used", pct);
                        }
                    }
                }
            }
        }

        // Check log directory
        if let Ok(entries) = std::fs::read_dir("/var/log/jcr1440") {
            let count = entries.count();
            check.extra.push(("Log sessions".into(), format!("{}", count)));
        }

        results.push(check);
    }

    results
}

fn glob_paths(pattern: &str) -> Vec<std::path::PathBuf> {
    // Simple glob using the pattern directly
    let dir = std::path::Path::new(pattern).parent().unwrap_or(std::path::Path::new("/dev"));
    let prefix = std::path::Path::new(pattern).file_name()
        .unwrap_or_default().to_string_lossy();
    let prefix = prefix.trim_end_matches('*');

    std::fs::read_dir(dir).into_iter().flatten().filter_map(|e| {
        let p = e.ok()?.path();
        if p.file_name()?.to_string_lossy().starts_with(prefix) {
            Some(p)
        } else {
            None
        }
    }).collect()
}

// ---------------------------------------------------------------------------
// UI drawing
// ---------------------------------------------------------------------------

/// Draw the full preflight screen.
/// Returns (start_pressed, selected_cluster_idx).
pub fn draw_preflight(
    ui: &mut Ui,
    state: &mut PreflightState,
    renderers: &[Box<dyn ClusterRenderer>],
) -> (bool, Option<usize>) {
    let mut start_pressed = false;
    let avail = ui.available_rect_before_wrap();
    let w = avail.width();
    let h = avail.height();

    // Background
    let painter = ui.painter_at(avail);
    painter.rect_filled(avail, 0.0, BG_DARK);

    // Header
    let header_h = h * 0.12;
    let header_rect = Rect::from_min_size(avail.min, Vec2::new(w, header_h));
    draw_header(&painter, header_rect);

    // Check rows
    let checks_top = avail.top() + header_h;
    let checks_h = h * 0.55;
    let row_h = checks_h / state.checks.len().max(1) as f32;

    for (i, check) in state.checks.iter().enumerate() {
        let row_rect = Rect::from_min_size(
            Pos2::new(avail.left() + w * 0.05, checks_top + i as f32 * row_h),
            Vec2::new(w * 0.90, row_h * 0.85),
        );
        draw_check_row(&painter, row_rect, check);
    }

    // Handle click on check rows for expansion
    if ui.input(|i| i.pointer.any_pressed()) {
        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
            for (i, _check) in state.checks.iter().enumerate() {
                let row_rect = Rect::from_min_size(
                    Pos2::new(avail.left() + w * 0.05, checks_top + i as f32 * row_h),
                    Vec2::new(w * 0.90, row_h * 0.85),
                );
                if row_rect.contains(pos) {
                    if state.expanded_check == Some(i) {
                        state.expanded_check = None;
                    } else {
                        state.expanded_check = Some(i);
                    }
                }
            }
        }
    }

    // Expanded detail panel (overlay)
    if let Some(idx) = state.expanded_check {
        if idx < state.checks.len() {
            let panel_rect = Rect::from_center_size(
                avail.center(),
                Vec2::new(w * 0.70, h * 0.55),
            );
            draw_detail_panel(&painter, panel_rect, &state.checks[idx]);

            // Close on click outside
            if ui.input(|i| i.pointer.any_pressed()) {
                if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                    if !panel_rect.contains(pos) {
                        state.expanded_check = None;
                    }
                }
            }
        }
    }

    // Overall status bar
    let status_y = checks_top + checks_h;
    let status_rect = Rect::from_min_size(
        Pos2::new(avail.left() + w * 0.05, status_y),
        Vec2::new(w * 0.90, h * 0.06),
    );
    draw_overall_status(&painter, status_rect, &state.overall, &state.status_message);

    // Cluster selector row
    let selector_y = status_y + h * 0.07;
    if !renderers.is_empty() {
        let sel_name = renderers.get(state.selected_cluster_idx)
            .map(|r| r.name()).unwrap_or("---");
        let sel_rect = Rect::from_min_size(
            Pos2::new(avail.left() + w * 0.05, selector_y),
            Vec2::new(w * 0.90, h * 0.06),
        );
        painter.rect_filled(sel_rect, 4.0, CARD_BG);
        painter.rect_stroke(sel_rect, 4.0, Stroke::new(1.0_f32, CARD_BORDER), StrokeKind::Outside);
        painter.text(
            Pos2::new(sel_rect.left() + 15.0, sel_rect.center().y),
            Align2::LEFT_CENTER,
            "Selected Cluster",
            FontId::proportional(sel_rect.height() * 0.30),
            TEXT_DIM,
        );
        painter.text(
            Pos2::new(sel_rect.right() - 30.0, sel_rect.center().y),
            Align2::RIGHT_CENTER,
            sel_name,
            FontId::proportional(sel_rect.height() * 0.34),
            TITLE_COLOR,
        );
        painter.text(
            Pos2::new(sel_rect.right() - 10.0, sel_rect.center().y),
            Align2::RIGHT_CENTER,
            ">",
            FontId::proportional(sel_rect.height() * 0.30),
            TEXT_DIM,
        );

        // Click to cycle
        if ui.input(|i| i.pointer.any_released()) {
            if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                if sel_rect.contains(pos) {
                    state.selected_cluster_idx =
                        (state.selected_cluster_idx + 1) % renderers.len();
                }
            }
        }
    }

    // Start button
    let btn_y = selector_y + h * 0.08;
    let btn_rect = Rect::from_center_size(
        Pos2::new(avail.center().x, btn_y),
        Vec2::new(w * 0.40, h * 0.08),
    );

    let can_start = state.can_start();
    let hovered = ui.input(|i| {
        i.pointer.interact_pos().map_or(false, |p| btn_rect.contains(p))
    });

    let bg = if !can_start {
        BTN_BG_DISABLED
    } else if hovered {
        BTN_BG_HOVER
    } else {
        BTN_BG
    };
    let text_color = if can_start { BTN_TEXT } else { BTN_TEXT_DISABLED };

    painter.rect_filled(btn_rect, 6.0, bg);
    painter.text(
        btn_rect.center(),
        Align2::CENTER_CENTER,
        "START CLUSTER",
        FontId::proportional(h * 0.032),
        text_color,
    );

    if can_start && hovered && ui.input(|i| i.pointer.any_released()) {
        start_pressed = true;
    }

    // Ctrl+Alt+D toggles dev panel
    if ui.input(|i| {
        i.key_pressed(egui::Key::D) && i.modifiers.ctrl && i.modifiers.alt
    }) {
        state.dev_panel_open = !state.dev_panel_open;
    }

    // Enter to start
    if can_start && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        start_pressed = true;
    }

    let cluster_idx = if start_pressed { Some(state.selected_cluster_idx) } else { None };
    (start_pressed, cluster_idx)
}

fn draw_header(painter: &Painter, rect: Rect) {
    // NinoDash wordmark
    painter.text(
        Pos2::new(rect.left() + 30.0, rect.center().y - 8.0),
        Align2::LEFT_CENTER,
        "Nino",
        FontId::proportional(rect.height() * 0.36),
        TEXT_WHITE,
    );
    painter.text(
        Pos2::new(rect.left() + 92.0, rect.center().y - 8.0),
        Align2::LEFT_CENTER,
        "Dash",
        FontId::proportional(rect.height() * 0.36),
        TITLE_COLOR,
    );

    // Subtitle
    painter.text(
        Pos2::new(rect.left() + 30.0, rect.center().y + 16.0),
        Align2::LEFT_CENTER,
        "SYSTEM PREFLIGHT",
        FontId::proportional(rect.height() * 0.18),
        SUBTITLE_COLOR,
    );

    // Clock (top-right)
    let now = chrono_time();
    painter.text(
        Pos2::new(rect.right() - 30.0, rect.center().y),
        Align2::RIGHT_CENTER,
        &now,
        FontId::proportional(rect.height() * 0.22),
        CLOCK_COLOR,
    );

    // Separator line
    painter.line_segment(
        [
            Pos2::new(rect.left() + 20.0, rect.bottom()),
            Pos2::new(rect.right() - 20.0, rect.bottom()),
        ],
        Stroke::new(1.0_f32, CARD_BORDER),
    );
}

fn draw_check_row(painter: &Painter, rect: Rect, check: &SystemCheck) {
    let h = rect.height();
    let status_color = check.status.color();

    // Background card
    painter.rect_filled(rect, 4.0, CARD_BG);
    painter.rect_stroke(rect, 4.0, Stroke::new(1.0_f32, CARD_BORDER), StrokeKind::Outside);

    // Left accent bar
    let accent_rect = Rect::from_min_size(rect.min, Vec2::new(3.0, rect.height()));
    painter.rect_filled(accent_rect, 2.0, status_color);

    // Status icon
    let icon_x = rect.left() + 28.0;
    painter.text(
        Pos2::new(icon_x, rect.center().y),
        Align2::CENTER_CENTER,
        check.status.icon(),
        FontId::monospace(h * 0.32),
        status_color,
    );

    // Check name
    let name_x = rect.left() + 60.0;
    painter.text(
        Pos2::new(name_x, rect.center().y),
        Align2::LEFT_CENTER,
        &check.name,
        FontId::proportional(h * 0.38),
        TEXT_WHITE,
    );

    // Status label (right-aligned)
    painter.text(
        Pos2::new(rect.right() - 15.0, rect.center().y - h * 0.08),
        Align2::RIGHT_CENTER,
        check.status.label(),
        FontId::proportional(h * 0.30),
        status_color,
    );

    // Detail text (right-aligned, smaller)
    if !check.detail.is_empty() {
        painter.text(
            Pos2::new(rect.right() - 15.0, rect.center().y + h * 0.22),
            Align2::RIGHT_CENTER,
            &check.detail,
            FontId::proportional(h * 0.18),
            TEXT_DIM,
        );
    }

    // Required indicator
    if check.required {
        painter.text(
            Pos2::new(name_x, rect.center().y + h * 0.25),
            Align2::LEFT_CENTER,
            "REQUIRED",
            FontId::proportional(h * 0.14),
            TEXT_DIM,
        );
    }

    // Tap hint
    if !check.extra.is_empty() {
        painter.text(
            Pos2::new(rect.right() - 15.0, rect.bottom() - 2.0),
            Align2::RIGHT_BOTTOM,
            "tap for details",
            FontId::proportional(h * 0.12),
            Color32::from_rgb(50, 52, 58),
        );
    }
}

fn draw_detail_panel(painter: &Painter, rect: Rect, check: &SystemCheck) {
    // Dim background
    let full = painter.clip_rect();
    painter.rect_filled(full, 0.0, Color32::from_rgba_premultiplied(0, 0, 0, 180));

    // Panel
    painter.rect_filled(rect, 8.0, DETAIL_BG);
    painter.rect_stroke(rect, 8.0, Stroke::new(1.0_f32, check.status.color()), StrokeKind::Outside);

    // Title
    painter.text(
        Pos2::new(rect.left() + 20.0, rect.top() + 25.0),
        Align2::LEFT_CENTER,
        &check.name,
        FontId::proportional(20.0),
        TEXT_WHITE,
    );

    // Status
    painter.text(
        Pos2::new(rect.right() - 20.0, rect.top() + 25.0),
        Align2::RIGHT_CENTER,
        check.status.label(),
        FontId::proportional(16.0),
        check.status.color(),
    );

    // Separator
    painter.line_segment(
        [
            Pos2::new(rect.left() + 15.0, rect.top() + 45.0),
            Pos2::new(rect.right() - 15.0, rect.top() + 45.0),
        ],
        Stroke::new(1.0_f32, CARD_BORDER),
    );

    // Detail rows
    let row_h = 28.0;
    let start_y = rect.top() + 60.0;
    let max_rows = ((rect.height() - 80.0) / row_h) as usize;

    for (i, (key, val)) in check.extra.iter().take(max_rows).enumerate() {
        let y = start_y + i as f32 * row_h;
        painter.text(
            Pos2::new(rect.left() + 25.0, y),
            Align2::LEFT_CENTER,
            key,
            FontId::proportional(13.0),
            SUBTITLE_COLOR,
        );
        painter.text(
            Pos2::new(rect.right() - 25.0, y),
            Align2::RIGHT_CENTER,
            val,
            FontId::proportional(13.0),
            TEXT_WHITE,
        );
    }
}

fn draw_overall_status(
    painter: &Painter, rect: Rect,
    overall: &OverallStatus, message: &str,
) {
    let cx = rect.center().x;

    // Status label
    painter.text(
        Pos2::new(cx, rect.top() + rect.height() * 0.30),
        Align2::CENTER_CENTER,
        overall.label(),
        FontId::proportional(rect.height() * 0.32),
        overall.color(),
    );

    // Message
    painter.text(
        Pos2::new(cx, rect.top() + rect.height() * 0.70),
        Align2::CENTER_CENTER,
        message,
        FontId::proportional(rect.height() * 0.16),
        TEXT_DIM,
    );
}

pub fn chrono_time_internal() -> String {
    chrono_time()
}

fn chrono_time() -> String {
    // Use simple system time formatting without chrono dependency
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    // UTC time parts
    let hours = ((secs % 86400) / 3600) as u32;
    let mins = ((secs % 3600) / 60) as u32;

    // Adjust for IST (+5:30)
    let ist_mins = mins + 30;
    let carry = ist_mins / 60;
    let ist_mins = ist_mins % 60;
    let ist_hours = (hours + 5 + carry) % 24;

    format!("{:02}:{:02}", ist_hours, ist_mins)
}
