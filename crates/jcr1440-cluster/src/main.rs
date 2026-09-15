// Haltech-inspired racing instrument cluster for JCR1440 OBD-II telemetry.
//
// Rendering: eframe/egui with glow backend (OpenGL ES — Pi 4/5 compatible).
// Architecture: tokio poller → watch channel → 60fps egui render with
// exponential smoothing between ~300ms data updates.

use std::time::Instant;

use clap::Parser;
use eframe::egui;
use jcr1440_client::{DeviceConfig, DeviceState, ObdData, TelemetryFrame};
use tokio::sync::watch;

mod gauges;

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "jcr1440-cluster", about = "Racing instrument cluster")]
struct Cli {
    #[arg(long)]
    mock: bool,
    #[arg(long, default_value = "192.168.1.1")]
    ip: String,
    #[arg(long, default_value = "enxfcde56ff0106")]
    iface: String,
    #[arg(long)]
    fullscreen: bool,
    #[arg(long, default_value = "1024")]
    width: u32,
    #[arg(long, default_value = "600")]
    height: u32,
}

// ---------------------------------------------------------------------------
// Smoothed gauge state
// ---------------------------------------------------------------------------

struct SmoothedGauges {
    rpm: f32,
    speed: f32,
    coolant_temp: f32,
    intake_temp: f32,
    oil_temp: f32,
    voltage: f32,
    throttle: f32,
    manifold_pressure: f32,
    fuel_rate: f32,
    fuel_level: f32,
    maf: f32,
    last_update: Instant,
}

impl Default for SmoothedGauges {
    fn default() -> Self {
        Self {
            rpm: 0.0, speed: 0.0, coolant_temp: 0.0, intake_temp: 0.0,
            oil_temp: 0.0, voltage: 0.0, throttle: 0.0, manifold_pressure: 0.0,
            fuel_rate: 0.0, fuel_level: 0.0, maf: 0.0,
            last_update: Instant::now(),
        }
    }
}

impl SmoothedGauges {
    fn update(&mut self, target: &ObdData) {
        let dt = self.last_update.elapsed().as_secs_f32();
        self.last_update = Instant::now();

        let fast = 1.0 - (-dt * 8.0).exp();
        let med = 1.0 - (-dt * 6.0).exp();
        let slow = 1.0 - (-dt * 2.0).exp();

        self.rpm = lerp(self.rpm, target.engine_rpm.unwrap_or(0.0), fast);
        self.speed = lerp(self.speed, target.vehicle_speed.unwrap_or(0.0), med);
        self.coolant_temp = lerp(self.coolant_temp, target.coolant_temp.unwrap_or(0.0), slow);
        self.intake_temp = lerp(self.intake_temp, target.intake_air_temp.unwrap_or(0.0), slow);
        self.oil_temp = lerp(self.oil_temp, target.oil_temp.unwrap_or(0.0), slow);
        self.voltage = lerp(self.voltage, target.battery_voltage.unwrap_or(0.0), slow);
        self.throttle = lerp(self.throttle, target.throttle_position.unwrap_or(0.0), med);
        self.manifold_pressure = lerp(self.manifold_pressure, target.manifold_pressure.unwrap_or(0.0), med);
        self.fuel_rate = lerp(self.fuel_rate, target.fuel_rate.unwrap_or(0.0), med);
        self.fuel_level = lerp(self.fuel_level, target.fuel_level.unwrap_or(0.0), slow);
        self.maf = lerp(self.maf, target.maf.unwrap_or(0.0), med);
    }

