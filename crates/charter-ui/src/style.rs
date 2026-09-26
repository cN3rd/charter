//! The editor's look: a dark theme of layered neutrals with one accent, Inter for text and
//! Phosphor for icons, and the colours that mark what things are on the notefield.

use std::sync::Arc;

use egui::{self, Color32, CornerRadius, FontFamily, FontId, Margin, RichText, Stroke, Ui, vec2};

pub use egui_phosphor::regular as icon;

pub const TEXT: Color32 = Color32::from_gray(214);
/// At least 4.5:1 against the panels, which egui's own weak text is not.
pub const WEAK_TEXT: Color32 = Color32::from_gray(146);

/// The menu bar and status bar, the frame around the work.
pub const CHROME: Color32 = Color32::from_gray(17);
/// The side panels, a step lighter than the chrome.
pub const PANEL: Color32 = Color32::from_gray(26);
/// The centre, where the notefield sits.
pub const CANVAS: Color32 = Color32::from_gray(21);
/// Dialogs and popups, lifted above the panels.
pub const RAISED: Color32 = Color32::from_gray(32);
/// Text fields, sunk below the panels.
pub const FIELD: Color32 = Color32::from_gray(14);
/// The toolbar, between the menu bar and the work.
pub const TOOLBAR: Color32 = Color32::from_gray(22);
/// The well around the notefield, between the canvas and the lanes.
pub const STAGE: Color32 = Color32::from_gray(17);
/// Sunken strips in the toolbar that group its controls.
pub const WELL: Color32 = Color32::from_gray(14);
/// Lines between regions.
pub const EDGE: Color32 = Color32::from_gray(40);

/// The one accent: the current selection, the primary action and focus.
pub const ACCENT: Color32 = Color32::from_rgb(110, 170, 255);
/// The accent as a fill behind white text, dark enough for 4.5:1.
pub const ACCENT_FILL: Color32 = Color32::from_rgb(40, 104, 208);
/// Warnings and refusals.
pub const WARN: Color32 = Color32::from_rgb(240, 180, 90);
/// The strip a notice sits on, tinted from the warning.
pub const NOTICE: Color32 = Color32::from_rgb(40, 34, 24);

pub const COLUMN_BG: Color32 = Color32::from_gray(24);
pub const LANES_BG: Color32 = Color32::from_gray(13);
pub const DIVIDER: Color32 = Color32::from_gray(58);
pub const BEAT_LINE: Color32 = Color32::from_gray(36);
/// Beat lines among sixteenth lines, a step brighter so each beat still reads.
pub const BEAT_LINE_STRONG: Color32 = Color32::from_gray(52);
pub const SIXTEENTH_LINE: Color32 = Color32::from_gray(26);
pub const MEASURE_LINE: Color32 = Color32::from_gray(92);
pub const TIMING_MARK: Color32 = Color32::from_rgb(230, 170, 90);
pub const NOW_LINE: Color32 = Color32::from_rgb(236, 240, 245);
pub const NOW_BAND: Color32 = Color32::from_rgba_unmultiplied_const(255, 255, 255, 10);
pub const SECTION: Color32 = Color32::from_rgb(110, 200, 130);
pub const BOOKMARK: Color32 = Color32::from_rgb(190, 140, 255);
pub const TAP: Color32 = ACCENT;
pub const HOLD: Color32 = Color32::from_rgb(60, 100, 170);
pub const ROLL: Color32 = Color32::from_rgb(230, 140, 70);
pub const MINE: Color32 = Color32::from_rgb(235, 90, 90);
pub const FAKE: Color32 = Color32::from_gray(110);
pub const OTHER: Color32 = Color32::from_rgb(170, 170, 120);
pub const SELECTED: Color32 = Color32::from_rgb(200, 224, 255);
pub const MARQUEE: Color32 = ACCENT;
/// The waveform, quieter than the notes it sits beside, on a strip of its own.
pub const WAVE: Color32 = Color32::from_rgb(92, 128, 176);
pub const WAVE_BG: Color32 = Color32::from_gray(16);
pub const MARQUEE_FILL: Color32 = Color32::from_rgba_unmultiplied_const(110, 170, 255, 28);

/// The family for titles and emphasis.
fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

