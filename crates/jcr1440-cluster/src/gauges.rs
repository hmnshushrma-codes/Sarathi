//! Haltech-inspired dual-gauge racing cluster.
//!
//! Layout reference: Haltech iC-7
//!   - Two large sweep gauges (RPM left, Speed right)
//!   - Yellow/gold dial face, red needle, red zone fill
//!   - Digital readout boxes below each gauge
//!   - Stacked readout bars on left and right edges
//!   - Branding bottom center

use std::f32::consts::PI;
use egui::{Color32, FontId, Painter, Pos2, Rect, Stroke, StrokeKind, Vec2};
use jcr1440_client::GpsData;

// ---------------------------------------------------------------------------
// Color palette — Haltech-inspired yellow/black/red
// ---------------------------------------------------------------------------

const BG: Color32 = Color32::from_rgb(8, 8, 10);
const DIAL_YELLOW: Color32 = Color32::from_rgb(230, 200, 30);
const DIAL_YELLOW_DIM: Color32 = Color32::from_rgb(160, 140, 20);
const DIAL_BG: Color32 = Color32::from_rgb(18, 18, 22);
const DIAL_RING: Color32 = Color32::from_rgb(50, 50, 55);
const NEEDLE_RED: Color32 = Color32::from_rgb(230, 30, 20);
const NEEDLE_GLOW: Color32 = Color32::from_rgba_premultiplied(230, 30, 20, 50);
const REDZONE_FILL: Color32 = Color32::from_rgba_premultiplied(200, 25, 15, 140);
const TICK_MAJOR: Color32 = Color32::from_rgb(220, 195, 25);
const TICK_MINOR: Color32 = Color32::from_rgb(100, 90, 20);
const NUM_COLOR: Color32 = Color32::from_rgb(230, 200, 30);
const NUM_RED: Color32 = Color32::from_rgb(230, 40, 30);

const TEXT_WHITE: Color32 = Color32::from_rgb(240, 242, 248);
const TEXT_DIM: Color32 = Color32::from_rgb(100, 105, 110);
const TEXT_UNIT: Color32 = Color32::from_rgb(70, 72, 78);

const BOX_BG: Color32 = Color32::from_rgb(16, 16, 20);
const BOX_BORDER: Color32 = Color32::from_rgb(45, 45, 50);
const BAR_BG: Color32 = Color32::from_rgb(30, 30, 35);
const BAR_YELLOW: Color32 = Color32::from_rgb(210, 185, 25);
const BAR_GREEN: Color32 = Color32::from_rgb(30, 190, 60);
const BAR_AMBER: Color32 = Color32::from_rgb(230, 160, 20);
const BAR_RED: Color32 = Color32::from_rgb(220, 35, 30);
const BAR_CYAN: Color32 = Color32::from_rgb(0, 210, 240);

const GEAR_ACTIVE: Color32 = Color32::from_rgb(0, 220, 255);
const GEAR_DIM: Color32 = Color32::from_rgb(45, 48, 55);

const BRAND_COLOR: Color32 = Color32::from_rgb(230, 200, 30);
const BRAND_DIM: Color32 = Color32::from_rgb(90, 80, 20);

const NO_SIGNAL_BG: Color32 = Color32::from_rgba_premultiplied(8, 8, 10, 210);

// ---------------------------------------------------------------------------
// Main layout — called from the app
// ---------------------------------------------------------------------------

pub struct ClusterLayout {
    pub rpm: f32,
    pub speed: f32,
    pub coolant_temp: f32,
    pub intake_temp: f32,
    pub oil_temp: f32,
    pub voltage: f32,
    pub throttle: f32,
    pub manifold_pressure: f32,
    pub fuel_rate: f32,
    pub fuel_level: f32,
    pub maf: f32,
    pub gear: u8,
    pub gps: Option<GpsData>,
    pub dtc_count: u32,
    pub connected: bool,
    pub error_msg: Option<String>,
    pub redline_intensity: f32,
    // Speed source info
    pub speed_source: &'static str,  // "OBD", "GPS", "---"
    pub gps_speed: f32,
    pub obd_speed: f32,
    // Trip info
    pub trip_active: bool,
    pub trip_distance_km: f64,
    pub trip_points: u32,
}

