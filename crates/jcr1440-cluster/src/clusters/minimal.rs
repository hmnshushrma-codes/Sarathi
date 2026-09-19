//! Minimal — distraction-free speed + RPM + gear.
//!
//! Pure black background, large numerals only. No gauges, no chrome.
//! Designed for maximum readability while driving.

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect};

use super::ClusterRenderer;
use crate::gauges::ClusterLayout;
use crate::theme;

pub struct MinimalRenderer;

impl ClusterRenderer for MinimalRenderer {
    fn id(&self) -> &'static str { "minimal" }
    fn name(&self) -> &'static str { "Minimal" }
    fn description(&self) -> &'static str { "Distraction-free: speed, RPM, gear only" }

    fn draw(&self, painter: &Painter, rect: Rect, data: &ClusterLayout) {
        painter.rect_filled(rect, 0.0, Color32::from_rgb(2, 2, 2));

        let cx = rect.center().x;
        let h = rect.height();

        // Speed — dominant
        painter.text(
            Pos2::new(cx, rect.top() + h * 0.35),
            Align2::CENTER_CENTER,
            &format!("{:.0}", data.speed),
            FontId::proportional(h * 0.35),
            theme::TEXT_PRIMARY,
        );
        painter.text(
            Pos2::new(cx, rect.top() + h * 0.52),
            Align2::CENTER_CENTER,
            "km/h",
            FontId::proportional(h * 0.05),
            theme::TEXT_MUTED,
        );

        // Gear — right of speed
        let gear_text = if data.gear == 0 { "N".to_string() } else { format!("{}", data.gear) };
        painter.text(
            Pos2::new(rect.right() - rect.width() * 0.15, rect.top() + h * 0.35),
            Align2::CENTER_CENTER,
            &gear_text,
            FontId::proportional(h * 0.20),
            theme::ORANGE,
        );

        // RPM — bottom
        painter.text(
            Pos2::new(cx, rect.top() + h * 0.72),
            Align2::CENTER_CENTER,
            &format!("{:.0}", data.rpm),
            FontId::proportional(h * 0.12),
            if data.rpm > 6500.0 { theme::RED } else { theme::TEXT_DIM },
        );
        painter.text(
            Pos2::new(cx, rect.top() + h * 0.80),
            Align2::CENTER_CENTER,
            "RPM",
            FontId::proportional(h * 0.035),
            theme::TEXT_MUTED,
        );

        // Warnings at bottom
        let mut warnings = vec![];
        if data.coolant_temp > 105.0 { warnings.push("HIGH COOLANT"); }
        if data.voltage > 0.0 && data.voltage < 11.5 { warnings.push("LOW VOLTAGE"); }
        if data.oil_temp > 120.0 { warnings.push("HIGH OIL TEMP"); }
        if !data.connected { warnings.push("NO SIGNAL"); }

        if !warnings.is_empty() {
            let warn_text = warnings.join(" | ");
            painter.text(
                Pos2::new(cx, rect.bottom() - h * 0.05),
                Align2::CENTER_CENTER,
                &warn_text,
                FontId::proportional(h * 0.035),
                theme::RED,
            );
        }
    }
}