/// The family icons are drawn in, which holds nothing but them.
fn icons() -> FontFamily {
    FontFamily::Name("phosphor".into())
}

/// `family` in `ui`, or the ordinary family on the first frame, before the fonts take.
fn bound(ui: &Ui, family: FontFamily) -> FontFamily {
    if ui.fonts(|fonts| fonts.definitions().families.contains_key(&family)) {
        family
    } else {
        FontFamily::Proportional
    }
}

/// The family for titles in `ui`.
pub fn titles(ui: &Ui) -> FontFamily {
    bound(ui, semibold())
}

/// `symbol`, one of [`icon`], drawn as the icon it is.
pub fn glyph(ui: &Ui, symbol: &str) -> RichText {
    RichText::new(symbol).family(bound(ui, icons()))
}

/// Inter for every word, `SemiBold` for titles, and Phosphor in a family of its own for icons,
/// which are always asked for by name: Phosphor draws letters as icons, and Inter has glyphs of
/// its own where Phosphor keeps its icons. `fallbacks`, such as the system's CJK fonts, draw
/// what Inter and egui's own fonts cannot, in words, titles and monospace alike.
fn fonts(fallbacks: &[(String, Arc<egui::FontData>)]) -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    let inter = |bytes: &'static [u8]| Arc::new(egui::FontData::from_static(bytes));
    fonts.font_data.insert(
        "inter".to_owned(),
        inter(include_bytes!("../assets/fonts/Inter-Regular.ttf")),
    );
    fonts.font_data.insert(
        "inter-semibold".to_owned(),
        inter(include_bytes!("../assets/fonts/Inter-SemiBold.ttf")),
    );
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
    proportional.retain(|font| font != "phosphor");
    proportional.insert(0, "inter".to_owned());
    for (name, data) in fallbacks {
        fonts.font_data.insert(name.clone(), data.clone());
    }
    let names = fallbacks.iter().map(|(name, _)| name.clone());
    let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
    proportional.extend(names.clone());
    let mut titles = vec!["inter-semibold".to_owned()];
    titles.extend(proportional.iter().skip(1).cloned());
    fonts.families.insert(semibold(), titles);
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .extend(names);
    fonts
}

/// Dark whatever the system prefers, since charting means long sessions against a game's dark
/// playfield, and with text bright enough to read at small sizes. Ctrl+Plus and Ctrl+Minus zoom
/// the notefield, so egui's own use of them to scale the whole window is off. Runs until it has
/// taken. `fallbacks` are fonts for what the bundled ones cannot draw.
pub fn apply(ctx: &egui::Context, fallbacks: &[(String, Arc<egui::FontData>)]) {
    if ctx.global_style().visuals.weak_text_color == Some(WEAK_TEXT) {
        return;
    }
    ctx.options_mut(|options| options.zoom_with_keyboard = false);
    ctx.set_theme(egui::Theme::Dark);
    // The fonts take from the next frame, and the look names them, so it waits until they have.
    let taken = ctx.fonts(|fonts| fonts.definitions().families.contains_key(&semibold()));
    if taken {
        ctx.style_mut_of(egui::Theme::Dark, look);
    } else {
        ctx.set_fonts(fonts(fallbacks));
        ctx.request_repaint();
    }
}

