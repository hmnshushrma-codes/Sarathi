//! Classic — the original Haltech-inspired dual-gauge cluster.
//!
//! Wraps the existing `gauges::draw_full_cluster` as a `ClusterRenderer`.

use egui::{Painter, Rect};

use super::ClusterRenderer;
use crate::gauges::{self, ClusterLayout};

pub struct ClassicRenderer;

impl ClusterRenderer for ClassicRenderer {
    fn id(&self) -> &'static str { "classic" }
    fn name(&self) -> &'static str { "Classic" }
    fn description(&self) -> &'static str { "Haltech iC-7 inspired dual sweep gauges" }

    fn draw(&self, painter: &Painter, rect: Rect, data: &ClusterLayout) {
        gauges::draw_full_cluster(painter, rect, data);
    }
}
