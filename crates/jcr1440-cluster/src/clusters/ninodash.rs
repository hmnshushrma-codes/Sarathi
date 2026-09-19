//! NinoDash Performance — Audi RS virtual cockpit inspired layout.
//!
//! Layout (1024×600):
//!   Top:        Warning indicators + status icons
//!   Main:       Large forward-sweeping RPM arc (dominant element)
//!   Center:     Speed + Gear + Drive mode
//!   Right:      Two circular gauges (Coolant, Oil/Voltage)
//!   Left:       Fuel info
//!   Bottom:     Status bar (time, temp, GPS, brand)
//!   Shift LEDs: Integrated into RPM arc top

use std::f32::consts::PI;

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, Vec2};

use super::ClusterRenderer;
use crate::gauges::ClusterLayout;
use crate::theme;

// ---------------------------------------------------------------------------
// Audi RS-inspired palette
// ---------------------------------------------------------------------------

const ARC_BG: Color32 = Color32::from_rgb(20, 20, 22);
const ARC_INACTIVE: Color32 = Color32::from_rgb(35, 35, 38);
const ARC_ACTIVE: Color32 = Color32::from_rgb(200, 200, 205);
const ARC_WARN: Color32 = Color32::from_rgb(255, 98, 0);
const ARC_RED: Color32 = Color32::from_rgb(220, 30, 25);

const SPEED_COLOR: Color32 = Color32::from_rgb(244, 244, 244);
const GEAR_COLOR: Color32 = Color32::from_rgb(244, 244, 244);
const LABEL_DIM: Color32 = Color32::from_rgb(100, 100, 105);
const VALUE_COLOR: Color32 = Color32::from_rgb(200, 200, 205);
const RING_BG: Color32 = Color32::from_rgb(28, 28, 32);
const RING_BORDER: Color32 = Color32::from_rgb(55, 55, 60);

const WARNING_RED: Color32 = Color32::from_rgb(220, 30, 25);
const WARNING_ORANGE: Color32 = Color32::from_rgb(255, 140, 0);

// ---------------------------------------------------------------------------
// Renderer
// ---------------------------------------------------------------------------

pub struct NinoDashRenderer;

impl ClusterRenderer for NinoDashRenderer {
    fn id(&self) -> &'static str { "ninodash-performance" }
    fn name(&self) -> &'static str { "NinoDash Performance" }
    fn description(&self) -> &'static str { "Audi RS virtual cockpit inspired layout" }

    fn draw(&self, painter: &Painter, rect: Rect, data: &ClusterLayout) {
        let w = rect.width();
        let h = rect.height();
        let cx = rect.center().x;

        // Background
        painter.rect_filled(rect, 0.0, theme::BG_BLACK);

        // === RPM arc (dominant element — upper 65% of screen) ===
        let arc_rect = Rect::from_min_size(
            rect.min,
            Vec2::new(w, h * 0.65),
        );
        draw_rpm_arc(painter, arc_rect, data);

        // === Center: Speed + Gear + Mode (overlaid on lower arc area) ===
        let center_y = rect.top() + h * 0.42;
        draw_center_display(painter, Pos2::new(cx, center_y), h, data);

        // === Right circular gauges (Coolant + Oil) ===
        let gauge_r = h * 0.11;
        let gauge_x = rect.right() - w * 0.12;
        let gauge_y1 = rect.top() + h * 0.38;
        let gauge_y2 = rect.top() + h * 0.62;
        draw_circular_gauge(painter, Pos2::new(gauge_x, gauge_y1), gauge_r,
            data.coolant_temp, 120.0, "CLT", "°C",
            if data.coolant_temp > 105.0 { WARNING_RED }
            else if data.coolant_temp > 95.0 { WARNING_ORANGE }
            else { ARC_ACTIVE });
        draw_circular_gauge(painter, Pos2::new(gauge_x, gauge_y2), gauge_r,
            data.voltage, 15.0, "BATT", "V",
            if data.voltage > 0.0 && data.voltage < 11.5 { WARNING_RED }
            else { ARC_ACTIVE });

        // === Left: Fuel info ===
        let fuel_x = rect.left() + w * 0.10;
        draw_fuel_display(painter, Pos2::new(fuel_x, rect.top() + h * 0.50), h, data);

        // === Bottom status bar ===
        let bar_rect = Rect::from_min_size(
            Pos2::new(rect.left(), rect.bottom() - h * 0.08),
            Vec2::new(w, h * 0.08),
        );
        draw_status_bar(painter, bar_rect, data);

        // === No-signal overlay ===
        if !data.connected {
            draw_data_lost(painter, rect, data.error_msg.as_deref());
        }
    }
}