pub fn draw_full_cluster(painter: &Painter, rect: Rect, data: &ClusterLayout) {
    let w = rect.width();
    let h = rect.height();
    let cx = rect.center().x;

    // Layout zones:
    //   Top strip:   gear bar + DTC badge         (~8% height)
    //   Middle:      two sweep gauges side by side (~60% height)
    //   Bottom:      6 readout boxes in a row      (~25% height)
    //   Footer:      GPS + branding                (~7% height)

    let top_h = h * 0.07;
    let footer_h = h * 0.07;
    let bottom_row_h = h * 0.20;
    let gauge_area_h = h - top_h - bottom_row_h - footer_h;

    // Gauge sizing: fit two circles side by side with padding
    let gauge_radius = (gauge_area_h * 0.48).min(w * 0.22);
    let gauge_cy = rect.top() + top_h + gauge_area_h * 0.52;
    let gauge_gap = w * 0.01;
    let rpm_cx = cx - gauge_radius - gauge_gap;
    let spd_cx = cx + gauge_radius + gauge_gap;

    // -- Gear bar (top center) --
    let gear_bar_y = rect.top() + top_h * 0.55;
    draw_gear_bar(painter, Pos2::new(cx, gear_bar_y), data.gear);

    // DTC badge (left of gear bar)
    if data.dtc_count > 0 {
        draw_dtc_badge(painter, Pos2::new(cx - 170.0, gear_bar_y), data.dtc_count);
    }

    // -- RPM gauge (left) --
    draw_sweep_gauge(
        painter, Pos2::new(rpm_cx, gauge_cy), gauge_radius,
        data.rpm, 8000.0, 7000.0, 1000.0, true, "Nino's Tech",
    );

    // Digital RPM + fuel inside gauge bottom
    let rpm_box_y = gauge_cy + gauge_radius * 0.38;
    draw_digital_box(painter, Pos2::new(rpm_cx, rpm_box_y),
        gauge_radius * 0.72, gauge_radius * 0.26,
        &format!("{:.0}", data.rpm), "RPM",
        &format!("{:.1}", data.fuel_rate), "L/h  FUEL",
    );

    // -- Speed gauge (right) --
    draw_sweep_gauge(
        painter, Pos2::new(spd_cx, gauge_cy), gauge_radius,
        data.speed, 240.0, 200.0, 20.0, false, "uC10",
    );

    // Digital speed + gear inside gauge bottom
    let gear_text = if data.gear == 0 { "N".into() } else { format!("{}", data.gear) };
    draw_digital_box(painter, Pos2::new(spd_cx, rpm_box_y),
        gauge_radius * 0.72, gauge_radius * 0.26,
        &format!("{:.0}", data.speed), "SPEED",
        &gear_text, "GEAR",
    );

    // -- Bottom row: 6 readout boxes evenly spaced --
    let row_y = rect.bottom() - footer_h - bottom_row_h;
    let box_gap = w * 0.01;
    let total_gap = box_gap * 5.0; // 5 gaps between 6 boxes
    let box_w = (w - total_gap - box_gap * 2.0) / 6.0; // box_gap on left/right margin
    let box_h = bottom_row_h * 0.90;
    let box_y = row_y + (bottom_row_h - box_h) * 0.5;

    let boxes: [(f32, f32, &str, &str, Color32); 6] = [
        (data.manifold_pressure, 101.0, "MAP", "kPa",
            BAR_YELLOW),
        (data.coolant_temp, 120.0, "CLT", "°C",
            if data.coolant_temp > 105.0 { BAR_RED } else if data.coolant_temp > 95.0 { BAR_AMBER } else { BAR_YELLOW }),
        (data.intake_temp, 70.0, "IAT", "°C",
            if data.intake_temp > 55.0 { BAR_RED } else { BAR_YELLOW }),
        (data.throttle, 100.0, "TPS", "%",
            if data.throttle > 80.0 { BAR_RED } else if data.throttle > 50.0 { BAR_AMBER } else { BAR_GREEN }),
        (data.voltage, 15.0, "BATT", "V",
            if data.voltage < 11.5 { BAR_RED } else if data.voltage < 12.2 { BAR_AMBER } else { BAR_GREEN }),
        (data.oil_temp, 140.0, "OIL", "°C",
            if data.oil_temp > 120.0 { BAR_RED } else if data.oil_temp > 100.0 { BAR_AMBER } else { BAR_YELLOW }),
    ];

    for (i, (val, max, label, unit, color)) in boxes.iter().enumerate() {
        let bx = rect.left() + box_gap + i as f32 * (box_w + box_gap);
        draw_readout_box(painter, Pos2::new(bx, box_y), box_w, box_h,
            *val, *max, label, unit, *color);
    }

    // -- Footer: GPS + branding --
    let footer_y = rect.bottom() - footer_h;
    if let Some(ref gps) = data.gps {
        draw_gps_line(painter, Pos2::new(cx, footer_y + footer_h * 0.3), gps);
    }
    painter.text(
        Pos2::new(cx, footer_y + footer_h * 0.75),
        egui::Align2::CENTER_CENTER,
        "Nino's Tech  ·  oyenino.com",
        FontId::proportional(10.0),
        BRAND_DIM,
    );

    // -- No-signal overlay --
    if !data.connected {
        draw_no_signal(painter, rect, data.error_msg.as_deref());
    }
}

