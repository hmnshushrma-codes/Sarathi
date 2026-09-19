//! Animated automotive preflight diagnostic screen.
//!
//! All animations are time-based (not frame-coupled), targeting 60fps.
//! Health checks run in background threads; the UI only reads the latest state.
//! Progress indicator, staggered row appearance, pulsing status dots.

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
// Colors
// ---------------------------------------------------------------------------

const BG: Color32 = Color32::from_rgb(5, 5, 5);
const PANEL_BG: Color32 = Color32::from_rgb(14, 14, 17);
const ROW_BG: Color32 = Color32::from_rgb(17, 17, 20);
const ROW_BORDER: Color32 = Color32::from_rgb(30, 30, 34);
const SEPARATOR: Color32 = Color32::from_rgb(30, 30, 34);
const TEXT_PRIMARY: Color32 = Color32::from_rgb(244, 244, 244);
const TEXT_SECONDARY: Color32 = Color32::from_rgb(184, 184, 184);
const TEXT_MUTED: Color32 = Color32::from_rgb(96, 96, 96);
const ORANGE: Color32 = Color32::from_rgb(255, 90, 0);
const ORANGE_HOVER: Color32 = Color32::from_rgb(255, 107, 0);
const GREEN: Color32 = Color32::from_rgb(30, 200, 80);
const RED: Color32 = Color32::from_rgb(227, 24, 24);
const AMBER: Color32 = Color32::from_rgb(255, 160, 0);
const CHECKING_COLOR: Color32 = Color32::from_rgb(120, 120, 125);
const BTN_DISABLED_BG: Color32 = Color32::from_rgb(28, 28, 32);
const BTN_DISABLED_TEXT: Color32 = Color32::from_rgb(70, 70, 75);
const DETAIL_BG: Color32 = Color32::from_rgb(10, 10, 14);
const PROGRESS_BG: Color32 = Color32::from_rgb(25, 25, 28);

// ---------------------------------------------------------------------------
// Animation constants
// ---------------------------------------------------------------------------

const ROW_STAGGER_DELAY: f32 = 0.08;  // seconds between row appearances
const ROW_FADE_DURATION: f32 = 0.3;
const STATUS_TRANSITION: f32 = 0.4;
const PULSE_SPEED: f32 = 3.0;         // Hz for checking pulse
const PROGRESS_LERP_SPEED: f32 = 6.0;
const DOT_RADIUS: f32 = 4.5;

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

    fn symbol(&self) -> &'static str {
        match self {
            Self::Checking => "",  // animated dot
            Self::Ready => "OK",
            Self::Connected => "OK",
            Self::Receiving => "OK",
            Self::Warning => "!",
            Self::Failed => "X",
            Self::NotAvailable => "X",
        }
    }

    fn color(&self) -> Color32 {
        match self {
            Self::Checking => CHECKING_COLOR,
            Self::Ready | Self::Connected | Self::Receiving => GREEN,
            Self::Warning => AMBER,
            Self::Failed | Self::NotAvailable => RED,
        }
    }

    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ready | Self::Connected | Self::Receiving)
    }

    pub fn is_done(&self) -> bool {
        !matches!(self, Self::Checking)
    }
}

// ---------------------------------------------------------------------------
// Data
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SystemCheck {
    pub name: String,
    pub status: CheckStatus,
    pub detail: String,
    pub required: bool,
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
            Self::Checking => "SYSTEM CHECK",
            Self::Ready => "READY",
            Self::ReadyWithWarnings => "READY WITH WARNINGS",
            Self::NotReady => "NOT READY",
        }
    }

    fn color(&self) -> Color32 {
        match self {
            Self::Checking => CHECKING_COLOR,
            Self::Ready => GREEN,
            Self::ReadyWithWarnings => AMBER,
            Self::NotReady => RED,
        }
    }
}

// ---------------------------------------------------------------------------
// Animation state per row
// ---------------------------------------------------------------------------