// ---------------------------------------------------------------------------
// RPM arc — Audi RS style forward sweep
// ---------------------------------------------------------------------------

fn draw_rpm_arc(painter: &Painter, rect: Rect, data: &ClusterLayout) {
    let w = rect.width();
    let h = rect.height();
    let cx = rect.center().x;
    // Arc center below the visible area (so we see just the upper portion)
    let cy = rect.bottom() + h * 0.40;
    let outer_r = h * 1.15;
    let inner_r = outer_r * 0.90;
    let tick_r = outer_r * 0.87;

    // Arc spans from ~8 o'clock to ~2 o'clock (viewed from below)
    let start_angle = PI + 0.45;  // left side
    let end_angle = -0.45;        // right side
    let sweep = end_angle - start_angle + 2.0 * PI; // going clockwise

    let max_rpm = 8000.0;
    let redline = 7000.0;
    let rpm_frac = (data.rpm / max_rpm).clamp(0.0, 1.0);
    let redline_frac = redline / max_rpm;

    let num_segments = 60;

    // Draw arc segments
    for i in 0..num_segments {
        let frac = i as f32 / num_segments as f32;
        let frac_end = (i + 1) as f32 / num_segments as f32;
        let a0 = start_angle + frac * sweep;
        let a1 = start_angle + frac_end * sweep - 0.005; // small gap

        let color = if frac <= rpm_frac {
            if frac >= redline_frac {
                ARC_RED
            } else if frac >= redline_frac * 0.85 {
                ARC_WARN
            } else {
                ARC_ACTIVE
            }
        } else {
            ARC_INACTIVE
        };

        draw_arc_band(painter, Pos2::new(cx, cy), inner_r, outer_r, a0, a1, color);
    }

    // RPM numerals along the arc
    for rpm_k in 0..=8 {
        let frac = rpm_k as f32 / 8.0;
        let angle = start_angle + frac * sweep;
        let label_r = tick_r - h * 0.06;
        let pos = pos_on_circle(Pos2::new(cx, cy), label_r, angle);

        // Only draw if visible (above bottom of rect)
        if pos.y < rect.bottom() + 10.0 {
            let is_red = (rpm_k as f32 * 1000.0) >= redline;
            let color = if is_red { ARC_RED } else { LABEL_DIM };
            let font_size = h * 0.065;
            painter.text(pos, Align2::CENTER_CENTER,
                &format!("{}", rpm_k),
                FontId::proportional(font_size), color);

            // Tick mark
            let t_inner = pos_on_circle(Pos2::new(cx, cy), tick_r, angle);
            let t_outer = pos_on_circle(Pos2::new(cx, cy), inner_r - 2.0, angle);
            painter.line_segment([t_inner, t_outer],
                Stroke::new(2.0_f32, if is_red { ARC_RED } else { Color32::from_rgb(55, 55, 60) }));
        }
    }

    // "x1000 r/min" label
    let label_pos = Pos2::new(cx + w * 0.12, rect.top() + h * 0.12);
    painter.text(label_pos, Align2::LEFT_CENTER,
        "x1000 r/min",
        FontId::proportional(h * 0.030), LABEL_DIM);

    // RPM digital readout (top center of arc)
    painter.text(
        Pos2::new(cx, rect.top() + h * 0.10),
        Align2::CENTER_CENTER,
        &format!("{:.0}", data.rpm),
        FontId::proportional(h * 0.06),
        if data.rpm >= redline { ARC_RED } else { VALUE_COLOR },
    );
}

// ---------------------------------------------------------------------------
// Center display: Speed + Gear + Drive mode
// ---------------------------------------------------------------------------

fn draw_center_display(painter: &Painter, center: Pos2, h: f32, data: &ClusterLayout) {
    // Speed — large
    painter.text(
        Pos2::new(center.x, center.y),
        Align2::CENTER_CENTER,
        &format!("{:.0}", data.speed),
        FontId::proportional(h * 0.16),
        SPEED_COLOR,
    );
    painter.text(
        Pos2::new(center.x, center.y + h * 0.10),
        Align2::CENTER_CENTER,
        "km/h",
        FontId::proportional(h * 0.025),
        LABEL_DIM,
    );

    // Gear — above speed
    let gear_text = if data.gear == 0 { "P".to_string() } else { format!("{}", data.gear) };
    painter.text(
        Pos2::new(center.x, center.y - h * 0.14),
        Align2::CENTER_CENTER,
        &gear_text,
        FontId::proportional(h * 0.08),
        GEAR_COLOR,
    );

    // Drive mode label
    let mode = if data.gear == 0 { "READY" } else { "DYNAMIC" };
    painter.text(
        Pos2::new(center.x, center.y + h * 0.14),
        Align2::CENTER_CENTER,
        mode,
        FontId::proportional(h * 0.022),
        theme::ORANGE,
    );
}

