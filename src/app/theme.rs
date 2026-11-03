//! The design tokens, the egui style, and the small widgets every page shares.
use eframe::egui::{self, Color32, FontFamily, RichText, Vec2};

pub const ACCENT: Color32 = Color32::from_rgb(79, 70, 229);
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(67, 56, 202);
pub const ACCENT_PRESSED: Color32 = Color32::from_rgb(55, 48, 163);
pub const ACCENT_TINT: Color32 = Color32::from_rgb(239, 238, 255);
pub const ACCENT_TINT_STRONG: Color32 = Color32::from_rgb(224, 222, 252);
/// Hovering is quieter than being selected, which uses `ACCENT_TINT` and `ACCENT`.
pub const HOVER_FILL: Color32 = Color32::from_rgb(248, 247, 255);
pub const HOVER_BORDER: Color32 = Color32::from_rgb(178, 172, 240);
pub const INK: Color32 = Color32::from_rgb(30, 33, 43);
pub const MUTED: Color32 = Color32::from_rgb(100, 108, 124);
pub const BORDER: Color32 = Color32::from_rgb(228, 230, 237);
pub const PAGE: Color32 = Color32::from_rgb(246, 247, 250);
pub const CONTROL: Color32 = Color32::from_rgb(246, 247, 251);
pub const TRACK: Color32 = Color32::from_rgb(232, 234, 242);
pub const RAIL: Color32 = Color32::from_rgb(26, 29, 40);
pub const RAIL_HOVER: Color32 = Color32::from_rgb(42, 46, 62);
pub const RAIL_TEXT: Color32 = Color32::from_rgb(184, 190, 208);
pub const SUCCESS: Color32 = Color32::from_rgb(22, 120, 84);
pub const WARNING: Color32 = Color32::from_rgb(150, 84, 14);
pub const DANGER: Color32 = Color32::from_rgb(175, 46, 57);
pub const DANGER_TINT: Color32 = Color32::from_rgb(255, 240, 240);

pub const TEXT_CAPTION: f32 = 12.;
pub const TEXT_BODY: f32 = 14.;
pub const TEXT_SECTION: f32 = 16.;
pub const TEXT_HEADING: f32 = 20.;
pub const TEXT_TITLE: f32 = 28.;

pub const SPACE_XS: f32 = 4.;
pub const SPACE_S: f32 = 8.;
pub const SPACE_M: f32 = 12.;
pub const SPACE_L: f32 = 16.;
pub const SPACE_XL: f32 = 24.;
pub const PAGE_MARGIN: i8 = 32;

pub const RADIUS_S: u8 = 6;
pub const RADIUS_M: u8 = 8;
pub const RADIUS_L: u8 = 12;

const CONTROL_HEIGHT: f32 = 36.;
const SEMIBOLD: &str = "Semibold";

fn semibold_family() -> FontFamily {
    FontFamily::Name(SEMIBOLD.into())
}

