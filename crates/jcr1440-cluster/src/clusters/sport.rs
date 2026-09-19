//! Sport — horizontal bar-graph cluster with central speed.
//!
//! Layout:
//!   Left:    RPM bar (vertical)
//!   Center:  Speed + Gear
//!   Right:   Readout stack (CLT, OIL, BATT, TPS)
//!   Bottom:  Status bar

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, StrokeKind, Vec2};

use super::ClusterRenderer;
use crate::gauges::ClusterLayout;
use crate::theme;

const BAR_BG: Color32 = Color32::from_rgb(25, 25, 28);

pub struct SportRenderer;

impl ClusterRenderer for SportRenderer {
    fn id(&self) -> &'static str { "sport" }
    fn name(&self) -> &'static str { "Sport" }
    fn description(&self) -> &'static str { "Horizontal bars with central speed readout" }

    fn draw(&self, painter: &Painter, rect: Rect, data: &ClusterLayout) {
        let w = rect.width();
        let h = rect.height();

        painter.rect_filled(rect, 0.0, theme::BG_BLACK);

        let margin = 15.0;
        let rpm_bar_w = w * 0.08;
        let readout_w = w * 0.22;
        let center_w = w - rpm_bar_w - readout_w - margin * 4.0;
        let status_h = h * 0.08;

        // === RPM bar (left vertical) ===
        let rpm_rect = Rect::from_min_size(
            Pos2::new(rect.left() + margin, rect.top() + margin),
            Vec2::new(rpm_bar_w, h - margin * 2.0 - status_h),
        );
        draw_rpm_bar(painter, rpm_rect, data.rpm, 8000.0, 7000.0);

        // === Center: Speed + Gear ===
        let center_x = rect.left() + margin + rpm_bar_w + margin;
        let center_rect = Rect::from_min_size(
            Pos2::new(center_x, rect.top() + margin),
            Vec2::new(center_w, h - margin * 2.0 - status_h),
        );
        draw_center(painter, center_rect, data);

        // === Right: readout stack ===
        let right_x = center_x + center_w + margin;
        let right_rect = Rect::from_min_size(
            Pos2::new(right_x, rect.top() + margin),
            Vec2::new(readout_w, h - margin * 2.0 - status_h),
        );
        draw_readout_stack(painter, right_rect, data);

        // === Bottom status ===
        let status_rect = Rect::from_min_size(
            Pos2::new(rect.left(), rect.bottom() - status_h),
            Vec2::new(w, status_h),
        );
        draw_status_bar(painter, status_rect, data);

        // No-signal
        if !data.connected {
            painter.rect_filled(rect, 0.0, Color32::from_rgba_premultiplied(5, 5, 5, 200));
            painter.text(rect.center(), Align2::CENTER_CENTER,
                "NO SIGNAL", FontId::proportional(32.0), theme::RED);
        }
    }
}

fn draw_rpm_bar(painter: &Painter, rect: Rect, rpm: f32, max: f32, redline: f32) {
    painter.rect_filled(rect, 4.0, BAR_BG);
    painter.rect_stroke(rect, 4.0, Stroke::new(1.0_f32, theme::GRAPHITE_MID), StrokeKind::Outside);

    let frac = (rpm / max).clamp(0.0, 1.0);
    let fill_h = rect.height() * frac;
    let fill_rect = Rect::from_min_size(
        Pos2::new(rect.left(), rect.bottom() - fill_h),
        Vec2::new(rect.width(), fill_h),
    );

    let color = if rpm >= redline {
        theme::RED
    } else if rpm >= redline * 0.75 {
        theme::ORANGE
    } else {
        theme::ORANGE_DIM
    };
    painter.rect_filled(fill_rect, 0.0, color);

    // RPM text
    painter.text(
        Pos2::new(rect.center().x, rect.top() + 20.0),
        Align2::CENTER_CENTER,
        &format!("{:.0}", rpm),
        FontId::proportional(14.0),
        theme::TEXT_PRIMARY,
    );
    painter.text(
        Pos2::new(rect.center().x, rect.top() + 35.0),
        Align2::CENTER_CENTER,
        "RPM",
        FontId::proportional(9.0),
        theme::TEXT_MUTED,
    );

    // Tick marks
    for i in 0..=8 {
        let y = rect.bottom() - rect.height() * (i as f32 / 8.0);
        let x_end = if i % 2 == 0 { rect.right() + 3.0 } else { rect.right() + 1.0 };
        painter.line_segment(
            [Pos2::new(rect.right(), y), Pos2::new(x_end, y)],
            Stroke::new(1.0_f32, theme::GRAPHITE_LIGHT),
        );
    }
}

