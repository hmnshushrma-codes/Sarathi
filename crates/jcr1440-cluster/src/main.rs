// NinoDash — Performance Digital Cluster.
//
// Boot flow:
//   1. Branded splash animation (~3s)
//   2. Preflight screen validates system health
//   3. User presses "Start Cluster"
//   4. Selected cluster layout activates fullscreen
//
// Architecture: tokio poller → watch channel → 60fps egui render
// One telemetry source, multiple cluster views.

use std::time::{Duration, Instant};

use clap::Parser;
use eframe::egui;
use egui::Rect;
use jcr1440_client::{DeviceConfig, DeviceState, ObdData, TelemetryFrame};
use tokio::sync::watch;

mod clusters;
mod gauges;
mod gps_logger;
mod journey;
mod preflight;
mod splash;
mod switcher;
mod theme;
mod trip_db;

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "ninodash", about = "NinoDash — Performance Digital Cluster")]
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
    /// Skip splash and preflight
    #[arg(long)]
    skip_preflight: bool,
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

    fn to_layout(
        &self, frame: &Option<TelemetryFrame>,
        connected: bool, error_msg: &Option<String>,
        journey: &journey::JourneyRecorder,
    ) -> gauges::ClusterLayout {
        gauges::ClusterLayout {
            rpm: self.rpm,
            speed: self.speed,
            coolant_temp: self.coolant_temp,
            intake_temp: self.intake_temp,
            oil_temp: self.oil_temp,
            voltage: self.voltage,
            throttle: self.throttle,
            manifold_pressure: self.manifold_pressure,
            fuel_rate: self.fuel_rate,
            fuel_level: self.fuel_level,
            maf: self.maf,
            gear: self.estimated_gear(),
            gps: frame.as_ref().map(|f| f.gps.clone()),
            dtc_count: frame.as_ref().and_then(|f| f.obd.dtc_count).unwrap_or(0),
            connected,
            error_msg: error_msg.clone(),
            redline_intensity: 0.0,
            speed_source: journey.speed_source.label(),
            gps_speed: journey.gps_speed,
            obd_speed: journey.obd_speed,
            trip_active: journey.is_trip_active(),
            trip_distance_km: journey.trip_distance(),
            trip_points: journey.trip_points(),
        }
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

// ---------------------------------------------------------------------------
// Screen state machine
// ---------------------------------------------------------------------------

#[derive(PartialEq)]
enum Screen {
    Splash,
    Preflight,
    Cluster,
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

struct NinoDashApp {
    rx: watch::Receiver<DeviceState>,
    screen: Screen,
    // Splash
    splash: splash::SplashState,
    // Preflight
    preflight: preflight::PreflightState,
    // Cluster
    renderers: Vec<Box<dyn clusters::ClusterRenderer>>,
    active_cluster: usize,
    switcher: switcher::SwitcherState,
    gauges: SmoothedGauges,
    last_frame: Option<TelemetryFrame>,
    connected: bool,
    error_msg: Option<String>,
    // Data loss recovery
    data_lost_since: Option<Instant>,
    data_restored_at: Option<Instant>,
    // GPS track logger (GPX files)
    gps_logger: gps_logger::GpsLogger,
    // Journey recorder (SQLite trips + auto trip detection)
    journey: journey::JourneyRecorder,
}

impl NinoDashApp {
    fn new(rx: watch::Receiver<DeviceState>, skip_preflight: bool) -> Self {
        let renderers = clusters::all_renderers();
        let saved_id = clusters::load_selected_cluster();
        let active_idx = renderers.iter()
            .position(|r| r.id() == saved_id)
            .unwrap_or(0);

        let mut preflight = preflight::PreflightState::new();
        preflight.start_checks();
        preflight.selected_cluster_idx = active_idx;

        let screen = if skip_preflight { Screen::Cluster } else { Screen::Splash };

        Self {
            rx,
            screen,
            splash: splash::SplashState::new(),
            preflight,
            renderers,
            active_cluster: active_idx,
            switcher: switcher::SwitcherState::new(active_idx),
            gauges: SmoothedGauges::default(),
            last_frame: None,
            connected: false,
            error_msg: None,
            data_lost_since: None,
            data_restored_at: None,
            gps_logger: gps_logger::GpsLogger::new(),
            journey: journey::JourneyRecorder::new(),
        }
    }

    fn update_telemetry(&mut self) {
        let state = self.rx.borrow().clone();
        match state {
            DeviceState::Live(frame) => {
                self.gauges.update(&frame.obd);
                // Journey recorder (SQLite + trip detection + speed source)
                self.journey.update(&frame.gps, &frame.obd);
                // GPX track logger (backup)
                self.gps_logger.log_point(&frame.gps);
                if !self.connected {
                    self.data_restored_at = Some(Instant::now());
                    self.data_lost_since = None;
                }
                self.last_frame = Some(frame);
                self.connected = true;
                self.error_msg = None;
            }
            DeviceState::Error { message, .. } => {
                if self.connected && self.data_lost_since.is_none() {
                    self.data_lost_since = Some(Instant::now());
                    self.data_restored_at = None;
                }
                self.connected = false;
                self.error_msg = Some(message);
                self.gauges.update(&ObdData::default());
            }
            DeviceState::Connecting => {
                self.connected = false;
                self.error_msg = Some("Connecting...".into());
            }
            DeviceState::Disconnected => {
                if self.connected && self.data_lost_since.is_none() {
                    self.data_lost_since = Some(Instant::now());
                }
                self.connected = false;
                self.error_msg = Some("Disconnected".into());
            }
        }
    }

    fn set_active_cluster(&mut self, idx: usize) {
        if idx < self.renderers.len() {
            self.active_cluster = idx;
            self.switcher.selected_idx = idx;
            clusters::save_selected_cluster(self.renderers[idx].id());
        }
    }

    fn draw_cluster_screen(&mut self, ctx: &egui::Context) {
        self.update_telemetry();

        let layout = self.gauges.to_layout(
            &self.last_frame, self.connected, &self.error_msg, &self.journey);

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG_BLACK))
            .show(ctx, |ui| {
                let rect = ui.available_rect_before_wrap();
                let painter = ui.painter_at(rect);

                // Draw active cluster
                if let Some(renderer) = self.renderers.get(self.active_cluster) {
                    renderer.draw(&painter, rect, &layout);
                }

                // Data restored flash
                if let Some(restored) = self.data_restored_at {
                    if restored.elapsed() < Duration::from_secs(3) {
                        let banner_h = 36.0;
                        let banner = Rect::from_min_size(rect.min,
                            egui::Vec2::new(rect.width(), banner_h));
                        painter.rect_filled(banner, 0.0,
                            egui::Color32::from_rgba_premultiplied(255, 98, 0, 180));
                        painter.text(banner.center(), egui::Align2::CENTER_CENTER,
                            "VEHICLE DATA RESTORED",
                            egui::FontId::proportional(14.0), theme::TEXT_PRIMARY);
                    } else {
                        self.data_restored_at = None;
                    }
                }

                // Quick switch bar
                if !self.switcher.overlay_open {
                    let action = switcher::draw_quick_switch(
                        ui, &painter, rect, &self.switcher, &self.renderers);
                    match action {
                        switcher::QuickSwitchAction::Prev => {
                            let n = self.renderers.len();
                            let idx = (self.active_cluster + n - 1) % n;
                            self.set_active_cluster(idx);
                            self.switcher.touch();
                        }
                        switcher::QuickSwitchAction::Next => {
                            let idx = (self.active_cluster + 1) % self.renderers.len();
                            self.set_active_cluster(idx);
                            self.switcher.touch();
                        }
                        switcher::QuickSwitchAction::OpenOverlay => {
                            self.switcher.overlay_open = true;
                        }
                        switcher::QuickSwitchAction::None => {}
                    }
                }

                // Full selector overlay
                if self.switcher.overlay_open {
                    let action = switcher::draw_selector_overlay(
                        ui, &painter, rect, self.active_cluster, &self.renderers);
                    match action {
                        switcher::SelectorAction::Select(idx) => {
                            self.set_active_cluster(idx);
                            self.switcher.overlay_open = false;
                            self.switcher.touch();
                        }
                        switcher::SelectorAction::Close => {
                            self.switcher.overlay_open = false;
                        }
                        switcher::SelectorAction::None => {}
                    }
                }

                // Show quick switch on any pointer movement
                if ui.input(|i| i.pointer.is_moving()) {
                    self.switcher.touch();
                }

                // Ctrl+Alt+Q → back to preflight
                if ui.input(|i| {
                    i.key_pressed(egui::Key::Q) && i.modifiers.ctrl && i.modifiers.alt
                }) {
                    self.screen = Screen::Preflight;
                    self.preflight = preflight::PreflightState::new();
                    self.preflight.start_checks();
                    self.preflight.selected_cluster_idx = self.active_cluster;
                }
            });
    }
}