// ---------------------------------------------------------------------------
// Sweep gauge — used for both RPM and Speed
// ---------------------------------------------------------------------------

fn draw_sweep_gauge(
    painter: &Painter, center: Pos2, radius: f32,
    value: f32, max: f32, redline: f32,
    tick_step: f32, fill_redzone: bool, brand_text: &str,
) {
    let start_angle = PI * 0.75;   // 7 o'clock
    let sweep = PI * 1.5;          // 270°

    // Yellow outer band (the distinctive Haltech look)
    let band_outer = radius;
    let band_inner = radius * 0.82;
    draw_arc_band(painter, center, band_inner, band_outer, start_angle, sweep, DIAL_YELLOW);

    // Redzone fill on the yellow band
    let redline_frac = redline / max;
    let redline_angle = start_angle + sweep * redline_frac;
    let redline_sweep = sweep * (1.0 - redline_frac);
    if fill_redzone {
        draw_arc_band(painter, center, band_inner, band_outer, redline_angle, redline_sweep, REDZONE_FILL);
    }

    // Dark inner circle
    painter.circle_filled(center, band_inner, DIAL_BG);

    // Inner ring
    draw_arc(painter, center, band_inner, start_angle, sweep, Stroke::new(2.0, DIAL_RING));

    // Tick marks and numerals
    let num_major = (max / tick_step) as u32;
    for i in 0..=num_major {
        let val = i as f32 * tick_step;
        let frac = val / max;
        let angle = start_angle + sweep * frac;
        let is_red = val >= redline;

        // Major tick — cuts into the yellow band
        let t_inner = pos_on_circle(center, radius * 0.78, angle);
        let t_outer = pos_on_circle(center, radius * 0.95, angle);
        let tick_color = if is_red { NUM_RED } else { Color32::from_rgb(20, 20, 24) };
        painter.line_segment([t_inner, t_outer], Stroke::new(2.5, tick_color));

        // Numeral inside the dial
        let label_pos = pos_on_circle(center, radius * 0.68, angle);
        let display = if tick_step >= 1000.0 {
            format!("{}", (val / 1000.0) as u32) // "0" "1" "2" ... "8" for RPM
        } else {
            format!("{}", val as u32) // "0" "20" "40" ... "240" for speed
        };
        let color = if is_red { NUM_RED } else { NUM_COLOR };
        let font_size = if tick_step >= 1000.0 { radius * 0.14 } else { radius * 0.10 };
        painter.text(label_pos, egui::Align2::CENTER_CENTER, display, FontId::proportional(font_size), color);

        // Minor ticks
        if i < num_major {
            let mid_val = val + tick_step / 2.0;
            let mid_angle = start_angle + sweep * (mid_val / max);
            let mi = pos_on_circle(center, radius * 0.84, mid_angle);
            let mo = pos_on_circle(center, radius * 0.95, mid_angle);
            painter.line_segment([mi, mo], Stroke::new(1.2, Color32::from_rgb(40, 38, 15)));
        }
    }

    // Brand text inside dial
    let brand_pos = Pos2::new(center.x, center.y - radius * 0.25);
    painter.text(brand_pos, egui::Align2::CENTER_CENTER, brand_text, FontId::proportional(radius * 0.09), DIAL_YELLOW_DIM);

    // Unit label
    let unit = if tick_step >= 1000.0 { "×1000 rpm" } else { "km/h" };
    let unit_pos = Pos2::new(center.x + radius * 0.25, center.y - radius * 0.05);
    painter.text(unit_pos, egui::Align2::CENTER_CENTER, unit, FontId::proportional(radius * 0.06), TEXT_DIM);

    // -- Needle --
    let clamped = value.clamp(0.0, max);
    let needle_angle = start_angle + sweep * (clamped / max);
    let tip = pos_on_circle(center, radius * 0.76, needle_angle);
    let tail = pos_on_circle(center, radius * 0.10, needle_angle + PI);
    let base_l = pos_on_circle(center, radius * 0.035, needle_angle + PI * 0.5);
    let base_r = pos_on_circle(center, radius * 0.035, needle_angle - PI * 0.5);

    // Glow
    painter.line_segment([center, tip], Stroke::new(5.0, NEEDLE_GLOW));

    // Needle body
    painter.add(egui::Shape::convex_polygon(
        vec![tip, base_l, tail, base_r],
        NEEDLE_RED,
        Stroke::NONE,
    ));

    // Center cap
    painter.circle_filled(center, radius * 0.055, NEEDLE_RED);
    painter.circle_filled(center, radius * 0.03, DIAL_BG);
}