// ---------------------------------------------------------------------------
// Circular gauge — Audi RS style ring gauge
// ---------------------------------------------------------------------------

fn draw_circular_gauge(
    painter: &Painter, center: Pos2, radius: f32,
    value: f32, max: f32, label: &str, unit: &str,
    fill_color: Color32,
) {
    // Background ring
    painter.circle_filled(center, radius, RING_BG);
    painter.circle_stroke(center, radius, Stroke::new(1.5_f32, RING_BORDER));

    // Fill arc (from bottom, clockwise)
    let frac = (value / max).clamp(0.0, 1.0);
    if frac > 0.0 {
        let start = PI * 0.5; // bottom
        let sweep = PI * 2.0 * frac;
        let arc_w = radius * 0.15;
        draw_arc_band(painter, center,
            radius - arc_w, radius - 1.0,
            start, start - sweep, // counter-clockwise fill
            fill_color);
    }

    // Inner dark circle
    painter.circle_filled(center, radius * 0.72, theme::BG_BLACK);

    // Value
    let val_text = if value > 0.0 {
        if unit == "V" { format!("{:.1}", value) }
        else { format!("{:.0}", value) }
    } else {
        "--".into()
    };
    painter.text(center, Align2::CENTER_CENTER,
        &val_text, FontId::proportional(radius * 0.45), VALUE_COLOR);

    // Label below
    painter.text(
        Pos2::new(center.x, center.y + radius * 0.40),
        Align2::CENTER_CENTER,
        label, FontId::proportional(radius * 0.22), LABEL_DIM);

    // Unit above
    painter.text(
        Pos2::new(center.x, center.y - radius * 0.35),
        Align2::CENTER_CENTER,
        unit, FontId::proportional(radius * 0.18), LABEL_DIM);
}

// ---------------------------------------------------------------------------
// Fuel display (left side)
// ---------------------------------------------------------------------------

fn draw_fuel_display(painter: &Painter, pos: Pos2, h: f32, data: &ClusterLayout) {
    // Fuel rate
    let fuel_text = if data.fuel_rate > 0.1 {
        format!("{:.1}", data.fuel_rate)
    } else {
        "--".into()
    };
    painter.text(
        Pos2::new(pos.x, pos.y - h * 0.04),
        Align2::CENTER_CENTER,
        &fuel_text,
        FontId::proportional(h * 0.05),
        VALUE_COLOR,
    );
    painter.text(
        Pos2::new(pos.x, pos.y + h * 0.02),
        Align2::CENTER_CENTER,
        "L/h",
        FontId::proportional(h * 0.020),
        LABEL_DIM,
    );

    // Throttle
    painter.text(
        Pos2::new(pos.x, pos.y + h * 0.10),
        Align2::CENTER_CENTER,
        &format!("{:.0}%", data.throttle),
        FontId::proportional(h * 0.035),
        if data.throttle > 80.0 { ARC_WARN } else { VALUE_COLOR },
    );
    painter.text(
        Pos2::new(pos.x, pos.y + h * 0.14),
        Align2::CENTER_CENTER,
        "TPS",
        FontId::proportional(h * 0.018),
        LABEL_DIM,
    );

    // MAP
    painter.text(
        Pos2::new(pos.x, pos.y + h * 0.22),
        Align2::CENTER_CENTER,
        &format!("{:.0}", data.manifold_pressure),
        FontId::proportional(h * 0.035),
        VALUE_COLOR,
    );
    painter.text(
        Pos2::new(pos.x, pos.y + h * 0.26),
        Align2::CENTER_CENTER,
        "MAP kPa",
        FontId::proportional(h * 0.018),
        LABEL_DIM,
    );
}

// ---------------------------------------------------------------------------
// Bottom status bar
// ---------------------------------------------------------------------------