/// The dark theme's spacing, shapes, surfaces and states.
fn look(style: &mut egui::Style) {
    use egui::TextStyle;
    style.text_styles = [
        (TextStyle::Small, FontId::proportional(11.0)),
        (TextStyle::Body, FontId::proportional(13.0)),
        (TextStyle::Button, FontId::proportional(13.0)),
        (TextStyle::Heading, FontId::new(17.0, semibold())),
        (TextStyle::Monospace, FontId::monospace(12.5)),
    ]
    .into();
    let spacing = &mut style.spacing;
    spacing.item_spacing = vec2(8.0, 6.0);
    spacing.button_padding = vec2(10.0, 4.0);
    spacing.interact_size.y = 24.0;
    spacing.menu_margin = Margin::same(6);
    spacing.window_margin = Margin::same(16);

    let visuals = &mut style.visuals;
    visuals.weak_text_color = Some(WEAK_TEXT);
    visuals.panel_fill = PANEL;
    visuals.window_fill = RAISED;
    visuals.window_stroke = Stroke::new(1.0, EDGE);
    visuals.window_corner_radius = CornerRadius::same(10);
    visuals.menu_corner_radius = CornerRadius::same(8);
    let shadow = egui::Shadow {
        offset: [0, 10],
        blur: 28,
        spread: 0,
        color: Color32::from_black_alpha(120),
    };
    visuals.window_shadow = shadow;
    visuals.popup_shadow = egui::Shadow {
        offset: [0, 6],
        blur: 16,
        ..shadow
    };
    visuals.extreme_bg_color = FIELD;
    visuals.faint_bg_color = Color32::from_gray(30);
    visuals.selection.bg_fill = ACCENT_FILL;
    visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.hyperlink_color = ACCENT;
    visuals.text_cursor.stroke = Stroke::new(2.0, ACCENT);

    let radius = CornerRadius::same(5);
    let widgets = &mut visuals.widgets;
    widgets.noninteractive.fg_stroke.color = TEXT;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, EDGE);
    for (state, fill, text) in [
        (&mut widgets.inactive, 40, 214),
        (&mut widgets.hovered, 50, 240),
        (&mut widgets.active, 60, 250),
        (&mut widgets.open, 46, 240),
    ] {
        state.bg_fill = Color32::from_gray(fill);
        state.weak_bg_fill = Color32::from_gray(fill);
        state.bg_stroke = Stroke::NONE;
        state.fg_stroke.color = Color32::from_gray(text);
        state.corner_radius = radius;
        state.expansion = 0.0;
    }
    widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_gray(70));
    widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
}

/// The frame of the menu bar and status bar.
pub fn chrome() -> egui::Frame {
    egui::Frame::new()
        .fill(CHROME)
        .inner_margin(Margin::symmetric(10, 5))
}

/// The frame of the toolbar under the menu bar.
pub fn toolbar() -> egui::Frame {
    egui::Frame::new()
        .fill(TOOLBAR)
        .inner_margin(Margin::symmetric(10, 6))
        .stroke(Stroke::new(1.0, EDGE))
}

/// The frame of a side panel.
pub fn panel() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL)
        .inner_margin(Margin::symmetric(14, 8))
}

/// The frame of the centre.
pub fn canvas() -> egui::Frame {
    egui::Frame::new()
        .fill(CANVAS)
        .inner_margin(Margin::symmetric(16, 10))
}

/// The main action of a screen or dialog, in the accent.
pub fn primary(text: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(text).color(Color32::WHITE)).fill(ACCENT_FILL)
}

/// Names `response`, a button, `name` for screen readers and tests.
fn named(ui: &Ui, response: egui::Response, name: &str) -> egui::Response {
    let (name, enabled) = (name.to_owned(), ui.is_enabled());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &name));
    response
}

/// A button showing `symbol` before `text`, named `text` alone.
pub fn icon_button(ui: &mut Ui, symbol: &str, text: &str, primary_look: bool) -> egui::Response {
    let button = if primary_look {
        let white = |text: RichText| text.color(Color32::WHITE);
        egui::Button::new((white(glyph(ui, symbol)), white(RichText::new(text)))).fill(ACCENT_FILL)
    } else {
        egui::Button::new((glyph(ui, symbol), RichText::new(text)))
    };
    let response = ui.add(button);
    named(ui, response, text)
}

/// An icon that shows its button only on hover, named `name` and explained by `tip`.
pub fn quiet_icon(ui: &mut Ui, symbol: &str, name: &str, tip: &str) -> egui::Response {
    let button = egui::Button::new(glyph(ui, symbol).color(WEAK_TEXT)).frame_when_inactive(false);
    let response = ui.add(button).on_hover_text(tip);
    named(ui, response, name)
}

/// An icon button for the toolbar, lit in the accent while `on`, named `name` and explained by
/// `tip`.
pub fn tool(ui: &mut Ui, symbol: &str, name: &str, tip: &str, on: bool) -> egui::Response {
    let symbol = glyph(ui, symbol).size(16.0);
    let button = if on {
        egui::Button::new(symbol.color(Color32::WHITE)).fill(ACCENT_FILL)
    } else {
        egui::Button::new(symbol.color(TEXT)).frame_when_inactive(false)
    };
    let response = ui.add(button.min_size(vec2(30.0, 28.0))).on_hover_text(tip);
    named(ui, response, name)
}

