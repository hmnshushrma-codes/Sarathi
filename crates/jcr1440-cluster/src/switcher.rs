//! Cluster switcher — overlay for selecting between cluster layouts.
//!
//! Two modes:
//!   1. Quick switch: "< Layout >" arrows at bottom of cluster, auto-hides
//!   2. Full selector: grid overlay triggered by tapping the quick switch

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, StrokeKind, Vec2};

use crate::clusters::{self, ClusterRenderer};
use crate::theme;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

pub struct SwitcherState {
    pub selected_idx: usize,
    pub overlay_open: bool,
    /// When the quick-switch hint was last shown (for auto-hide)
    pub last_interaction: std::time::Instant,
    pub show_hint: bool,
}

impl SwitcherState {
    pub fn new(initial_idx: usize) -> Self {
        Self {
            selected_idx: initial_idx,
            overlay_open: false,
            last_interaction: std::time::Instant::now(),
            show_hint: true,
        }
    }

    pub fn touch(&mut self) {
        self.last_interaction = std::time::Instant::now();
        self.show_hint = true;
    }

    pub fn hint_visible(&self) -> bool {
        self.show_hint && self.last_interaction.elapsed().as_secs() < 5
    }
}

// ---------------------------------------------------------------------------
// Quick-switch hint bar
// ---------------------------------------------------------------------------

/// Draw the quick-switch hint at bottom of screen.
/// Returns Some(delta) if user clicked prev/next, or toggles overlay.
pub fn draw_quick_switch(
    ui: &egui::Ui,
    painter: &Painter,
    rect: Rect,
    state: &SwitcherState,
    renderers: &[Box<dyn ClusterRenderer>],
) -> QuickSwitchAction {
    if !state.hint_visible() || state.overlay_open {
        return QuickSwitchAction::None;
    }

    let bar_h = 36.0;
    let bar_w = rect.width() * 0.35;
    let bar_rect = Rect::from_center_size(
        Pos2::new(rect.center().x, rect.bottom() - bar_h * 0.6),
        Vec2::new(bar_w, bar_h),
    );

    // Semi-transparent background
    painter.rect_filled(bar_rect, 18.0,
        Color32::from_rgba_premultiplied(10, 10, 10, 180));
    painter.rect_stroke(bar_rect, 18.0,
        Stroke::new(1.0_f32, theme::GRAPHITE_MID), StrokeKind::Outside);

    let name = renderers.get(state.selected_idx)
        .map(|r| r.name()).unwrap_or("---");

    // Prev arrow
    let prev_pos = Pos2::new(bar_rect.left() + 25.0, bar_rect.center().y);
    painter.text(prev_pos, Align2::CENTER_CENTER, "<",
        FontId::proportional(18.0), theme::TEXT_MUTED);

    // Name
    painter.text(bar_rect.center(), Align2::CENTER_CENTER, name,
        FontId::proportional(13.0), theme::TEXT_SECONDARY);

    // Next arrow
    let next_pos = Pos2::new(bar_rect.right() - 25.0, bar_rect.center().y);
    painter.text(next_pos, Align2::CENTER_CENTER, ">",
        FontId::proportional(18.0), theme::TEXT_MUTED);

    // Handle clicks
    if let Some(pos) = ui.input(|i| {
        if i.pointer.any_released() { i.pointer.interact_pos() } else { None }
    }) {
        if bar_rect.contains(pos) {
            let rel_x = (pos.x - bar_rect.left()) / bar_rect.width();
            if rel_x < 0.25 {
                return QuickSwitchAction::Prev;
            } else if rel_x > 0.75 {
                return QuickSwitchAction::Next;
            } else {
                return QuickSwitchAction::OpenOverlay;
            }
        }
    }

    QuickSwitchAction::None
}

pub enum QuickSwitchAction {
    None,
    Prev,
    Next,
    OpenOverlay,
}

// ---------------------------------------------------------------------------
// Full selector overlay
// ---------------------------------------------------------------------------

