//! Cluster renderer registry.
//!
//! Each cluster implements `ClusterRenderer` and is registered in `REGISTRY`.
//! The app calls `draw()` on whichever renderer is active — telemetry is shared,
//! only the presentation layer swaps.

pub mod classic;
pub mod minimal;
pub mod ninodash;
pub mod sport;

use egui::{Painter, Rect};

use crate::gauges::ClusterLayout;

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// A cluster layout that can render vehicle telemetry.
pub trait ClusterRenderer {
    /// Unique identifier (used for persistence).
    fn id(&self) -> &'static str;
    /// Human-readable name shown in the selector.
    fn name(&self) -> &'static str;
    /// Short description.
    fn description(&self) -> &'static str;
    /// Draw the cluster into the given rect.
    fn draw(&self, painter: &Painter, rect: Rect, data: &ClusterLayout);
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/// All available cluster renderers.
pub fn all_renderers() -> Vec<Box<dyn ClusterRenderer>> {
    vec![
        Box::new(ninodash::NinoDashRenderer),
        Box::new(classic::ClassicRenderer),
        Box::new(sport::SportRenderer),
        Box::new(minimal::MinimalRenderer),
    ]
}

/// Default cluster ID.
pub const DEFAULT_CLUSTER: &str = "ninodash-performance";

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

use std::path::PathBuf;

fn config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".config/ninodash/config.json")
}

pub fn load_selected_cluster() -> String {
    let path = config_path();
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(id) = val.get("selected_cluster").and_then(|v| v.as_str()) {
                return id.to_string();
            }
        }
    }
    DEFAULT_CLUSTER.to_string()
}

pub fn save_selected_cluster(id: &str) {
    let path = config_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let json = format!(r#"{{"selected_cluster":"{}"}}"#, id);
    let _ = std::fs::write(&path, json);
}