/// An on and off switch, in the accent while on, named `name` for screen readers and tests.
pub fn switch(ui: &mut Ui, on: &mut bool, name: &str) -> egui::Response {
    let size = vec2(36.0, 20.0);
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let (state, name) = (*on, name.to_owned());
    let enabled = ui.is_enabled();
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, state, &name)
    });
    if ui.is_rect_visible(rect) {
        let lit = ui.ctx().animate_bool_responsive(response.id, *on);
        let track = if *on {
            ACCENT_FILL
        } else {
            Color32::from_gray(58)
        };
        let track = if response.hovered() {
            track.gamma_multiply(1.15)
        } else {
            track
        };
        let radius = rect.height() / 2.0;
        ui.painter().rect_filled(rect, radius, track);
        let x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), lit);
        let knob = egui::pos2(x, rect.center().y);
        ui.painter()
            .circle_filled(knob, radius - 3.0, Color32::from_gray(245));
        if response.has_focus() {
            let ring = Stroke::new(2.0, ACCENT);
            ui.painter().rect_stroke(
                rect.expand(2.0),
                radius + 2.0,
                ring,
                egui::StrokeKind::Outside,
            );
        }
    }
    response
}

/// A group of toolbar controls on a sunken strip, like a section of a hardware panel.
pub fn chip<R>(ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(WELL)
        .corner_radius(7)
        .inner_margin(Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.horizontal(body).inner
        })
        .inner
}

/// A collapsible panel section, open to start with, under a title in small capitals.
pub fn section(ui: &mut Ui, text: &str, body: impl FnOnce(&mut Ui)) {
    ui.add_space(6.0);
    let id = ui.make_persistent_id(("section", text));
    let title = title(ui, text);
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true)
        .show_header(ui, |ui| ui.label(title))
        .body(|ui| {
            ui.add_space(2.0);
            body(ui);
            ui.add_space(6.0);
        });
    ui.add(egui::Separator::default().spacing(2.0));
}

/// The well the notefield sits in, the darkest surface, so the chart reads as the stage.
pub fn stage() -> egui::Frame {
    egui::Frame::new()
        .fill(STAGE)
        .stroke(Stroke::new(1.0, EDGE))
        .corner_radius(10)
        .inner_margin(Margin::same(8))
}

/// A section title in small capitals.
pub fn title(ui: &Ui, text: &str) -> RichText {
    RichText::new(text.to_uppercase())
        .family(titles(ui))
        .size(10.5)
        .color(WEAK_TEXT)
        .extra_letter_spacing(0.9)
}

/// Starts a panel section, with more room above its title than below.
pub fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(14.0);
    let text = title(ui, text);
    ui.label(text);
    ui.add_space(4.0);
}

/// Wide enough for every label in the side panels, so their values line up.
const LABEL_W: f32 = 72.0;

/// Labels and values, or `empty` when there are none.
pub fn fields(ui: &mut Ui, id: &str, rows: &[(&str, String)], empty: &str) {
    if rows.is_empty() {
        ui.weak(empty);
        return;
    }
    table(ui, id, |ui| {
        for (name, value) in rows {
            ui.weak(*name);
            ui.label(value);
            ui.end_row();
        }
    });
}

/// Two columns whose second starts at the same place in every section.
pub fn table(ui: &mut Ui, id: &str, rows: impl FnOnce(&mut Ui)) {
    egui::Grid::new(id)
        .num_columns(2)
        .min_col_width(LABEL_W)
        .spacing([12.0, 6.0])
        .show(ui, rows);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_fonts_come_after_inter_in_text_titles_and_monospace() {
        let cjk = ("cjk".to_owned(), Arc::new(egui::FontData::from_static(&[])));
        let fonts = fonts(std::slice::from_ref(&cjk));
        assert!(fonts.font_data.contains_key("cjk"));
        for family in [FontFamily::Proportional, semibold(), FontFamily::Monospace] {
            let names = &fonts.families[&family];
            assert_eq!(names.last().map(String::as_str), Some("cjk"), "{family:?}");
        }
        let text = &fonts.families[&FontFamily::Proportional];
        assert_eq!(text.first().map(String::as_str), Some("inter"));
        assert!(!fonts.families[&icons()].contains(&"cjk".to_owned()));
    }
}