    fn estimated_gear(&self) -> u8 {
        if self.speed < 3.0 || self.rpm < 600.0 { return 0; }
        let ratio = self.rpm / self.speed;
        if ratio > 95.0 { 1 }
        else if ratio > 58.0 { 2 }
        else if ratio > 40.0 { 3 }
        else if ratio > 30.0 { 4 }
        else if ratio > 24.0 { 5 }
        else { 6 }
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

struct ClusterApp {
    rx: watch::Receiver<DeviceState>,
    gauges: SmoothedGauges,
    last_frame: Option<TelemetryFrame>,
    connected: bool,
    error_msg: Option<String>,
}

impl ClusterApp {
    fn new(rx: watch::Receiver<DeviceState>) -> Self {
        Self { rx, gauges: SmoothedGauges::default(), last_frame: None, connected: false, error_msg: None }
    }
}

impl eframe::App for ClusterApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let state = self.rx.borrow().clone();
        match state {
            DeviceState::Live(frame) => {
                self.gauges.update(&frame.obd);
                self.last_frame = Some(frame);
                self.connected = true;
                self.error_msg = None;
            }
            DeviceState::Error { message, .. } => {
                self.connected = false;
                self.error_msg = Some(message);
                self.gauges.update(&ObdData::default());
            }
            DeviceState::Connecting => {
                self.connected = false;
                self.error_msg = Some("Connecting...".into());
            }
            DeviceState::Disconnected => {
                self.connected = false;
                self.error_msg = Some("Disconnected".into());
            }
        }

        // Background: pulse red near redline
        let redline = 7000.0_f32;
        let rpm = self.gauges.rpm;
        let bg = if rpm > redline * 0.85 {
            let intensity = ((rpm - redline * 0.85) / (redline * 0.15)).clamp(0.0, 1.0);
            let flash = if rpm > redline {
                (ctx.input(|i| i.time) as f32 * 12.0).sin().abs() * 0.4
            } else { 0.0 };
            let r = (8.0 + (70.0 + flash * 70.0) * intensity) as u8;
            let g = (8.0 * (1.0 - intensity * 0.7)) as u8;
            let b = (10.0 * (1.0 - intensity * 0.8)) as u8;
            egui::Color32::from_rgb(r, g, b)
        } else {
            egui::Color32::from_rgb(8, 8, 10)
        };

        ctx.set_visuals(egui::Visuals::dark());

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(bg))
            .show(ctx, |ui| {
                let rect = ui.available_rect_before_wrap();
                let painter = ui.painter_at(rect);

                let layout = gauges::ClusterLayout {
                    rpm: self.gauges.rpm,
                    speed: self.gauges.speed,
                    coolant_temp: self.gauges.coolant_temp,
                    intake_temp: self.gauges.intake_temp,
                    oil_temp: self.gauges.oil_temp,
                    voltage: self.gauges.voltage,
                    throttle: self.gauges.throttle,
                    manifold_pressure: self.gauges.manifold_pressure,
                    fuel_rate: self.gauges.fuel_rate,
                    fuel_level: self.gauges.fuel_level,
                    maf: self.gauges.maf,
                    gear: self.gauges.estimated_gear(),
                    gps: self.last_frame.as_ref().map(|f| f.gps.clone()),
                    dtc_count: self.last_frame.as_ref().and_then(|f| f.obd.dtc_count).unwrap_or(0),
                    connected: self.connected,
                    error_msg: self.error_msg.clone(),
                    redline_intensity: 0.0,
                };

                gauges::draw_full_cluster(&painter, rect, &layout);
            });

        ctx.request_repaint();
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter("jcr1440_client=debug,jcr1440_cluster=info")
        .init();

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let _guard = rt.enter();

    let rx = if cli.mock {
        tracing::info!("Using mock data source");
        jcr1440_client::spawn_mock_poller()
    } else {
        let config = DeviceConfig {
            base_url: format!("http://{}", cli.ip),
            ..Default::default()
        };
        tracing::info!("Connecting to JCR1440 at {}", cli.ip);
        jcr1440_client::spawn_poller(config)
    };

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([cli.width as f32, cli.height as f32])
            .with_fullscreen(cli.fullscreen)
            .with_decorations(!cli.fullscreen),
        ..Default::default()
    };

    eframe::run_native(
        "JCR1440 Cluster",
        native_options,
        Box::new(|_cc| Ok(Box::new(ClusterApp::new(rx)))),
    )
    .expect("eframe run");
}