struct RowAnim {
    appear_time: f32,         // elapsed seconds when row should appear
    status_changed_at: f32,   // elapsed seconds when status last changed
    last_status: CheckStatus,
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
    pub selected_cluster_idx: usize,
    bg_rx: Option<mpsc::Receiver<Vec<SystemCheck>>>,
    last_device_state: DeviceState,
    last_telemetry_update: Instant,
    telemetry_packets: u64,
    // Animation
    row_anims: Vec<RowAnim>,
    displayed_progress: f32,
}

impl PreflightState {
    pub fn new() -> Self {
        let checks = vec![
            SystemCheck { name: "Raspberry Pi System".into(), status: CheckStatus::Checking, detail: String::new(), required: true, extra: vec![] },
            SystemCheck { name: "JCR1440 USB Device".into(), status: CheckStatus::Checking, detail: String::new(), required: true, extra: vec![] },
            SystemCheck { name: "OBD Interface".into(), status: CheckStatus::Checking, detail: String::new(), required: true, extra: vec![] },
            SystemCheck { name: "Vehicle Data Stream".into(), status: CheckStatus::Checking, detail: String::new(), required: true, extra: vec![] },
            SystemCheck { name: "GPS".into(), status: CheckStatus::Checking, detail: String::new(), required: false, extra: vec![] },
            SystemCheck { name: "Network".into(), status: CheckStatus::Checking, detail: String::new(), required: false, extra: vec![] },
            SystemCheck { name: "Telemetry Service".into(), status: CheckStatus::Checking, detail: String::new(), required: false, extra: vec![] },
            SystemCheck { name: "Storage".into(), status: CheckStatus::Checking, detail: String::new(), required: false, extra: vec![] },
        ];
        let num = checks.len();
        let row_anims = (0..num).map(|i| RowAnim {
            appear_time: 0.3 + i as f32 * ROW_STAGGER_DELAY,
            status_changed_at: 0.0,
            last_status: CheckStatus::Checking,
        }).collect();

        Self {
            checks,
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
            row_anims,
            displayed_progress: 0.0,
        }
    }

    pub fn start_checks(&mut self) {
        let (tx, rx) = mpsc::channel();
        self.bg_rx = Some(rx);
        std::thread::spawn(move || {
            let checks = run_system_checks();
            let _ = tx.send(checks);
        });
    }

    fn elapsed(&self) -> f32 {
        self.started_at.elapsed().as_secs_f32()
    }

