//! NinoDash boot splash — branded startup with logo image.
//!
//! Sequence (~3 seconds):
//!   1. Black screen
//!   2. Logo fades in (loaded from assets/nino_dash.png)
//!   3. Subtitle appears
//!   4. Progress bar fills
//!   5. Transition to preflight

use std::path::PathBuf;
use std::time::Instant;

use egui::{Align2, Color32, ColorImage, FontId, Painter, Pos2, Rect, TextureHandle, Vec2};

use crate::theme;

const SPLASH_DURATION: f32 = 3.5; // seconds

pub struct SplashState {
    start: Instant,
    logo_texture: Option<TextureHandle>,
    logo_loaded: bool,
}

impl SplashState {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            logo_texture: None,
            logo_loaded: false,
        }
    }

    pub fn elapsed(&self) -> f32 {
        self.start.elapsed().as_secs_f32()
    }

    pub fn is_done(&self) -> bool {
        self.elapsed() > SPLASH_DURATION
    }
}

/// Try to find the logo image in common locations.
fn find_logo() -> Option<PathBuf> {
    let candidates = [
        // Development paths
        "assets/nino_dash.png",
        "../assets/nino_dash.png",
        "../../assets/nino_dash.png",
        // Installed path
        "/opt/jcr1440/assets/nino_dash.png",
        "/opt/saarthi/assets/nino_dash.png",
    ];

    // Also try relative to executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("assets/nino_dash.png");
            if p.exists() { return Some(p); }
            let p = dir.join("../assets/nino_dash.png");
            if p.exists() { return Some(p); }
        }
    }

    for c in &candidates {
        let p = PathBuf::from(c);
        if p.exists() { return Some(p); }
    }
    None
}

/// Load the logo PNG into an egui texture.
fn load_logo_texture(ctx: &egui::Context) -> Option<TextureHandle> {
    let path = find_logo()?;
    let img = image::open(&path).ok()?.into_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let pixels = img.into_raw();
    let color_image = ColorImage::from_rgba_unmultiplied(size, &pixels);
    Some(ctx.load_texture("ninodash-logo", color_image, egui::TextureOptions::LINEAR))
}

/// Draw the splash screen. Returns true when animation is complete.
pub fn draw_splash(ctx: &egui::Context, painter: &Painter, rect: Rect, state: &mut SplashState) -> bool {
    let t = state.elapsed();
    let w = rect.width();
    let h = rect.height();
    let cx = rect.center().x;
    let cy = rect.center().y;

    // Background
    painter.rect_filled(rect, 0.0, Color32::from_rgb(2, 2, 2));

    // Load logo texture on first frame
    if !state.logo_loaded {
        state.logo_texture = load_logo_texture(ctx);
        state.logo_loaded = true;
    }

    // Phase 1 (0.0 - 0.4s): Black
    // Phase 2 (0.3 - 1.5s): Logo fades in
    if t > 0.3 {
        let alpha = ((t - 0.3) / 0.8).clamp(0.0, 1.0);

        if let Some(ref tex) = state.logo_texture {
            // Draw the actual logo image, centered, scaled to fit ~60% of width
            let tex_size = tex.size_vec2();
            let aspect = tex_size.x / tex_size.y;
            let target_w = w * 0.55;
            let target_h = target_w / aspect;

            let logo_rect = Rect::from_center_size(
                Pos2::new(cx, cy - h * 0.06),
                Vec2::new(target_w, target_h),
            );

            // Tint with alpha for fade-in
            let a = (alpha * 255.0) as u8;
            let tint = Color32::from_rgba_unmultiplied(255, 255, 255, a);

            painter.image(tex.id(), logo_rect, Rect::from_min_max(
                Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)), tint);
        } else {
            // Fallback: text-based logo if image not found
            let a = (alpha * 244.0) as u8;
            let nino_color = Color32::from_rgba_premultiplied(a, a, a, a);
            let r = (alpha * 255.0) as u8;
            let g = (alpha * 98.0) as u8;
            let dash_color = Color32::from_rgba_premultiplied(r, g, 0, (alpha * 255.0) as u8);

            painter.text(
                Pos2::new(cx - 5.0, cy - h * 0.04),
                Align2::RIGHT_CENTER,
                "Nino",
                FontId::proportional(h * 0.14),
                nino_color,
            );
            painter.text(
                Pos2::new(cx + 5.0, cy - h * 0.04),
                Align2::LEFT_CENTER,
                "Dash",
                FontId::proportional(h * 0.14),
                dash_color,
            );
        }
    }

    // Phase 3 (1.5 - 2.5s): Subtitle
    if t > 1.5 {
        let alpha = ((t - 1.5) / 0.6).clamp(0.0, 1.0);
        let a = (alpha * 168.0) as u8;
        let sub_color = Color32::from_rgba_premultiplied(a, a, a, a);
        painter.text(
            Pos2::new(cx, cy + h * 0.14),
            Align2::CENTER_CENTER,
            "PERFORMANCE DIGITAL CLUSTER",
            FontId::proportional(h * 0.025),
            sub_color,
        );
    }

    // Phase 4 (2.0 - 3.0s): Initializing + progress bar
    if t > 2.0 {
        let alpha = ((t - 2.0) / 0.3).clamp(0.0, 1.0);
        let a = (alpha * 118.0) as u8;
        let init_color = Color32::from_rgba_premultiplied(a, a, a, a);
        painter.text(
            Pos2::new(cx, cy + h * 0.24),
            Align2::CENTER_CENTER,
            "Initializing systems...",
            FontId::proportional(h * 0.020),
            init_color,
        );

        // Progress bar
        let bar_w = w * 0.22;
        let bar_h = 3.0;
        let bar_x = cx - bar_w / 2.0;
        let bar_y = cy + h * 0.30;
        let bar_rect = Rect::from_min_size(
            Pos2::new(bar_x, bar_y), Vec2::new(bar_w, bar_h));
        painter.rect_filled(bar_rect, 1.0, theme::GRAPHITE_DARK);

        let progress = ((t - 2.0) / 1.2).clamp(0.0, 1.0);
        let fill = Rect::from_min_size(
            Pos2::new(bar_x, bar_y), Vec2::new(bar_w * progress, bar_h));
        painter.rect_filled(fill, 1.0, theme::ORANGE);
    }

    state.is_done()
}