/// Fonts and style for the whole app.
pub fn apply(ctx: &egui::Context) {
    // Use the Windows UI typefaces when present, with egui's bundled fallback.
    let mut fonts = egui::FontDefinitions::default();
    if let Ok(data) = std::fs::read("C:\\Windows\\Fonts\\segoeui.ttf") {
        fonts
            .font_data
            .insert("Segoe UI".into(), egui::FontData::from_owned(data).into());
        fonts
            .families
            .get_mut(&FontFamily::Proportional)
            .unwrap()
            .insert(0, "Segoe UI".into());
    }
    // Titles and file names hold symbols and emoji that Segoe UI lacks; egui draws them in one
    // color. Without these, they show as empty boxes.
    for (name, file) in [
        ("Segoe UI Symbol", "seguisym.ttf"),
        ("Segoe UI Emoji", "seguiemj.ttf"),
    ] {
        if let Ok(data) = std::fs::read(format!("C:\\Windows\\Fonts\\{file}")) {
            fonts
                .font_data
                .insert(name.into(), egui::FontData::from_owned(data).into());
            for family in [FontFamily::Proportional, FontFamily::Monospace] {
                fonts.families.get_mut(&family).unwrap().push(name.into());
            }
        }
    }
    // egui has no font weights: Semibold is its own family, falling back to the regular text.
    let mut semibold = fonts.families[&FontFamily::Proportional].clone();
    if let Ok(data) = std::fs::read("C:\\Windows\\Fonts\\seguisb.ttf") {
        fonts.font_data.insert(
            "Segoe UI Semibold".into(),
            egui::FontData::from_owned(data).into(),
        );
        semibold.insert(0, "Segoe UI Semibold".into());
    }
    fonts.families.insert(semibold_family(), semibold);
    ctx.set_fonts(fonts);

    let mut style = (*ctx.style()).clone();
    let visuals = &mut style.visuals;
    *visuals = egui::Visuals::light();
    visuals.panel_fill = PAGE;
    visuals.window_fill = Color32::WHITE;
    visuals.hyperlink_color = ACCENT;
    visuals.selection.bg_fill = ACCENT;
    visuals.selection.stroke.color = Color32::WHITE;
    visuals.extreme_bg_color = Color32::from_rgb(250, 250, 253);
    visuals.slider_trailing_fill = true;
    visuals.handle_shape = egui::style::HandleShape::Circle;
    // Buttons fill with `weak_bg_fill`; sliders and stock checkboxes use `bg_fill`.
    let widgets = &mut visuals.widgets;
    widgets.inactive.bg_fill = TRACK;
    widgets.inactive.weak_bg_fill = CONTROL;
    widgets.inactive.bg_stroke = egui::Stroke::new(1., BORDER);
    widgets.hovered.bg_fill = ACCENT_TINT;
    widgets.hovered.weak_bg_fill = HOVER_FILL;
    widgets.hovered.bg_stroke = egui::Stroke::new(1., HOVER_BORDER);
    widgets.active.bg_fill = ACCENT_TINT_STRONG;
    widgets.active.weak_bg_fill = ACCENT_TINT_STRONG;
    widgets.active.bg_stroke = egui::Stroke::new(1., HOVER_BORDER);
    widgets.open.weak_bg_fill = HOVER_FILL;
    widgets.noninteractive.bg_stroke = egui::Stroke::new(1., BORDER);
    for widget in [
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
        &mut widgets.open,
        &mut widgets.noninteractive,
    ] {
        widget.corner_radius = egui::CornerRadius::same(RADIUS_M);
        widget.fg_stroke.color = INK;
        // The default growth on hover makes neighbouring controls look like they jump.
        widget.expansion = 0.;
    }
    style.spacing.interact_size = Vec2::new(40., CONTROL_HEIGHT);
    style.spacing.item_spacing = Vec2::new(SPACE_S, SPACE_S);
    style.spacing.button_padding = Vec2::new(SPACE_L, SPACE_S);
    style.spacing.slider_width = 220.;
    style.spacing.slider_rail_height = 6.;
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(TEXT_BODY));
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::proportional(TEXT_BODY),
    );
    style.text_styles.insert(
        egui::TextStyle::Small,
        egui::FontId::proportional(TEXT_CAPTION),
    );
    ctx.set_style(style);
}

/// Text in the semibold weight; `RichText::strong` only changes the color.
pub fn semibold(text: impl Into<String>) -> RichText {
    RichText::new(text).family(semibold_family())
}

/// Secondary text: hints, paths and explanations.
pub fn caption(text: impl Into<String>) -> RichText {
    RichText::new(text).size(TEXT_CAPTION).color(MUTED)
}

/// The title of a card or a section in one.
pub fn section_title(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(semibold(text).size(TEXT_SECTION));
}

const TITLE_ROWS: usize = 2;

fn title_job(text: &str, width: f32) -> egui::text::LayoutJob {
    let font = egui::FontId::new(TEXT_SECTION, semibold_family());
    let mut job = egui::text::LayoutJob::simple(text.to_owned(), font, INK, width);
    job.wrap.max_rows = TITLE_ROWS;
    job
}

/// A title from a website, which can be a whole caption: at most two rows, in full on hover.
pub fn clamped_title(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let width = ui.available_width();
    ui.add(egui::Label::new(title_job(text, width)))
        .on_hover_text(text)
}

pub fn page_header(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.label(semibold(title).size(TEXT_TITLE));
    ui.label(RichText::new(subtitle).color(MUTED));
    ui.add_space(SPACE_L);
}