    pub fn update(&mut self, device_rx: &watch::Receiver<DeviceState>) {
        let t = self.elapsed();

        // Poll background system checks
        if let Some(ref rx) = self.bg_rx {
            if let Ok(system_checks) = rx.try_recv() {
                for sc in &system_checks {
                    if let Some((idx, check)) = self.checks.iter_mut().enumerate()
                        .find(|(_, c)| c.name == sc.name)
                    {
                        if check.status != sc.status {
                            if idx < self.row_anims.len() {
                                self.row_anims[idx].status_changed_at = t;
                                self.row_anims[idx].last_status = check.status;
                            }
                        }
                        check.status = sc.status;
                        check.detail = sc.detail.clone();
                        check.extra = sc.extra.clone();
                    }
                }
                self.bg_rx = None;
            }
        }

        // Update from telemetry poller
        let state = device_rx.borrow().clone();
        match &state {
            DeviceState::Live(frame) => {
                self.telemetry_packets += 1;
                self.last_telemetry_update = Instant::now();

                // JCR1440 USB
                self.update_check("JCR1440 USB Device", CheckStatus::Connected,
                    "Device responding", t);

                // OBD Interface
                let has_obd = frame.obd.engine_rpm.is_some()
                    || frame.obd.vehicle_speed.is_some()
                    || frame.obd.battery_voltage.is_some();
                if has_obd {
                    self.update_check("OBD Interface", CheckStatus::Connected,
                        "OBD data available", t);
                } else if frame.device_connected {
                    self.update_check("OBD Interface", CheckStatus::Warning,
                        "Device connected, no OBD data", t);
                }

                // Vehicle Data Stream
                let has_data = frame.obd.engine_rpm.is_some()
                    || frame.obd.vehicle_speed.is_some();
                if has_data {
                    self.update_check("Vehicle Data Stream", CheckStatus::Receiving,
                        &format!("{} packets", self.telemetry_packets), t);
                } else {
                    self.update_check("Vehicle Data Stream", CheckStatus::Warning,
                        "Waiting for vehicle data...", t);
                }

                // GPS
                if frame.gps.fix_valid && frame.gps.satellites > 0 {
                    self.update_check("GPS", CheckStatus::Ready,
                        &format!("Fix: {} sats, HDOP {:.1}", frame.gps.satellites, frame.gps.hdop), t);
                } else if frame.gps.satellites > 0 {
                    self.update_check("GPS", CheckStatus::Ready,
                        &format!("Active: {} sats, no fix", frame.gps.satellites), t);
                } else {
                    self.update_check("GPS", CheckStatus::Warning, "No GPS signal", t);
                }
            }
            DeviceState::Connecting => {
                self.update_check("JCR1440 USB Device", CheckStatus::Checking,
                    "Connecting...", t);
            }
            DeviceState::Error { message, failures } => {
                if *failures > 5 {
                    self.update_check("JCR1440 USB Device", CheckStatus::Failed,
                        &format!("{} ({}x)", message, failures), t);
                }
            }
            DeviceState::Disconnected => {
                if self.elapsed() > 10.0 {
                    self.update_check("JCR1440 USB Device", CheckStatus::Failed,
                        "Not detected", t);
                }
            }
        }
        self.last_device_state = state;

        // Overall status
        let done_count = self.checks.iter().filter(|c| c.status.is_done()).count();
        let total = self.checks.len();
        let all_done = done_count == total;
        let required_failed = self.checks.iter()
            .filter(|c| c.required)
            .any(|c| matches!(c.status, CheckStatus::Failed | CheckStatus::NotAvailable));
        let any_warning = self.checks.iter()
            .any(|c| matches!(c.status, CheckStatus::Warning));

        if !all_done && self.elapsed() < 15.0 {
            self.overall = OverallStatus::Checking;
            self.status_message = format!("{} / {} checks complete", done_count, total);
        } else if required_failed {
            self.overall = OverallStatus::NotReady;
            let failed: Vec<_> = self.checks.iter()
                .filter(|c| c.required && !c.status.is_ok())
                .map(|c| c.name.as_str()).collect();
            self.status_message = failed.join(", ");
        } else if any_warning {
            self.overall = OverallStatus::ReadyWithWarnings;
            let warns: Vec<_> = self.checks.iter()
                .filter(|c| matches!(c.status, CheckStatus::Warning))
                .map(|c| c.name.as_str()).collect();
            self.status_message = warns.join(", ");
        } else {
            self.overall = OverallStatus::Ready;
            self.status_message = "All critical systems operational".into();
        }

        self.checks_complete = all_done || self.elapsed() > 15.0;

        // Smooth progress interpolation
        let target = done_count as f32 / total as f32;
        let dt = 1.0 / 60.0; // approximate
        self.displayed_progress += (target - self.displayed_progress) * (1.0 - (-PROGRESS_LERP_SPEED * dt).exp());
    }

    fn update_check(&mut self, name: &str, status: CheckStatus, detail: &str, t: f32) {
        if let Some((idx, check)) = self.checks.iter_mut().enumerate()
            .find(|(_, c)| c.name == name)
        {
            if check.status != status {
                if idx < self.row_anims.len() {
                    self.row_anims[idx].status_changed_at = t;
                    self.row_anims[idx].last_status = check.status;
                }
                check.status = status;
            }
            check.detail = detail.into();
        }
    }

    pub fn can_start(&self) -> bool {
        self.checks_complete && !matches!(self.overall, OverallStatus::Checking)
    }
}

// ---------------------------------------------------------------------------
// UI drawing
// ---------------------------------------------------------------------------