fn draw_status_bar(painter: &Painter, rect: Rect, data: &ClusterLayout) {
    // Separator line
    painter.line_segment(
        [Pos2::new(rect.left() + 15.0, rect.top()),
         Pos2::new(rect.right() - 15.0, rect.top())],
        Stroke::new(1.0_f32, Color32::from_rgb(36, 36, 40)),
    );

    let h = rect.height();
    let cx = rect.center().x;

    // GPS status + coordinates — left
    let gps_active = data.gps.as_ref().map_or(false, |g| g.satellites > 0);
    let gps_fix = data.gps.as_ref().map_or(false, |g| g.fix_valid);
    let gps_text = if let Some(ref gps) = data.gps {
        if gps.fix_valid {
            format!("GPS {:.4}°, {:.4}° | {}sat",
                gps.latitude, gps.longitude, gps.satellites)
        } else if gps.satellites > 0 {
            format!("GPS {}sat (searching)", gps.satellites)
        } else {
            "GPS --".into()
        }
    } else { "GPS --".into() };
    let gps_color = if gps_fix { theme::STATUS_OK }
        else if gps_active { theme::ORANGE }
        else { LABEL_DIM };
    painter.text(
        Pos2::new(rect.left() + 20.0, rect.center().y),
        Align2::LEFT_CENTER,
        &gps_text,
        FontId::proportional(h * 0.28),
        gps_color,
    );

    // Time — center
    let time = crate::preflight::chrono_time_internal();
    painter.text(
        Pos2::new(cx, rect.center().y),
        Align2::CENTER_CENTER,
        &time,
        FontId::proportional(h * 0.34),
        VALUE_COLOR,
    );

    // Oil temp — center right
    let oil_color = if data.oil_temp > 120.0 { WARNING_RED }
        else if data.oil_temp > 100.0 { WARNING_ORANGE }
        else { VALUE_COLOR };
    painter.text(
        Pos2::new(cx + 120.0, rect.center().y),
        Align2::CENTER_CENTER,
        &format!("OIL {:.0}°C", data.oil_temp),
        FontId::proportional(h * 0.32),
        oil_color,
    );

    // NinoDash brand — right
    painter.text(
        Pos2::new(rect.right() - 20.0, rect.center().y),
        Align2::RIGHT_CENTER,
        "NinoDash",
        FontId::proportional(h * 0.28),
        Color32::from_rgb(60, 60, 65),
    );

    // DTC warning if active
    if data.dtc_count > 0 {
        painter.text(
            Pos2::new(rect.right() - 100.0, rect.center().y),
            Align2::RIGHT_CENTER,
            &format!("DTC:{}", data.dtc_count),
            FontId::proportional(h * 0.30),
            WARNING_RED,
        );
    }
}

// ---------------------------------------------------------------------------
// Data lost overlay
// ---------------------------------------------------------------------------

fn draw_data_lost(painter: &Painter, rect: Rect, message: Option<&str>) {
    painter.rect_filled(rect, 0.0, Color32::from_rgba_premultiplied(5, 5, 5, 200));
    let c = rect.center();
    painter.text(
        Pos2::new(c.x, c.y - 20.0), Align2::CENTER_CENTER,
        "VEHICLE DATA LOST", FontId::proportional(28.0), theme::ORANGE);
    painter.text(
        Pos2::new(c.x, c.y + 10.0), Align2::CENTER_CENTER,
        "RECONNECTING...", FontId::proportional(14.0), theme::TEXT_DIM);
    if let Some(msg) = message {
        painter.text(
            Pos2::new(c.x, c.y + 35.0), Align2::CENTER_CENTER,
            msg, FontId::proportional(11.0), theme::TEXT_MUTED);
    }
}

// ---------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------

fn pos_on_circle(center: Pos2, radius: f32, angle: f32) -> Pos2 {
    Pos2::new(center.x + radius * angle.cos(), center.y + radius * angle.sin())
}

fn draw_arc_band(
    painter: &Painter, center: Pos2,
    r_inner: f32, r_outer: f32,
    a_start: f32, a_end: f32,
    color: Color32,
) {
    let sweep = a_end - a_start;
    let steps = (sweep.abs() / 0.05).ceil().max(2.0) as usize;
    let da = sweep / steps as f32;
    for i in 0..steps {
        let a0 = a_start + da * i as f32;
        let a1 = a_start + da * (i + 1) as f32;
        let p0_out = pos_on_circle(center, r_outer, a0);
        let p1_out = pos_on_circle(center, r_outer, a1);
        let p1_in = pos_on_circle(center, r_inner, a1);
        let p0_in = pos_on_circle(center, r_inner, a0);
        painter.add(egui::Shape::convex_polygon(
            vec![p0_out, p1_out, p1_in, p0_in],
            color,
            Stroke::NONE,
        ));
    }
}