fn draw_center(painter: &Painter, rect: Rect, data: &ClusterLayout) {
    let cx = rect.center().x;

    // Gear
    let gear_text = if data.gear == 0 { "N".to_string() } else { format!("{}", data.gear) };
    painter.text(
        Pos2::new(cx, rect.top() + rect.height() * 0.28),
        Align2::CENTER_CENTER,
        &gear_text,
        FontId::proportional(rect.height() * 0.30),
        theme::ORANGE,
    );
    painter.text(
        Pos2::new(cx, rect.top() + rect.height() * 0.44),
        Align2::CENTER_CENTER,
        "GEAR",
        FontId::proportional(rect.height() * 0.04),
        theme::TEXT_MUTED,
    );

    // Speed
    painter.text(
        Pos2::new(cx, rect.center().y + rect.height() * 0.12),
        Align2::CENTER_CENTER,
        &format!("{:.0}", data.speed),
        FontId::proportional(rect.height() * 0.28),
        theme::TEXT_PRIMARY,
    );
    painter.text(
        Pos2::new(cx, rect.center().y + rect.height() * 0.28),
        Align2::CENTER_CENTER,
        "km/h",
        FontId::proportional(rect.height() * 0.05),
        theme::TEXT_MUTED,
    );

    // RPM digital below speed
    painter.text(
        Pos2::new(cx, rect.bottom() - rect.height() * 0.08),
        Align2::CENTER_CENTER,
        &format!("{:.0} RPM", data.rpm),
        FontId::proportional(rect.height() * 0.06),
        theme::ORANGE_DIM,
    );
}

fn draw_readout_stack(painter: &Painter, rect: Rect, data: &ClusterLayout) {
    let items: Vec<(&str, &str, f32, Color32)> = vec![
        ("CLT", "°C", data.coolant_temp,
            if data.coolant_temp > 105.0 { theme::RED }
            else if data.coolant_temp > 95.0 { theme::STATUS_WARN }
            else { theme::TEXT_SECONDARY }),
        ("OIL", "°C", data.oil_temp,
            if data.oil_temp > 120.0 { theme::RED }
            else { theme::TEXT_SECONDARY }),
        ("BATT", "V", data.voltage,
            if data.voltage < 11.5 { theme::RED }
            else if data.voltage < 12.2 { theme::STATUS_WARN }
            else { theme::TEXT_SECONDARY }),
        ("TPS", "%", data.throttle, theme::TEXT_SECONDARY),
        ("MAP", "kPa", data.manifold_pressure, theme::TEXT_SECONDARY),
        ("IAT", "°C", data.intake_temp, theme::TEXT_SECONDARY),
    ];

    let item_h = rect.height() / items.len() as f32;

    for (i, (label, unit, val, color)) in items.iter().enumerate() {
        let y = rect.top() + i as f32 * item_h;
        let item_rect = Rect::from_min_size(
            Pos2::new(rect.left(), y),
            Vec2::new(rect.width(), item_h * 0.90),
        );

        painter.rect_filled(item_rect, 3.0, BAR_BG);

        // Label
        painter.text(
            Pos2::new(item_rect.left() + 8.0, item_rect.center().y - item_h * 0.12),
            Align2::LEFT_CENTER,
            *label,
            FontId::proportional(item_h * 0.22),
            theme::TEXT_MUTED,
        );

        // Value
        let val_text = if *val > 0.0 {
            if *unit == "V" { format!("{:.1}", val) }
            else { format!("{:.0}", val) }
        } else {
            "--".into()
        };
        painter.text(
            Pos2::new(item_rect.right() - 8.0, item_rect.center().y),
            Align2::RIGHT_CENTER,
            &val_text,
            FontId::proportional(item_h * 0.35),
            *color,
        );

        // Unit
        painter.text(
            Pos2::new(item_rect.right() - 8.0, item_rect.center().y + item_h * 0.22),
            Align2::RIGHT_CENTER,
            *unit,
            FontId::proportional(item_h * 0.14),
            theme::TEXT_MUTED,
        );
    }
}

fn draw_status_bar(painter: &Painter, rect: Rect, data: &ClusterLayout) {
    painter.line_segment(
        [Pos2::new(rect.left() + 15.0, rect.top()),
         Pos2::new(rect.right() - 15.0, rect.top())],
        Stroke::new(1.0_f32, theme::GRAPHITE_MID),
    );

    let gps_ok = data.gps.as_ref().map_or(false, |g| g.fix_valid);
    let gps_text = if gps_ok { "GPS OK" } else { "GPS --" };
    let dtc_text = if data.dtc_count > 0 {
        format!("DTC: {}", data.dtc_count)
    } else {
        "DTC: 0".into()
    };

    painter.text(
        Pos2::new(rect.left() + 20.0, rect.center().y),
        Align2::LEFT_CENTER,
        gps_text,
        FontId::proportional(rect.height() * 0.32),
        if gps_ok { theme::ORANGE } else { theme::TEXT_MUTED },
    );

    painter.text(
        Pos2::new(rect.center().x, rect.center().y),
        Align2::CENTER_CENTER,
        "Sport",
        FontId::proportional(rect.height() * 0.28),
        theme::TEXT_MUTED,
    );

    painter.text(
        Pos2::new(rect.right() - 20.0, rect.center().y),
        Align2::RIGHT_CENTER,
        &dtc_text,
        FontId::proportional(rect.height() * 0.32),
        if data.dtc_count > 0 { theme::RED } else { theme::TEXT_MUTED },
    );
}