pub fn draw_preflight(
    ui: &mut Ui,
    state: &mut PreflightState,
    renderers: &[Box<dyn ClusterRenderer>],
) -> (bool, Option<usize>) {
    let mut start_pressed = false;
    let avail = ui.available_rect_before_wrap();
    let w = avail.width();
    let h = avail.height();
    let t = state.elapsed();
    let painter = ui.painter_at(avail);

    painter.rect_filled(avail, 0.0, BG);

    // === Header (8%) ===
    let header_h = h * 0.08;
    draw_header(&painter, Rect::from_min_size(avail.min, Vec2::new(w, header_h)), t);

    // === Progress bar (1.5%) ===
    let prog_y = avail.top() + header_h;
    let prog_h = h * 0.015;
    let prog_rect = Rect::from_min_size(
        Pos2::new(avail.left() + w * 0.05, prog_y),
        Vec2::new(w * 0.90, prog_h));
    painter.rect_filled(prog_rect, 2.0, PROGRESS_BG);
    let fill_w = prog_rect.width() * state.displayed_progress.clamp(0.0, 1.0);
    if fill_w > 0.5 {
        let fill = Rect::from_min_size(prog_rect.min, Vec2::new(fill_w, prog_h));
        painter.rect_filled(fill, 2.0, ORANGE);
    }

    // Progress text
    let done_count = state.checks.iter().filter(|c| c.status.is_done()).count();
    let total = state.checks.len();
    painter.text(
        Pos2::new(avail.right() - w * 0.05, prog_y + prog_h * 0.5),
        Align2::RIGHT_CENTER,
        &format!("{}/{}", done_count, total),
        FontId::proportional(prog_h * 2.5),
        TEXT_MUTED,
    );

    // === Check rows (52%) ===
    let rows_top = prog_y + prog_h + h * 0.01;
    let rows_h = h * 0.52;
    let row_h = rows_h / state.checks.len().max(1) as f32;
    let row_inner_h = row_h * 0.88;

    for (i, check) in state.checks.iter().enumerate() {
        let anim = &state.row_anims[i];
        // Staggered appearance
        let appear_alpha = ((t - anim.appear_time) / ROW_FADE_DURATION).clamp(0.0, 1.0);
        if appear_alpha <= 0.0 { continue; }

        let row_rect = Rect::from_min_size(
            Pos2::new(avail.left() + w * 0.04, rows_top + i as f32 * row_h),
            Vec2::new(w * 0.92, row_inner_h),
        );

        draw_animated_row(&painter, row_rect, check, anim, t, appear_alpha);
    }

    // Handle row clicks
    if ui.input(|i| i.pointer.any_pressed()) {
        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
            for i in 0..state.checks.len() {
                let row_rect = Rect::from_min_size(
                    Pos2::new(avail.left() + w * 0.04, rows_top + i as f32 * row_h),
                    Vec2::new(w * 0.92, row_inner_h),
                );
                if row_rect.contains(pos) {
                    state.expanded_check = if state.expanded_check == Some(i) { None } else { Some(i) };
                }
            }
        }
    }

    // Detail overlay
    if let Some(idx) = state.expanded_check {
        if idx < state.checks.len() {
            let panel_rect = Rect::from_center_size(
                avail.center(), Vec2::new(w * 0.70, h * 0.50));
            draw_detail_panel(&painter, panel_rect, &state.checks[idx]);
            if ui.input(|i| i.pointer.any_pressed()) {
                if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                    if !panel_rect.contains(pos) { state.expanded_check = None; }
                }
            }
        }
    }

    // === Overall status (5%) ===
    let status_y = rows_top + rows_h + h * 0.005;
    let status_h = h * 0.05;
    let cx = avail.center().x;
    painter.text(
        Pos2::new(cx, status_y + status_h * 0.35),
        Align2::CENTER_CENTER,
        state.overall.label(),
        FontId::proportional(status_h * 0.40),
        state.overall.color(),
    );
    painter.text(
        Pos2::new(cx, status_y + status_h * 0.75),
        Align2::CENTER_CENTER,
        &state.status_message,
        FontId::proportional(status_h * 0.22),
        TEXT_MUTED,
    );

    // === Cluster selector (5%) ===
    let sel_y = status_y + status_h + h * 0.005;
    let sel_h = h * 0.045;
    if !renderers.is_empty() {
        let sel_name = renderers.get(state.selected_cluster_idx)
            .map(|r| r.name()).unwrap_or("---");
        let sel_rect = Rect::from_min_size(
            Pos2::new(avail.left() + w * 0.04, sel_y),
            Vec2::new(w * 0.92, sel_h));
        painter.rect_filled(sel_rect, 4.0, ROW_BG);
        painter.rect_stroke(sel_rect, 4.0, Stroke::new(1.0_f32, ROW_BORDER), StrokeKind::Outside);
        painter.text(
            Pos2::new(sel_rect.left() + 15.0, sel_rect.center().y),
            Align2::LEFT_CENTER, "CLUSTER",
            FontId::proportional(sel_h * 0.30), TEXT_MUTED);
        painter.text(
            Pos2::new(sel_rect.right() - 25.0, sel_rect.center().y),
            Align2::RIGHT_CENTER, sel_name,
            FontId::proportional(sel_h * 0.34), ORANGE);
        painter.text(
            Pos2::new(sel_rect.right() - 8.0, sel_rect.center().y),
            Align2::RIGHT_CENTER, ">",
            FontId::proportional(sel_h * 0.28), TEXT_MUTED);

        if ui.input(|i| i.pointer.any_released()) {
            if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                if sel_rect.contains(pos) {
                    state.selected_cluster_idx = (state.selected_cluster_idx + 1) % renderers.len();
                }
            }
        }
    }

    // === START CLUSTER button (large, prominent) ===
    let btn_y = sel_y + sel_h + h * 0.015;
    let btn_h = h * 0.10;
    let btn_w = w * 0.55;
    let btn_rect = Rect::from_center_size(
        Pos2::new(cx, btn_y + btn_h * 0.5),
        Vec2::new(btn_w, btn_h));

    let can_start = state.can_start();
    let hovered = ui.input(|i| i.pointer.interact_pos().map_or(false, |p| btn_rect.contains(p)));

    let (bg_color, txt_color, sub_text) = if !can_start {
        (BTN_DISABLED_BG, BTN_DISABLED_TEXT,
         Some("Waiting for critical systems..."))
    } else if hovered {
        (ORANGE_HOVER, Color32::from_rgb(5, 5, 5), None)
    } else {
        (ORANGE, Color32::from_rgb(5, 5, 5), None)
    };

    // Button glow when enabled
    if can_start {
        let glow_alpha = ((t * 1.5).sin() * 0.15 + 0.85).clamp(0.0, 1.0);
        let ga = (glow_alpha * 20.0) as u8;
        let glow_rect = btn_rect.expand(3.0);
        painter.rect_filled(glow_rect, 10.0,
            Color32::from_rgba_premultiplied(255, 90, 0, ga));
    }

    painter.rect_filled(btn_rect, 8.0, bg_color);
    painter.text(
        Pos2::new(cx, btn_rect.center().y - if sub_text.is_some() { 4.0 } else { 0.0 }),
        Align2::CENTER_CENTER,
        "START CLUSTER",
        FontId::proportional(btn_h * 0.34),
        txt_color);

    if let Some(sub) = sub_text {
        painter.text(
            Pos2::new(cx, btn_rect.center().y + btn_h * 0.22),
            Align2::CENTER_CENTER, sub,
            FontId::proportional(btn_h * 0.14), BTN_DISABLED_TEXT);
    }

    if can_start && hovered && ui.input(|i| i.pointer.any_released()) {
        start_pressed = true;
    }
    if can_start && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        start_pressed = true;
    }

    // Ctrl+Alt+D dev panel
    if ui.input(|i| i.key_pressed(egui::Key::D) && i.modifiers.ctrl && i.modifiers.alt) {
        state.dev_panel_open = !state.dev_panel_open;
    }

    // Request repaint while animating
    if t < 5.0 || !state.checks_complete || state.displayed_progress < 0.99 {
        ui.ctx().request_repaint();
    }

    let cluster_idx = if start_pressed { Some(state.selected_cluster_idx) } else { None };
    (start_pressed, cluster_idx)
}

