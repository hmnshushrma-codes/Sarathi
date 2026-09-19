//! NinoDash Design System — centralized theme tokens.
//!
//! All color values, spacing, and style constants live here.
//! Individual clusters may define their own palettes, but system-wide
//! overlays (switcher, warnings, preflight) use these.

use egui::Color32;

// ---------------------------------------------------------------------------
// Background tiers
// ---------------------------------------------------------------------------

pub const BG_BLACK: Color32 = Color32::from_rgb(5, 5, 5);
pub const BG_DARK: Color32 = Color32::from_rgb(10, 10, 10);
pub const BG_PRIMARY: Color32 = Color32::from_rgb(17, 17, 17);

// ---------------------------------------------------------------------------
// Graphite / panel
// ---------------------------------------------------------------------------

pub const GRAPHITE_DARK: Color32 = Color32::from_rgb(24, 24, 24);
pub const GRAPHITE_MID: Color32 = Color32::from_rgb(36, 36, 36);
pub const GRAPHITE_LIGHT: Color32 = Color32::from_rgb(48, 48, 48);

// ---------------------------------------------------------------------------
// Metallic / text
// ---------------------------------------------------------------------------

pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(244, 244, 244);
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(216, 216, 216);
pub const TEXT_DIM: Color32 = Color32::from_rgb(168, 168, 168);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(118, 118, 118);

// ---------------------------------------------------------------------------
// NinoDash Orange — primary accent
// ---------------------------------------------------------------------------

pub const ORANGE: Color32 = Color32::from_rgb(255, 98, 0);
pub const ORANGE_BRIGHT: Color32 = Color32::from_rgb(255, 120, 0);
pub const ORANGE_DIM: Color32 = Color32::from_rgb(180, 70, 0);
pub const ORANGE_GLOW: Color32 = Color32::from_rgba_premultiplied(255, 98, 0, 60);

// ---------------------------------------------------------------------------
// Red — warnings, critical, over-rev
// ---------------------------------------------------------------------------

pub const RED: Color32 = Color32::from_rgb(227, 24, 24);
pub const RED_BRIGHT: Color32 = Color32::from_rgb(255, 42, 0);
pub const RED_DIM: Color32 = Color32::from_rgb(160, 20, 15);
pub const RED_GLOW: Color32 = Color32::from_rgba_premultiplied(227, 24, 24, 50);

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

pub const STATUS_READY: Color32 = Color32::from_rgb(255, 98, 0); // orange = ready
pub const STATUS_OK: Color32 = Color32::from_rgb(30, 200, 80);
pub const STATUS_WARN: Color32 = Color32::from_rgb(255, 160, 0);
pub const STATUS_FAIL: Color32 = Color32::from_rgb(227, 24, 24);
pub const STATUS_CHECK: Color32 = Color32::from_rgb(168, 168, 168);
pub const STATUS_INACTIVE: Color32 = Color32::from_rgb(60, 60, 60);

// ---------------------------------------------------------------------------
// Overlay
// ---------------------------------------------------------------------------

pub const OVERLAY_BG: Color32 = Color32::from_rgba_premultiplied(5, 5, 5, 220);
pub const CARD_BG: Color32 = Color32::from_rgb(20, 20, 24);
pub const CARD_BORDER: Color32 = Color32::from_rgb(40, 40, 48);
pub const CARD_SELECTED: Color32 = Color32::from_rgb(255, 98, 0);

// ---------------------------------------------------------------------------
// Cluster-neutral gauge colors
// ---------------------------------------------------------------------------

pub const GAUGE_BG: Color32 = Color32::from_rgb(18, 18, 22);
pub const GAUGE_RING: Color32 = Color32::from_rgb(50, 50, 55);
pub const NEEDLE_COLOR: Color32 = Color32::from_rgb(255, 98, 0);
pub const NEEDLE_GLOW_COLOR: Color32 = Color32::from_rgba_premultiplied(255, 98, 0, 50);

// ---------------------------------------------------------------------------
// Button
// ---------------------------------------------------------------------------

pub const BTN_PRIMARY: Color32 = Color32::from_rgb(255, 98, 0);
pub const BTN_PRIMARY_HOVER: Color32 = Color32::from_rgb(255, 120, 0);
pub const BTN_DISABLED: Color32 = Color32::from_rgb(40, 40, 45);
pub const BTN_TEXT: Color32 = Color32::from_rgb(5, 5, 5);
pub const BTN_TEXT_DISABLED: Color32 = Color32::from_rgb(80, 80, 85);
