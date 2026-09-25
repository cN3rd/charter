//! The editor's look: a dark theme, and the colours and type that mark what things are.

use egui::{self, Color32, RichText, Ui};

pub const TEXT: Color32 = Color32::from_gray(200);
/// At least 4.5:1 against the panels, which egui's own weak text is not.
pub const WEAK_TEXT: Color32 = Color32::from_gray(140);

pub const COLUMN_BG: Color32 = Color32::from_gray(22);
pub const LANES_BG: Color32 = Color32::from_gray(12);
pub const DIVIDER: Color32 = Color32::from_gray(60);
pub const BEAT_LINE: Color32 = Color32::from_gray(38);
pub const MEASURE_LINE: Color32 = Color32::from_gray(95);
pub const TIMING_MARK: Color32 = Color32::from_rgb(230, 170, 90);
pub const NOW_LINE: Color32 = Color32::from_rgb(236, 240, 245);
pub const NOW_BAND: Color32 = Color32::from_rgba_unmultiplied_const(255, 255, 255, 10);
pub const SECTION: Color32 = Color32::from_rgb(110, 200, 130);
pub const BOOKMARK: Color32 = Color32::from_rgb(190, 140, 255);
pub const TAP: Color32 = Color32::from_rgb(110, 170, 255);
pub const HOLD: Color32 = Color32::from_rgb(60, 100, 170);
pub const ROLL: Color32 = Color32::from_rgb(230, 140, 70);
pub const MINE: Color32 = Color32::from_rgb(235, 90, 90);
pub const FAKE: Color32 = Color32::from_gray(110);
pub const OTHER: Color32 = Color32::from_rgb(170, 170, 120);

/// Dark whatever the system prefers, since charting means long sessions against a game's dark
/// playfield, and with text bright enough to read at small sizes. Ctrl+Plus and Ctrl+Minus zoom
/// the notefield, so egui's own use of them to scale the whole window is off. Runs until it has
/// taken.
pub fn apply(ctx: &egui::Context) {
    if ctx.global_style().visuals.weak_text_color == Some(WEAK_TEXT) {
        return;
    }
    ctx.options_mut(|options| options.zoom_with_keyboard = false);
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        style.visuals.widgets.noninteractive.fg_stroke.color = TEXT;
        style.visuals.weak_text_color = Some(WEAK_TEXT);
    });
}

/// A section title in small capitals.
pub fn title(text: &str) -> RichText {
    RichText::new(text.to_uppercase())
        .size(11.0)
        .strong()
        .extra_letter_spacing(0.8)
}

/// Starts a panel section, with more room above its title than below.
pub fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(12.0);
    ui.label(title(text));
    ui.add_space(2.0);
}

/// Wide enough for every label in the side panels, so their values line up.
const LABEL_W: f32 = 64.0;

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
        .spacing([10.0, 5.0])
        .show(ui, rows);
}
