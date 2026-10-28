//! Colors, the egui style, and the small widgets every page shares.
use eframe::egui::{self, Color32, RichText, Vec2};

pub const BLUE: Color32 = Color32::from_rgb(79, 70, 229);
pub const INK: Color32 = Color32::from_rgb(30, 33, 43);
pub const MUTED: Color32 = Color32::from_rgb(100, 108, 124);
pub const BORDER: Color32 = Color32::from_rgb(228, 230, 237);

/// Fonts and style for the whole app.
pub fn apply(ctx: &egui::Context) {
    // Use the Windows UI typeface when present, with egui's bundled fallback.
    let mut fonts = egui::FontDefinitions::default();
    if let Ok(data) = std::fs::read("C:\\Windows\\Fonts\\segoeui.ttf") {
        fonts
            .font_data
            .insert("Segoe UI".into(), egui::FontData::from_owned(data).into());
        fonts
            .families
            .get_mut(&egui::FontFamily::Proportional)
            .unwrap()
            .insert(0, "Segoe UI".into());
    }
    ctx.set_fonts(fonts);
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::light();
    style.visuals.override_text_color = Some(INK);
    style.visuals.panel_fill = Color32::from_rgb(246, 247, 250);
    style.visuals.window_fill = Color32::WHITE;
    style.visuals.selection.bg_fill = BLUE;
    style.visuals.selection.stroke.color = Color32::WHITE;
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(232, 234, 242);
    style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(246, 247, 251);
    style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1., BORDER);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(239, 238, 255);
    style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1., BLUE);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(223, 235, 255);
    style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1., BORDER);
    style.visuals.extreme_bg_color = Color32::from_rgb(250, 250, 253);
    for widget in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.noninteractive,
    ] {
        widget.corner_radius = egui::CornerRadius::same(8);
    }
    style.spacing.interact_size = Vec2::new(40., 36.);
    style.spacing.item_spacing = Vec2::new(10., 10.);
    style.spacing.button_padding = Vec2::new(18., 10.);
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(15.));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(15.));
    style
        .text_styles
        .insert(egui::TextStyle::Small, egui::FontId::proportional(12.));
    ctx.set_style(style);
}

pub fn page_header(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.add_space(6.);
    ui.label(RichText::new(title).size(32.).strong());
    ui.label(RichText::new(subtitle).size(14.).color(MUTED));
    ui.add_space(22.);
}

pub fn primary(ui: &mut egui::Ui, text: &str, enabled: bool) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(text).color(Color32::WHITE).strong())
            .fill(BLUE)
            .stroke(egui::Stroke::NONE)
            .corner_radius(9)
            .min_size(Vec2::new(0., 42.)),
    )
}
pub fn card<R>(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(Color32::WHITE)
        .stroke(egui::Stroke::new(1., BORDER))
        .corner_radius(16)
        .inner_margin(24)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            contents(ui)
        })
        .inner
}

pub fn icon_tile(ui: &mut egui::Ui, size: f32, background: Color32, foreground: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    ui.painter().rect_filled(rect, size * 0.25, background);
    let center = rect.center();
    let stroke = egui::Stroke::new(2., foreground);
    ui.painter().line_segment(
        [
            center - egui::vec2(0., size * 0.19),
            center + egui::vec2(0., size * 0.16),
        ],
        stroke,
    );
    ui.painter().line_segment(
        [
            center + egui::vec2(-size * 0.13, size * 0.03),
            center + egui::vec2(0., size * 0.16),
        ],
        stroke,
    );
    ui.painter().line_segment(
        [
            center + egui::vec2(size * 0.13, size * 0.03),
            center + egui::vec2(0., size * 0.16),
        ],
        stroke,
    );
}