// ---------------------------------------------------------------------------
// Digital readout box (below each gauge)
// ---------------------------------------------------------------------------

fn draw_digital_box(
    painter: &Painter, center: Pos2, width: f32, height: f32,
    main_value: &str, main_label: &str,
    sub_value: &str, sub_label: &str,
) {
    let rect = Rect::from_center_size(center, Vec2::new(width, height));
    painter.rect_filled(rect, 4.0, BOX_BG);
    painter.rect_stroke(rect, 4.0, Stroke::new(1.0, BOX_BORDER), StrokeKind::Outside);

    // Main value (large, left side)
    let main_pos = Pos2::new(rect.left() + width * 0.35, rect.center().y - height * 0.12);
    painter.text(main_pos, egui::Align2::CENTER_CENTER, main_value, FontId::proportional(height * 0.52), TEXT_WHITE);

    // Main label (small, right of value)
    let label_pos = Pos2::new(rect.right() - width * 0.15, rect.center().y - height * 0.18);
    painter.text(label_pos, egui::Align2::CENTER_CENTER, main_label, FontId::proportional(height * 0.2), TEXT_DIM);

    // Sub value + label (bottom row)
    let sub_pos = Pos2::new(rect.left() + width * 0.35, rect.bottom() - height * 0.2);
    painter.text(sub_pos, egui::Align2::CENTER_CENTER, sub_value, FontId::proportional(height * 0.3), DIAL_YELLOW);

    let sub_label_pos = Pos2::new(rect.right() - width * 0.15, rect.bottom() - height * 0.2);
    painter.text(sub_label_pos, egui::Align2::CENTER_CENTER, sub_label, FontId::proportional(height * 0.17), TEXT_DIM);
}

// ---------------------------------------------------------------------------
// Readout box with horizontal bar (side panels)
// ---------------------------------------------------------------------------