// ---------------------------------------------------------------------------
// Animated row
// ---------------------------------------------------------------------------

fn draw_animated_row(
    painter: &Painter, rect: Rect, check: &SystemCheck,
    anim: &RowAnim, t: f32, alpha: f32,
) {
    let h = rect.height();
    let status_color = check.status.color();

    // Alpha-modulated colors
    let a = (alpha * 255.0) as u8;
    let row_bg = Color32::from_rgba_premultiplied(17, 17, 20, a);

    // Row background
    painter.rect_filled(rect, 4.0, row_bg);

    // Left accent bar (color = status)
    let sc = status_color;
    let accent_color = Color32::from_rgba_premultiplied(sc.r(), sc.g(), sc.b(), a);
    let accent = Rect::from_min_size(rect.min, Vec2::new(3.0, rect.height()));
    painter.rect_filled(accent, 2.0, accent_color);

    // Status dot (left side)
    let dot_cx = rect.left() + 20.0;
    let dot_cy = rect.center().y;

    if check.status == CheckStatus::Checking {
        // Pulsing dot for checking state
        let pulse = ((t * PULSE_SPEED * std::f32::consts::PI * 2.0).sin() * 0.4 + 0.6)
            .clamp(0.2, 1.0);
        let pa = (pulse * alpha * 255.0) as u8;
        let pulse_color = Color32::from_rgba_premultiplied(
            CHECKING_COLOR.r(), CHECKING_COLOR.g(), CHECKING_COLOR.b(), pa);
        painter.circle_filled(Pos2::new(dot_cx, dot_cy), DOT_RADIUS, pulse_color);
    } else {
        // Solid colored dot
        painter.circle_filled(Pos2::new(dot_cx, dot_cy), DOT_RADIUS, accent_color);
        // Checkmark / X / ! inside
        let sym = check.status.symbol();
        if !sym.is_empty() {
            painter.text(
                Pos2::new(dot_cx, dot_cy),
                Align2::CENTER_CENTER, sym,
                FontId::proportional(DOT_RADIUS * 1.4),
                Color32::from_rgba_premultiplied(5, 5, 5, a));
        }
    }

    // System name
    let name_x = rect.left() + 38.0;
    let name_color = Color32::from_rgba_premultiplied(244, 244, 244, a);
    painter.text(
        Pos2::new(name_x, rect.center().y - h * 0.08),
        Align2::LEFT_CENTER,
        &check.name,
        FontId::proportional(h * 0.34),
        name_color);

    // Detail text (below name, smaller)
    if !check.detail.is_empty() {
        let detail_color = Color32::from_rgba_premultiplied(140, 140, 145, a);
        painter.text(
            Pos2::new(name_x, rect.center().y + h * 0.20),
            Align2::LEFT_CENTER,
            &check.detail,
            FontId::proportional(h * 0.17),
            detail_color);
    } else if check.required {
        let req_color = Color32::from_rgba_premultiplied(70, 70, 75, a);
        painter.text(
            Pos2::new(name_x, rect.center().y + h * 0.20),
            Align2::LEFT_CENTER,
            "REQUIRED",
            FontId::proportional(h * 0.14),
            req_color);
    }

    // Status label (right)
    let label_color = Color32::from_rgba_premultiplied(
        sc.r(), sc.g(), sc.b(), a);
    painter.text(
        Pos2::new(rect.right() - 12.0, rect.center().y),
        Align2::RIGHT_CENTER,
        check.status.label(),
        FontId::proportional(h * 0.26),
        label_color);
}