/// Draw the full cluster selector overlay.
/// Returns Some(index) if user selected a cluster, None otherwise.
pub fn draw_selector_overlay(
    ui: &egui::Ui,
    painter: &Painter,
    rect: Rect,
    selected_idx: usize,
    renderers: &[Box<dyn ClusterRenderer>],
) -> SelectorAction {
    // Dim background
    painter.rect_filled(rect, 0.0, theme::OVERLAY_BG);

    let panel_w = rect.width() * 0.80;
    let panel_h = rect.height() * 0.70;
    let panel = Rect::from_center_size(rect.center(), Vec2::new(panel_w, panel_h));

    // Panel background
    painter.rect_filled(panel, 8.0, theme::BG_PRIMARY);
    painter.rect_stroke(panel, 8.0,
        Stroke::new(1.0_f32, theme::ORANGE_DIM), StrokeKind::Outside);

    // Title
    painter.text(
        Pos2::new(panel.left() + 25.0, panel.top() + 30.0),
        Align2::LEFT_CENTER,
        "SELECT CLUSTER",
        FontId::proportional(18.0),
        theme::TEXT_PRIMARY,
    );

    // Close button
    let close_pos = Pos2::new(panel.right() - 25.0, panel.top() + 30.0);
    painter.text(close_pos, Align2::RIGHT_CENTER, "X",
        FontId::proportional(16.0), theme::TEXT_MUTED);

    // Separator
    painter.line_segment(
        [Pos2::new(panel.left() + 15.0, panel.top() + 50.0),
         Pos2::new(panel.right() - 15.0, panel.top() + 50.0)],
        Stroke::new(1.0_f32, theme::GRAPHITE_MID),
    );

    // Cluster cards — grid
    let cards_top = panel.top() + 65.0;
    let cards_h = panel.height() - 130.0;
    let num = renderers.len();
    let cols = if num <= 2 { num } else { (num + 1) / 2 }.max(1);
    let rows = (num + cols - 1) / cols;
    let card_w = (panel_w - 40.0) / cols as f32 - 10.0;
    let card_h = (cards_h / rows as f32) - 10.0;

    let mut clicked_idx = None;

    for (i, renderer) in renderers.iter().enumerate() {
        let col = i % cols;
        let row = i / cols;
        let x = panel.left() + 20.0 + col as f32 * (card_w + 10.0);
        let y = cards_top + row as f32 * (card_h + 10.0);
        let card_rect = Rect::from_min_size(Pos2::new(x, y), Vec2::new(card_w, card_h));

        let is_selected = i == selected_idx;
        let border_color = if is_selected { theme::ORANGE } else { theme::GRAPHITE_MID };

        // Card background
        painter.rect_filled(card_rect, 6.0, theme::CARD_BG);
        painter.rect_stroke(card_rect, 6.0,
            Stroke::new(if is_selected { 2.0_f32 } else { 1.0_f32 }, border_color),
            StrokeKind::Outside);

        // Preview area (placeholder with ID text)
        let preview_h = card_h * 0.55;
        let preview_rect = Rect::from_min_size(
            Pos2::new(x + 8.0, y + 8.0),
            Vec2::new(card_w - 16.0, preview_h),
        );
        painter.rect_filled(preview_rect, 3.0, theme::BG_BLACK);

        // Mini preview identifiers
        let pc = preview_rect.center();
        painter.text(pc, Align2::CENTER_CENTER,
            renderer.id().split('-').next().unwrap_or("?"),
            FontId::proportional(preview_h * 0.25),
            theme::GRAPHITE_LIGHT,
        );

        // Name
        let name_y = y + preview_h + 20.0;
        painter.text(
            Pos2::new(x + card_w / 2.0, name_y),
            Align2::CENTER_CENTER,
            renderer.name(),
            FontId::proportional(14.0),
            if is_selected { theme::ORANGE } else { theme::TEXT_PRIMARY },
        );

        // Description
        painter.text(
            Pos2::new(x + card_w / 2.0, name_y + 18.0),
            Align2::CENTER_CENTER,
            renderer.description(),
            FontId::proportional(9.0),
            theme::TEXT_MUTED,
        );

        // Selected indicator
        if is_selected {
            painter.text(
                Pos2::new(x + card_w / 2.0, y + card_h - 12.0),
                Align2::CENTER_CENTER,
                "ACTIVE",
                FontId::proportional(9.0),
                theme::ORANGE,
            );
        }

        // Click detection
        if let Some(pos) = ui.input(|i| {
            if i.pointer.any_released() { i.pointer.interact_pos() } else { None }
        }) {
            if card_rect.contains(pos) {
                clicked_idx = Some(i);
            }
            // Close button
            if (pos - close_pos).length() < 20.0 {
                return SelectorAction::Close;
            }
        }
    }

    // Click outside panel to close
    if let Some(pos) = ui.input(|i| {
        if i.pointer.any_released() { i.pointer.interact_pos() } else { None }
    }) {
        if !panel.contains(pos) {
            return SelectorAction::Close;
        }
    }

    if let Some(idx) = clicked_idx {
        SelectorAction::Select(idx)
    } else {
        SelectorAction::None
    }
}

pub enum SelectorAction {
    None,
    Close,
    Select(usize),
}