fn draw_readout_box(
    painter: &Painter, top_left: Pos2, width: f32, height: f32,
    value: f32, max: f32, label: &str, unit: &str, bar_color: Color32,
) {
    let rect = Rect::from_min_size(top_left, Vec2::new(width, height));
    painter.rect_filled(rect, 3.0, BOX_BG);
    painter.rect_stroke(rect, 3.0, Stroke::new(1.0, BOX_BORDER), StrokeKind::Outside);

    // Value (large, top half)
    let val_text = if value > 0.0 { format!("{:.0}", value) } else { "--".to_string() };
    let val_pos = Pos2::new(rect.center().x, rect.top() + height * 0.32);
    painter.text(val_pos, egui::Align2::CENTER_CENTER, &val_text, FontId::proportional(height * 0.38), TEXT_WHITE);

    // Label + unit (right side, smaller)
    let label_pos = Pos2::new(rect.right() - width * 0.12, rect.top() + height * 0.15);
    painter.text(label_pos, egui::Align2::CENTER_CENTER, unit, FontId::proportional(height * 0.13), TEXT_DIM);

    let name_pos = Pos2::new(rect.left() + width * 0.15, rect.top() + height * 0.15);
    painter.text(name_pos, egui::Align2::CENTER_CENTER, label, FontId::proportional(height * 0.15), DIAL_YELLOW_DIM);

    // Horizontal bar (bottom portion)
    let bar_margin = width * 0.08;
    let bar_h = height * 0.16;
    let bar_y = rect.bottom() - height * 0.22;
    let bar_rect = Rect::from_min_size(
        Pos2::new(rect.left() + bar_margin, bar_y),
        Vec2::new(width - bar_margin * 2.0, bar_h),
    );
    painter.rect_filled(bar_rect, 2.0, BAR_BG);

    // Fill
    if value > 0.0 {
        let frac = (value / max).clamp(0.0, 1.0);
        let fill_rect = Rect::from_min_size(
            bar_rect.min,
            Vec2::new(bar_rect.width() * frac, bar_h),
        );
        painter.rect_filled(fill_rect, 2.0, bar_color);

        // Tick segments (Haltech-style horizontal lines)
        let num_ticks = 8;
        for i in 1..num_ticks {
            let tx = bar_rect.left() + bar_rect.width() * (i as f32 / num_ticks as f32);
            painter.line_segment(
                [Pos2::new(tx, bar_rect.top()), Pos2::new(tx, bar_rect.bottom())],
                Stroke::new(1.0, BOX_BG),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Gear bar — horizontal N 1 2 3 4 5 6
// ---------------------------------------------------------------------------

fn draw_gear_bar(painter: &Painter, center: Pos2, gear: u8) {
    let gears = ["N", "1", "2", "3", "4", "5", "6"];
    let slot_w = 32.0;
    let slot_h = 28.0;
    let gap = 4.0;
    let total_w = gears.len() as f32 * slot_w + (gears.len() - 1) as f32 * gap;
    let start_x = center.x - total_w / 2.0;
    let top_y = center.y - slot_h / 2.0;

    for (i, label) in gears.iter().enumerate() {
        let x = start_x + i as f32 * (slot_w + gap);
        let r = Rect::from_min_size(Pos2::new(x, top_y), Vec2::new(slot_w, slot_h));
        let active = i as u8 == gear;

        if active {
            painter.rect_filled(r, 3.0, GEAR_ACTIVE);
            painter.text(r.center(), egui::Align2::CENTER_CENTER, *label, FontId::proportional(20.0), Color32::from_rgb(8, 8, 10));
        } else {
            painter.rect_filled(r, 3.0, BOX_BG);
            painter.rect_stroke(r, 3.0, Stroke::new(1.0, GEAR_DIM), StrokeKind::Outside);
            painter.text(r.center(), egui::Align2::CENTER_CENTER, *label, FontId::proportional(17.0), GEAR_DIM);
        }
    }
}

// ---------------------------------------------------------------------------
// DTC badge
// ---------------------------------------------------------------------------

fn draw_dtc_badge(painter: &Painter, center: Pos2, count: u32) {
    painter.circle_filled(center, 14.0, BAR_RED);
    painter.text(center, egui::Align2::CENTER_CENTER, format!("{}", count), FontId::proportional(12.0), Color32::WHITE);
    let lp = Pos2::new(center.x, center.y + 18.0);
    painter.text(lp, egui::Align2::CENTER_CENTER, "DTC", FontId::proportional(9.0), BAR_RED);
}

// ---------------------------------------------------------------------------
// GPS info line
// ---------------------------------------------------------------------------

fn draw_gps_line(painter: &Painter, center: Pos2, gps: &GpsData) {
    let text = format!("{}sat  HDOP {:.1}  |  {:.5}°, {:.5}°", gps.satellites, gps.hdop, gps.latitude, gps.longitude);
    painter.text(center, egui::Align2::CENTER_CENTER, &text, FontId::proportional(10.0), TEXT_DIM);
}

// ---------------------------------------------------------------------------
// No-signal overlay
// ---------------------------------------------------------------------------

fn draw_no_signal(painter: &Painter, rect: Rect, message: Option<&str>) {
    painter.rect_filled(rect, 0.0, NO_SIGNAL_BG);
    let c = rect.center();
    painter.text(Pos2::new(c.x, c.y - 16.0), egui::Align2::CENTER_CENTER, "NO SIGNAL", FontId::proportional(36.0), BAR_RED);
    if let Some(msg) = message {
        painter.text(Pos2::new(c.x, c.y + 16.0), egui::Align2::CENTER_CENTER, msg, FontId::proportional(14.0), TEXT_DIM);
    }
}

// ---------------------------------------------------------------------------
// Geometry helpers
// ---------------------------------------------------------------------------

fn pos_on_circle(center: Pos2, radius: f32, angle: f32) -> Pos2 {
    Pos2::new(center.x + radius * angle.cos(), center.y + radius * angle.sin())
}

fn draw_arc(painter: &Painter, center: Pos2, radius: f32, start: f32, sweep: f32, stroke: Stroke) {
    let segs = 64;
    let step = sweep / segs as f32;
    let pts: Vec<Pos2> = (0..=segs).map(|i| {
        let a = start + step * i as f32;
        pos_on_circle(center, radius, a)
    }).collect();
    for w in pts.windows(2) {
        painter.line_segment([w[0], w[1]], stroke);
    }
}

/// Draw a filled arc band between inner and outer radius.
fn draw_arc_band(painter: &Painter, center: Pos2, r_inner: f32, r_outer: f32, start: f32, sweep: f32, color: Color32) {
    let segs = 48;
    let step = sweep / segs as f32;
    for i in 0..segs {
        let a0 = start + step * i as f32;
        let a1 = start + step * (i + 1) as f32;
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