// ---------------------------------------------------------------------------
// Header
// ---------------------------------------------------------------------------

fn draw_header(painter: &Painter, rect: Rect, t: f32) {
    let h = rect.height();
    let alpha = (t / 0.5).clamp(0.0, 1.0);
    let a = (alpha * 255.0) as u8;

    // NinoDash wordmark
    let nino_c = Color32::from_rgba_premultiplied(244, 244, 244, a);
    let dash_c = Color32::from_rgba_premultiplied(255, 90, 0, a);
    painter.text(
        Pos2::new(rect.left() + 25.0, rect.center().y - 6.0),
        Align2::LEFT_CENTER, "Nino",
        FontId::proportional(h * 0.42), nino_c);
    painter.text(
        Pos2::new(rect.left() + 90.0, rect.center().y - 6.0),
        Align2::LEFT_CENTER, "Dash",
        FontId::proportional(h * 0.42), dash_c);

    // Subtitle
    let sub_alpha = ((t - 0.2) / 0.4).clamp(0.0, 1.0);
    let sa = (sub_alpha * 140.0) as u8;
    painter.text(
        Pos2::new(rect.left() + 25.0, rect.center().y + 14.0),
        Align2::LEFT_CENTER, "SYSTEM PREFLIGHT",
        FontId::proportional(h * 0.20),
        Color32::from_rgba_premultiplied(sa, sa, sa, sa));

    // Clock
    let time = chrono_time();
    painter.text(
        Pos2::new(rect.right() - 25.0, rect.center().y),
        Align2::RIGHT_CENTER, &time,
        FontId::proportional(h * 0.24),
        Color32::from_rgba_premultiplied(168, 168, 168, a));

    // Separator
    painter.line_segment(
        [Pos2::new(rect.left() + 15.0, rect.bottom()),
         Pos2::new(rect.right() - 15.0, rect.bottom())],
        Stroke::new(1.0_f32, SEPARATOR));
}