impl eframe::App for NinoDashApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.set_visuals(egui::Visuals::dark());

        match self.screen {
            Screen::Splash => {
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(egui::Color32::from_rgb(2, 2, 2)))
                    .show(ctx, |ui| {
                        let rect = ui.available_rect_before_wrap();
                        let painter = ui.painter_at(rect);
                        if splash::draw_splash(ctx, &painter, rect, &mut self.splash) {
                            self.screen = Screen::Preflight;
                        }
                    });
            }
            Screen::Preflight => {
                self.preflight.update(&self.rx);

                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(theme::BG_BLACK))
                    .show(ctx, |ui| {
                        let (start, cluster_idx) = preflight::draw_preflight(
                            ui, &mut self.preflight, &self.renderers);
                        if start {
                            if let Some(idx) = cluster_idx {
                                self.set_active_cluster(idx);
                            }
                            self.screen = Screen::Cluster;
                            self.switcher.touch();
                        }
                    });
            }
            Screen::Cluster => {
                self.draw_cluster_screen(ctx);
            }
        }

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
            interface: cli.iface.clone(),
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
        "NinoDash",
        native_options,
        Box::new(move |cc| {
            // Load condensed italic font for racy feel
            let font_paths = [
                "/usr/share/fonts/truetype/liberation/LiberationSansNarrow-Italic.ttf",
                "/usr/share/fonts/truetype/liberation/LiberationSansNarrow-BoldItalic.ttf",
            ];
            let mut fonts = egui::FontDefinitions::default();
            for path in &font_paths {
                if let Ok(data) = std::fs::read(path) {
                    let name = if path.contains("Bold") { "bold-italic" } else { "italic" };
                    fonts.font_data.insert(
                        name.to_string(),
                        std::sync::Arc::new(egui::FontData::from_owned(data)),
                    );
                    // Put italic first in the proportional family so it becomes default
                    fonts.families.entry(egui::FontFamily::Proportional)
                        .or_default()
                        .insert(0, name.to_string());
                }
            }
            cc.egui_ctx.set_fonts(fonts);

            Ok(Box::new(NinoDashApp::new(rx, cli.skip_preflight)))
        }),
    )
    .expect("eframe run");
}