/// A button whose fill follows the pointer, which an explicit `Button::fill` would not.
fn filled_button(
    ui: &mut egui::Ui,
    button: egui::Button<'_>,
    enabled: bool,
    [normal, hovered, pressed]: [Color32; 3],
) -> egui::Response {
    ui.scope(|ui| {
        let widgets = &mut ui.visuals_mut().widgets;
        for (widget, fill) in [
            (&mut widgets.inactive, normal),
            (&mut widgets.hovered, hovered),
            (&mut widgets.active, pressed),
        ] {
            widget.weak_bg_fill = fill;
            widget.bg_stroke = egui::Stroke::NONE;
        }
        ui.add_enabled(enabled, button)
    })
    .inner
}

pub fn primary(ui: &mut egui::Ui, text: &str, enabled: bool) -> egui::Response {
    filled_button(
        ui,
        egui::Button::new(semibold(text).color(Color32::WHITE))
            .min_size(Vec2::new(0., CONTROL_HEIGHT)),
        enabled,
        [ACCENT, ACCENT_HOVER, ACCENT_PRESSED],
    )
}

/// An item in the sidebar, which fills its width.
pub fn nav_item(ui: &mut egui::Ui, text: RichText, active: bool, height: f32) -> egui::Response {
    let text = text.color(if active { Color32::WHITE } else { RAIL_TEXT });
    let width = ui.available_width();
    filled_button(
        ui,
        egui::Button::new(text).min_size(Vec2::new(width, height)),
        true,
        if active {
            [ACCENT; 3]
        } else {
            [RAIL, RAIL_HOVER, RAIL_HOVER]
        },
    )
}

pub fn card<R>(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(Color32::WHITE)
        .stroke(egui::Stroke::new(1., BORDER))
        .corner_radius(RADIUS_L)
        .inner_margin(SPACE_XL)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            contents(ui)
        })
        .inner
}

/// An error banner, as wide as the cards around it.
pub fn error_banner<R>(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(DANGER_TINT)
        .corner_radius(RADIUS_L)
        .inner_margin(SPACE_L)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            contents(ui)
        })
        .inner
}

/// The ring on a focused text field. egui draws one in `selection.stroke`, which is white here
/// so that selected text stays readable on the accent color.
pub fn focus_ring(ui: &egui::Ui, response: &egui::Response) {
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect,
            RADIUS_M,
            egui::Stroke::new(1.5, ACCENT),
            egui::StrokeKind::Outside,
        );
    }
}

/// A progress bar with its text beside it, never over a fill it might not contrast with.
/// Without a fraction, the amount of work is unknown and a spinner shows instead.
pub fn progress(ui: &mut egui::Ui, label: &str, fraction: Option<f32>) {
    ui.horizontal(|ui| {
        if fraction.is_none() {
            ui.spinner();
        }
        ui.label(semibold(label));
        if let Some(fraction) = fraction {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(semibold(format!("{:.0}%", fraction * 100.)));
            });
        }
    });
    if let Some(fraction) = fraction {
        ui.scope(|ui| {
            ui.visuals_mut().extreme_bg_color = TRACK;
            ui.add(
                egui::ProgressBar::new(fraction)
                    .desired_width(ui.available_width())
                    .desired_height(SPACE_S)
                    .corner_radius(SPACE_XS as u8)
                    .fill(ACCENT),
            );
        });
    }
}

/// A slider with a ring handle in the accent color.
pub fn slider(ui: &mut egui::Ui, slider: egui::Slider<'_>) -> egui::Response {
    ui.scope(|ui| {
        let widgets = &mut ui.visuals_mut().widgets;
        for widget in [
            &mut widgets.inactive,
            &mut widgets.hovered,
            &mut widgets.active,
        ] {
            widget.fg_stroke = egui::Stroke::new(2., ACCENT);
        }
        ui.add(slider)
    })
    .inner
}

const MARK_SIZE: f32 = 18.;

fn mark_border(on: bool, response: Option<&egui::Response>) -> egui::Stroke {
    let lit = on || response.is_some_and(|r| r.hovered() || r.has_focus());
    egui::Stroke::new(1.5, if lit { ACCENT } else { MUTED })
}

/// The radio mark of a row that handles its own clicks.
pub fn radio_mark(ui: &mut egui::Ui, selected: bool, row: Option<&egui::Response>) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(MARK_SIZE), egui::Sense::hover());
    let center = rect.center();
    let border = mark_border(selected, row);
    ui.painter().circle_stroke(center, 8., border);
    if selected {
        ui.painter().circle_filled(center, 4.5, ACCENT);
    }
}