// ---------------------------------------------------------------------------
// Detail panel
// ---------------------------------------------------------------------------

fn draw_detail_panel(painter: &Painter, rect: Rect, check: &SystemCheck) {
    let full = painter.clip_rect();
    painter.rect_filled(full, 0.0, Color32::from_rgba_premultiplied(0, 0, 0, 190));
    painter.rect_filled(rect, 8.0, DETAIL_BG);
    painter.rect_stroke(rect, 8.0, Stroke::new(1.0_f32, check.status.color()), StrokeKind::Outside);

    painter.text(
        Pos2::new(rect.left() + 20.0, rect.top() + 25.0),
        Align2::LEFT_CENTER, &check.name,
        FontId::proportional(18.0), TEXT_PRIMARY);
    painter.text(
        Pos2::new(rect.right() - 20.0, rect.top() + 25.0),
        Align2::RIGHT_CENTER, check.status.label(),
        FontId::proportional(15.0), check.status.color());

    painter.line_segment(
        [Pos2::new(rect.left() + 15.0, rect.top() + 45.0),
         Pos2::new(rect.right() - 15.0, rect.top() + 45.0)],
        Stroke::new(1.0_f32, SEPARATOR));

    let row_h = 26.0;
    let start_y = rect.top() + 58.0;
    let max_rows = ((rect.height() - 75.0) / row_h) as usize;

    for (i, (key, val)) in check.extra.iter().take(max_rows).enumerate() {
        let y = start_y + i as f32 * row_h;
        painter.text(
            Pos2::new(rect.left() + 22.0, y),
            Align2::LEFT_CENTER, key,
            FontId::proportional(12.0), TEXT_SECONDARY);
        painter.text(
            Pos2::new(rect.right() - 22.0, y),
            Align2::RIGHT_CENTER, val,
            FontId::proportional(12.0), TEXT_PRIMARY);
    }
}

// ---------------------------------------------------------------------------
// Time helper
// ---------------------------------------------------------------------------

pub fn chrono_time_internal() -> String { chrono_time() }

fn chrono_time() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let hours = ((secs % 86400) / 3600) as u32;
    let mins = ((secs % 3600) / 60) as u32;
    let ist_mins = mins + 30;
    let carry = ist_mins / 60;
    let ist_mins = ist_mins % 60;
    let ist_hours = (hours + 5 + carry) % 24;
    format!("{:02}:{:02}", ist_hours, ist_mins)
}

// ---------------------------------------------------------------------------
// Background system checks (unchanged logic, runs in thread)
// ---------------------------------------------------------------------------

fn run_system_checks() -> Vec<SystemCheck> {
    let mut results = vec![];

    // Raspberry Pi System
    {
        let mut check = SystemCheck {
            name: "Raspberry Pi System".into(), status: CheckStatus::Ready,
            detail: String::new(), required: true, extra: vec![],
        };
        let cpu_count = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        check.extra.push(("CPU cores".into(), format!("{}", cpu_count)));
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
        if let Ok(out) = Command::new("vcgencmd").arg("measure_temp").output() {
            let text = String::from_utf8_lossy(&out.stdout);
            let temp_str = text.trim().replace("temp=", "").replace("'C", " °C");
            check.extra.push(("Temperature".into(), temp_str.clone()));
            if let Some(t) = temp_str.split_whitespace().next().and_then(|s| s.parse::<f32>().ok()) {
                if t > 80.0 { check.status = CheckStatus::Warning; check.detail = format!("High temp: {:.0}°C", t); }
            }
        }
        if let Ok(out) = Command::new("uptime").arg("-p").output() {
            check.extra.push(("Uptime".into(), String::from_utf8_lossy(&out.stdout).trim().into()));
        }
        if check.detail.is_empty() { check.detail = format!("{} cores, healthy", cpu_count); }
        results.push(check);
    }

    // JCR1440 USB Device
    {
        let mut check = SystemCheck {
            name: "JCR1440 USB Device".into(), status: CheckStatus::Checking,
            detail: "Waiting for poller...".into(), required: true, extra: vec![],
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
            } else {
                check.status = CheckStatus::Failed;
                check.detail = "Not detected on USB".into();
            }
        }
        results.push(check);
    }

    // OBD Interface
    {
        let mut check = SystemCheck {
            name: "OBD Interface".into(), status: CheckStatus::Checking,
            detail: "Waiting for data...".into(), required: true, extra: vec![],
        };
        let mut rndis_found = false;
        for entry in std::fs::read_dir("/sys/class/net/").into_iter().flatten().flatten() {
            let p = entry.path();
            let driver_link = p.join("device/driver");
            if let Ok(target) = std::fs::read_link(&driver_link) {
                let drv = target.file_name().unwrap_or_default().to_string_lossy().to_string();
                if drv == "rndis_host" {
                    let iface = p.file_name().unwrap_or_default().to_string_lossy().to_string();
                    check.extra.push(("RNDIS interface".into(), iface));
                    rndis_found = true;
                }
            }
        }
        if rndis_found { check.status = CheckStatus::Connected; check.detail = "Interface available".into(); }
        results.push(check);
    }

    // Vehicle Data Stream (placeholder — updated by telemetry)
    results.push(SystemCheck {
        name: "Vehicle Data Stream".into(), status: CheckStatus::Checking,
        detail: "Waiting for data...".into(), required: true, extra: vec![],
    });

    // GPS (placeholder — updated by telemetry)
    results.push(SystemCheck {
        name: "GPS".into(), status: CheckStatus::Checking,
        detail: "Waiting for fix...".into(), required: false, extra: vec![],
    });

    // Network
    {
        let mut check = SystemCheck {
            name: "Network".into(), status: CheckStatus::Warning,
            detail: "Offline".into(), required: false, extra: vec![],
        };
        if let Ok(out) = Command::new("ip").args(["-4", "addr", "show"]).output() {
            let text = String::from_utf8_lossy(&out.stdout);
            if text.lines().any(|l| l.contains("inet ") && !l.contains("127.0.0.1")) {
                check.status = CheckStatus::Ready;
                check.detail = "Connected".into();
            }
        }
        results.push(check);
    }

    // Telemetry Service
    {
        let mut check = SystemCheck {
            name: "Telemetry Service".into(), status: CheckStatus::NotAvailable,
            detail: "Not running".into(), required: false, extra: vec![],
        };
        if let Ok(out) = Command::new("systemctl").args(["is-active", "jcr1440-telemetry"]).output() {
            let active = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if active == "active" {
                check.status = CheckStatus::Ready;
                check.detail = "Running".into();
            } else {
                check.detail = format!("State: {}", active);
            }
        }
        results.push(check);
    }

    // Storage
    {
        let mut check = SystemCheck {
            name: "Storage".into(), status: CheckStatus::Ready,
            detail: "Healthy".into(), required: false, extra: vec![],
        };
        if let Ok(out) = Command::new("df").args(["-h", "/"]).output() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines().skip(1) {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 5 {
                    check.detail = format!("{} free of {}", parts[3], parts[1]);
                    let pct_str = parts[4].trim_end_matches('%');
                    if let Ok(pct) = pct_str.parse::<u32>() {
                        if pct > 95 { check.status = CheckStatus::Failed; check.detail = format!("Critical: {}% full", pct); }
                        else if pct > 85 { check.status = CheckStatus::Warning; check.detail = format!("Low space: {}% used", pct); }
                    }
                }
            }
        }
        results.push(check);
    }

    results
}