fn paint_check(ui: &egui::Ui, rect: egui::Rect, color: Color32) {
    let at = |x: f32, y: f32| rect.min + rect.size() * Vec2::new(x, y);
    ui.painter().line(
        vec![at(0.24, 0.52), at(0.42, 0.7), at(0.76, 0.32)],
        egui::Stroke::new(2., color),
    );
}

pub fn checkbox(ui: &mut egui::Ui, value: &mut bool, text: &str) -> egui::Response {
    let gap = SPACE_S;
    let wrap = ui.available_width() - MARK_SIZE - gap;
    let galley = egui::WidgetText::from(text).into_galley(ui, None, wrap, egui::TextStyle::Body);
    let size = Vec2::new(
        MARK_SIZE + gap + galley.size().x,
        galley.size().y.max(MARK_SIZE),
    );
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click());
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    let mark = egui::Rect::from_min_size(
        egui::pos2(rect.left(), rect.center().y - MARK_SIZE / 2.),
        Vec2::splat(MARK_SIZE),
    );
    let border = mark_border(*value, Some(&response));
    let fill = if *value { ACCENT } else { Color32::WHITE };
    ui.painter()
        .rect(mark, 4., fill, border, egui::StrokeKind::Inside);
    if *value {
        paint_check(ui, mark, Color32::WHITE);
    }
    let text_at = egui::pos2(mark.right() + gap, rect.center().y - galley.size().y / 2.);
    ui.painter().galley(text_at, galley, INK);
    response
}

/// A semibold line with a painted check: the typeface has no check mark on every platform.
pub fn success_line(ui: &mut egui::Ui, text: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(MARK_SIZE), egui::Sense::hover());
        paint_check(ui, rect, SUCCESS);
        ui.label(semibold(text).color(SUCCESS));
    });
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

#[cfg(test)]
mod tests {
    use super::*;

    fn contrast(a: Color32, b: Color32) -> f32 {
        let luminance = |c: Color32| {
            let channel = |v: u8| {
                let v = f32::from(v) / 255.;
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
        };
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn text_tokens_meet_wcag_aa() {
        for (text, background) in [
            (INK, Color32::WHITE),
            (INK, PAGE),
            (MUTED, Color32::WHITE),
            (MUTED, PAGE),
            (ACCENT, Color32::WHITE),
            (ACCENT, PAGE),
            (SUCCESS, Color32::WHITE),
            (WARNING, PAGE),
            (DANGER, DANGER_TINT),
            (Color32::WHITE, ACCENT),
            (Color32::WHITE, ACCENT_HOVER),
            (RAIL_TEXT, RAIL),
            (RAIL_TEXT, RAIL_HOVER),
        ] {
            let ratio = contrast(text, background);
            assert!(ratio >= 4.5, "{text:?} on {background:?} is {ratio:.2}:1");
        }
    }

    #[test]
    fn long_titles_stop_after_two_rows() {
        let ctx = egui::Context::default();
        apply(&ctx);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let long = "long words ".repeat(200);
            let galley = ctx.fonts_mut(|fonts| fonts.layout_job(title_job(&long, 300.)));
            assert_eq!(galley.rows.len(), TITLE_ROWS);
            assert!(galley.elided);
            let galley = ctx.fonts_mut(|fonts| fonts.layout_job(title_job("Short title", 300.)));
            assert_eq!(galley.rows.len(), 1);
            assert!(!galley.elided);
        });
    }

    #[cfg(windows)]
    #[test]
    fn emoji_have_a_font_in_every_text_style() {
        let ctx = egui::Context::default();
        apply(&ctx);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            for family in [FontFamily::Proportional, semibold_family()] {
                let font = egui::FontId::new(TEXT_BODY, family);
                for emoji in ['🧊', '🌍', '✨'] {
                    let found = ctx.fonts_mut(|fonts| fonts.has_glyph(&font, emoji));
                    assert!(found, "{emoji}");
                }
            }
        });
    }

    #[test]
    fn selected_widgets_can_use_their_own_text_color() {
        let ctx = egui::Context::default();
        apply(&ctx);
        let style = ctx.style();
        assert!(
            style.visuals.override_text_color.is_none(),
            "An override would put dark text on selected, accent-filled widgets"
        );
        assert_eq!(style.visuals.selection.stroke.color, Color32::WHITE);
        assert_eq!(style.visuals.selection.bg_fill, ACCENT);
    }
}
