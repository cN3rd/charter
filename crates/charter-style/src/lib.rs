//! charter's look, shared by every window it draws: a dark theme of layered neutrals with one
//! accent, Inter for text and Phosphor for icons, the colours that mark what things are on the
//! notefield and the timing window, and the widgets drawn in that look.

use std::sync::Arc;

use egui::{self, Color32, CornerRadius, FontFamily, FontId, Margin, RichText, Stroke, Ui, vec2};

#[expect(
    clippy::pub_use,
    clippy::useless_attribute,
    reason = "Phosphor glyph names are part of the shared style's icon vocabulary"
)]
pub use egui_phosphor::regular as icon;

/// Ordinary text against the dark surfaces.
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
/// The edge of a switch or checkbox that is off, at least 3:1 against the panels behind it: with
/// no fill or text of its own, that edge is all there is to identify the control by.
pub const CONTROL_OFF: Color32 = Color32::from_gray(110);

/// The one accent: the current selection, the primary action and focus.
pub const ACCENT: Color32 = Color32::from_rgb(110, 170, 255);
/// The accent as a fill behind [`ON_ACCENT`], dark enough for 4.5:1.
pub const ACCENT_FILL: Color32 = Color32::from_rgb(40, 104, 208);
/// Text and icons drawn on [`ACCENT_FILL`], the one pairing that fill is chosen for.
pub const ON_ACCENT: Color32 = Color32::WHITE;
/// A row of a list under the pointer, lighter than the panel without reading as a border.
pub const ROW_HOVER: Color32 = Color32::from_gray(40);
/// Warnings and refusals in words: notices, failed reads, locked layers.
pub const WARN: Color32 = Color32::from_rgb(240, 180, 90);
/// An edit the game refuses, flashed on the thing in its way. Far enough from [`TIMING_MARK`]
/// and [`ANCHOR`] to show when it pulses over one, which [`WARN`] is not.
pub const REFUSED: Color32 = Color32::from_rgb(229, 76, 74);
/// The strip a notice sits on, tinted from the warning.
pub const NOTICE: Color32 = Color32::from_rgb(40, 34, 24);

/// The notefield's surround behind its lanes.
pub const COLUMN_BG: Color32 = Color32::from_gray(24);
/// The darker interior of each note lane.
pub const LANES_BG: Color32 = Color32::from_gray(13);
/// Separators between adjacent note lanes.
pub const DIVIDER: Color32 = Color32::from_gray(58);
/// Ordinary beat guides across the notefield.
pub const BEAT_LINE: Color32 = Color32::from_gray(36);
/// Beat lines among sixteenth lines, a step brighter so each beat still reads.
pub const BEAT_LINE_STRONG: Color32 = Color32::from_gray(52);
/// Guides between beats at sixteenth-note spacing.
pub const SIXTEENTH_LINE: Color32 = Color32::from_gray(26);
/// Bar boundaries on the notefield.
pub const MEASURE_LINE: Color32 = Color32::from_gray(92);
/// Timing changes and anchor positions.
pub const TIMING_MARK: Color32 = Color32::from_rgb(230, 170, 90);
/// The cursor's bright line.
pub const NOW_LINE: Color32 = Color32::from_rgb(236, 240, 245);
/// A faint band around the cursor.
pub const NOW_BAND: Color32 = Color32::from_rgba_unmultiplied_const(255, 255, 255, 10);
/// Section markers on the chart.
pub const SECTION: Color32 = Color32::from_rgb(110, 200, 130);
/// Bookmarks on the chart.
pub const BOOKMARK: Color32 = Color32::from_rgb(190, 140, 255);
/// Tap notes.
pub const TAP: Color32 = ACCENT;
/// Hold notes.
pub const HOLD: Color32 = Color32::from_rgb(60, 100, 170);
/// Roll notes.
pub const ROLL: Color32 = Color32::from_rgb(230, 140, 70);
/// Mine notes.
pub const MINE: Color32 = Color32::from_rgb(235, 90, 90);
/// Fake notes.
pub const FAKE: Color32 = Color32::from_gray(110);
/// Notes without a dedicated colour.
pub const OTHER: Color32 = Color32::from_rgb(170, 170, 120);
/// Highlighted notes in a selection.
pub const SELECTED: Color32 = Color32::from_rgb(200, 224, 255);
/// The outline of a selection marquee, the same light blue as the notes it gathers rather than
/// the accent, which is also [`TAP`].
pub const MARQUEE: Color32 = SELECTED;
/// The waveform, quieter than the notes it sits beside, on a strip of its own.
pub const WAVE: Color32 = Color32::from_rgb(92, 128, 176);
/// The strip beneath a waveform.
pub const WAVE_BG: Color32 = Color32::from_gray(16);
/// The translucent interior of a selection marquee, from the same light blue as its outline.
pub const MARQUEE_FILL: Color32 = Color32::from_rgba_unmultiplied_const(200, 224, 255, 28);
/// The timing window: onsets, how far they miss the grid either way, and anchors as they stand,
/// focused and selected.
///
/// Early and late differ in lightness as well as hue, and the residual strip draws them in its
/// two halves, so they read without colour.
pub const ONSET: Color32 = Color32::from_rgb(170, 180, 196);
/// A timing estimate before the chart's grid line. Green rather than blue: blue is the
/// waveform these sit on, and orange is the anchors they sit among.
pub const RESIDUAL_EARLY: Color32 = Color32::from_rgb(110, 216, 137);
/// A timing estimate after the chart's grid line, opposite the early one in hue and darker.
pub const RESIDUAL_LATE: Color32 = Color32::from_rgb(215, 115, 208);
/// An ordinary timing anchor.
pub const ANCHOR: Color32 = TIMING_MARK;
/// An anchor under keyboard focus.
pub const ANCHOR_FOCUS: Color32 = Color32::from_rgb(255, 214, 140);
/// A selected timing anchor, in the same light blue as any other selected thing: the accent sits
/// too close to [`TIMING_WAVE`], the waveform an anchor is drawn over.
pub const ANCHOR_SELECTED: Color32 = SELECTED;
/// The area beyond the timing window's editable reach.
pub const REACH_DIM: Color32 = Color32::from_rgba_unmultiplied_const(0, 0, 0, 120);
/// The timing window's waveform, brighter than the notefield waveform because it stands alone.
pub const TIMING_WAVE: Color32 = Color32::from_rgb(118, 158, 210);

/// Palette colour legible against the dark panels and note lanes. Each keeps its hue, and the
/// lightness differs from one to the next so every pair stays apart to colour-blind eyes too.
#[must_use]
pub const fn layer_colour(colour: charter_core::LayerColour) -> Color32 {
    use charter_core::LayerColour;
    match colour {
        LayerColour::Red => Color32::from_rgb(219, 86, 93),
        LayerColour::Orange => Color32::from_rgb(213, 130, 59),
        LayerColour::Yellow => Color32::from_rgb(251, 221, 111),
        // Clear of SECTION, which marks a part of the chart rather than a layer.
        LayerColour::Green => Color32::from_rgb(156, 204, 94),
        LayerColour::Teal => Color32::from_rgb(108, 226, 211),
        // Away from the accent, which marks selection and focus rather than a layer.
        LayerColour::Blue => Color32::from_rgb(28, 181, 236),
        // Clear of BOOKMARK, which marks a place in the chart rather than a layer.
        LayerColour::Purple => Color32::from_rgb(127, 129, 224),
        LayerColour::Pink => Color32::from_rgb(232, 137, 192),
    }
}

/// The family for titles and emphasis.
fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

/// The family icons are drawn in, which holds nothing but them.
fn icons() -> FontFamily {
    FontFamily::Name("phosphor".into())
}

/// Scroll bars in `ui` beside what they scroll, not floating over its right edge, where they
/// would hide the end of a row and take the clicks and drags meant for it.
pub fn solid_scroll_bars(ui: &mut Ui) {
    ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
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
#[must_use]
pub fn titles(ui: &Ui) -> FontFamily {
    bound(ui, semibold())
}

/// `symbol`, one of [`icon`], drawn as the icon it is.
#[must_use]
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
        fonts.font_data.insert(name.clone(), Arc::clone(data));
    }
    let names = fallbacks.iter().map(|(name, _)| name.clone());
    let text_families = fonts.families.entry(FontFamily::Proportional).or_default();
    text_families.extend(names.clone());
    let mut titles = vec!["inter-semibold".to_owned()];
    titles.extend(text_families.iter().skip(1).cloned());
    fonts.families.insert(semibold(), titles);
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .extend(names);
    fonts
}

/// Dark whatever the system prefers, with text bright enough to read at small sizes.
///
/// Charting means long sessions against a game's dark playfield. Ctrl+Plus and Ctrl+Minus zoom
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
    visuals.selection.stroke = Stroke::new(1.0, ON_ACCENT);
    visuals.hyperlink_color = ACCENT;
    visuals.text_cursor.stroke = Stroke::new(2.0, ACCENT);

    let radius = CornerRadius::same(5);
    let widgets = &mut visuals.widgets;
    widgets.noninteractive.fg_stroke.color = TEXT;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, EDGE);
    for (state, fill, foreground) in [
        (&mut widgets.inactive, 40, 214),
        (&mut widgets.hovered, 50, 240),
        (&mut widgets.active, 60, 250),
        (&mut widgets.open, 46, 240),
    ] {
        state.bg_fill = Color32::from_gray(fill);
        state.weak_bg_fill = Color32::from_gray(fill);
        state.bg_stroke = Stroke::NONE;
        state.fg_stroke.color = Color32::from_gray(foreground);
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
    egui::Button::new(RichText::new(text).color(ON_ACCENT)).fill(ACCENT_FILL)
}

/// Names `response`, a button, `name` for screen readers and tests, reporting `on` as the state
/// it holds. `None` is a button that does something rather than holding a state, which a screen
/// reader then announces without one.
fn named(ui: &Ui, response: egui::Response, name: &str, on: Option<bool>) -> egui::Response {
    let enabled = ui.is_enabled();
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, name);
        info.selected = on;
        info
    });
    response
}

/// A button showing `symbol` before `text`, named `text` alone.
pub fn icon_button(ui: &mut Ui, symbol: &str, text: &str, primary_look: bool) -> egui::Response {
    let button = if primary_look {
        let lit = |label: RichText| label.color(ON_ACCENT);
        egui::Button::new((lit(glyph(ui, symbol)), lit(RichText::new(text)))).fill(ACCENT_FILL)
    } else {
        egui::Button::new((glyph(ui, symbol), RichText::new(text)))
    };
    let response = ui.add(button);
    named(ui, response, text, None)
}

/// An icon that shows its button only on hover, named `name` and explained by `tip`.
pub fn quiet_icon(ui: &mut Ui, symbol: &str, name: &str, tip: &str) -> egui::Response {
    let button = egui::Button::new(glyph(ui, symbol).color(WEAK_TEXT)).frame_when_inactive(false);
    let response = ui.add(button).on_hover_text(tip);
    named(ui, response, name, None)
}

/// An icon button for the toolbar, named `name` and explained by `tip`.
///
/// `on` is the state a toggle holds, lit in the accent while it is `Some(true)`, and `None` for a
/// button that does something instead: the state reaches a screen reader as well as the eye, since
/// the accent alone says nothing to one.
pub fn tool(ui: &mut Ui, symbol: &str, name: &str, tip: &str, on: Option<bool>) -> egui::Response {
    let symbol = glyph(ui, symbol).size(16.0);
    let button = if on == Some(true) {
        egui::Button::new(symbol.color(ON_ACCENT)).fill(ACCENT_FILL)
    } else {
        egui::Button::new(symbol.color(TEXT)).frame_when_inactive(false)
    };
    let response = ui.add(button.min_size(vec2(30.0, 28.0))).on_hover_text(tip);
    named(ui, response, name, on)
}

/// An on and off switch, in the accent while on, named `name` for screen readers and tests.
pub fn switch(ui: &mut Ui, on: &mut bool, name: &str) -> egui::Response {
    let size = vec2(36.0, 20.0);
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let state = *on;
    let enabled = ui.is_enabled();
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, state, name)
    });
    if ui.is_rect_visible(rect) {
        let lit = ui.ctx().animate_bool_responsive(response.id, *on);
        // Off is a dark well like an empty checkbox, outlined at 3:1 against the panels so it can
        // be found, and so unlit beside the accent fill of a switch that is on.
        let (fill, edge) = if *on {
            (ACCENT_FILL, Color32::TRANSPARENT)
        } else {
            (FIELD, CONTROL_OFF)
        };
        let (fill, edge) = if response.hovered() {
            (fill.gamma_multiply(1.15), edge.gamma_multiply(1.15))
        } else {
            (fill, edge)
        };
        let radius = rect.height() / 2.0;
        ui.painter().rect(
            rect,
            radius,
            fill,
            Stroke::new(1.0, edge),
            egui::StrokeKind::Inside,
        );
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

/// A moment of the audio as minutes, seconds and milliseconds, such as `01:23.456`.
#[must_use]
pub fn clock(seconds: f64) -> String {
    if !seconds.is_finite() {
        return "00:00.000".to_owned();
    }
    let sign = if seconds < 0.0 { "-" } else { "" };
    let millis = milliseconds(seconds);
    format!(
        "{sign}{:02}:{:02}.{:03}",
        millis.div_euclid(60_000),
        millis.div_euclid(1000).rem_euclid(60),
        millis.rem_euclid(1000)
    )
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "rounded positive display time saturates on overflow"
)]
/// Absolute `seconds` rounded to whole milliseconds, saturating if out of range.
fn milliseconds(seconds: f64) -> u64 {
    (seconds.abs() * 1000.0).round() as u64
}

/// `value` to `places` decimal places, without trailing zeroes.
#[must_use]
pub fn trimmed_decimal(value: f64, places: usize) -> String {
    let text = format!("{value:.places$}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// A one-line text field showing `current`, kept in `draft` while it has keyboard focus.
///
/// Returns the trimmed text once the field is entered or left, unless Escape cancelled the edit
/// or the text still matches `current`.
#[must_use]
pub fn text_field(
    ui: &mut Ui,
    caption: egui::Id,
    current: &str,
    draft: &mut Option<String>,
    take_focus: bool,
) -> Option<String> {
    let mut text = draft.take().unwrap_or_else(|| current.to_owned());
    let response = ui
        .add(egui::TextEdit::singleline(&mut text).desired_width(84.0))
        .labelled_by(caption);
    if take_focus {
        response.request_focus();
    }
    let left = response.lost_focus()
        && !ui.input(|input| input.key_pressed(egui::Key::Escape))
        && text.trim() != current;
    if response.has_focus() {
        *draft = Some(text);
        None
    } else if left {
        Some(text.trim().to_owned())
    } else {
        None
    }
}

/// A checkbox before `text`, both one target: a sunken square with an edge when off, filled in
/// the accent with a white tick when on.
pub fn check(ui: &mut Ui, on: &mut bool, text: &str) -> egui::Response {
    let square_w = 14.0;
    let galley = egui::WidgetText::from(text).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Body,
    );
    let gap = 7.0;
    let size = vec2(
        square_w + gap + galley.size().x,
        ui.spacing().interact_size.y.max(galley.size().y),
    );
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let (state, enabled) = (*on, ui.is_enabled());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, state, text)
    });
    if ui.is_rect_visible(rect) {
        let square = egui::Rect::from_min_size(
            egui::pos2(rect.left(), rect.center().y - square_w / 2.0),
            vec2(square_w, square_w),
        );
        let hovered = response.hovered() && enabled;
        let painter = ui.painter();
        if *on {
            let fill = if hovered {
                ACCENT_FILL.gamma_multiply(1.15)
            } else {
                ACCENT_FILL
            };
            painter.rect_filled(square, 3.0, fill);
            #[expect(
                clippy::arithmetic_side_effects,
                reason = "egui position and vector arithmetic is floating-point"
            )]
            let at = |x: f32, y: f32| square.min + vec2(x, y) * square_w;
            painter.line(
                vec![at(0.24, 0.52), at(0.43, 0.7), at(0.76, 0.32)],
                Stroke::new(1.8, ON_ACCENT),
            );
        } else {
            // 3:1 against the panels behind it, since the edge is what identifies an empty box.
            let edge = if hovered {
                CONTROL_OFF.gamma_multiply(1.15)
            } else {
                CONTROL_OFF
            };
            painter.rect_filled(square, 3.0, FIELD);
            painter.rect_stroke(
                square,
                3.0,
                Stroke::new(1.0, edge),
                egui::StrokeKind::Inside,
            );
        }
        if response.has_focus() {
            let ring = Stroke::new(2.0, ACCENT);
            painter.rect_stroke(square.expand(2.0), 5.0, ring, egui::StrokeKind::Outside);
        }
        let color = if enabled { TEXT } else { WEAK_TEXT };
        let text_at = egui::pos2(
            square.right() + gap,
            rect.center().y - galley.size().y / 2.0,
        );
        painter.galley(text_at, galley, color);
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

/// A section whose header also holds controls at the trailing edge.
pub fn section_with_actions(ui: &mut Ui, text: &str, mut contents: impl FnMut(&mut Ui, bool)) {
    ui.add_space(6.0);
    let id = ui.make_persistent_id(("section", text));
    let title = title(ui, text);
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true)
        .show_header(ui, |ui| {
            ui.label(title);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                contents(ui, true);
            });
        })
        .body(|ui| {
            ui.add_space(2.0);
            contents(ui, false);
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
#[must_use]
pub fn title(ui: &Ui, text: &str) -> RichText {
    RichText::new(text.to_uppercase())
        .family(titles(ui))
        .size(10.5)
        .color(WEAK_TEXT)
        .extra_letter_spacing(0.9)
}

/// The title of a dialog or a settings page, above the line that explains it.
#[must_use]
pub fn dialog_title(ui: &Ui, text: &str) -> RichText {
    RichText::new(text).family(titles(ui)).size(15.0)
}

/// Starts a panel section, with more room above its title than below.
pub fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(14.0);
    let text = title(ui, text);
    ui.label(text);
    ui.add_space(4.0);
}

/// Wide enough for every label in the side panels, so their values line up.
const LABEL_W: f32 = 84.0;

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

    /// `colour`'s red, green and blue in linear light.
    fn linear(colour: Color32) -> [f64; 3] {
        let channel = |value: u8| {
            let value = f64::from(value) / 255.0;
            if value <= 0.040_45 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        [
            channel(colour.r()),
            channel(colour.g()),
            channel(colour.b()),
        ]
    }

    /// WCAG relative luminance of `colour`, drawn opaque.
    fn luminance(colour: Color32) -> f64 {
        let [red, green, blue] = linear(colour);
        0.2126 * red + 0.7152 * green + 0.0722 * blue
    }

    /// The contrast ratio between `a` and `b`, from 1.0 to 21.0.
    fn contrast(a: Color32, b: Color32) -> f64 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn an_off_control_stays_three_to_one_against_every_surface_it_sits_on() {
        // WCAG 1.4.11: a switch or checkbox that is off has no fill or text, so its edge is the
        // only thing that says a control is there.
        for surface in [PANEL, RAISED, CHROME, TOOLBAR, CANVAS] {
            let ratio = contrast(CONTROL_OFF, surface);
            assert!(ratio >= 3.0, "{CONTROL_OFF:?} on {surface:?} is {ratio:.2}");
        }
    }

    /// The track a switch draws while `on`, its fill and outline in the one shape.
    fn track(on: bool) -> egui::epaint::RectShape {
        let mut state = on;
        let mut output = egui::Context::default().run_ui(egui::RawInput::default(), |ui| {
            switch(ui, &mut state, "Switch");
        });
        output.textures_delta.clear();
        output
            .shapes
            .into_iter()
            .find_map(|clipped| {
                if let egui::Shape::Rect(rect) = clipped.shape
                    && (rect.rect.height() - 20.0).abs() < 0.01
                {
                    Some(rect)
                } else {
                    None
                }
            })
            .unwrap()
    }

    #[test]
    fn an_off_switch_is_an_outlined_well_dimmer_than_a_lit_one() {
        let (off, on) = (track(false), track(true));
        // Off has to look unlit beside on by its brightness, not by its hue alone.
        let apart = contrast(on.fill, off.fill);
        assert!(
            apart >= 2.0 && luminance(off.fill) < luminance(on.fill),
            "the on and off tracks are {apart:.2}:1 apart"
        );
        // The outline is what finds it on a panel, held at 3:1 by the test above.
        assert_eq!(off.stroke.color, CONTROL_OFF);
    }

    #[test]
    fn text_stays_readable_on_every_surface_it_is_drawn_on() {
        for surface in [CHROME, PANEL, CANVAS, RAISED, FIELD, TOOLBAR, STAGE, WELL] {
            for (name, text) in [("TEXT", TEXT), ("WEAK_TEXT", WEAK_TEXT)] {
                let ratio = contrast(text, surface);
                assert!(ratio >= 4.5, "{name} on {surface:?} is {ratio:.2}");
            }
        }
        let on_fill = contrast(ON_ACCENT, ACCENT_FILL);
        assert!(on_fill >= 4.5, "ON_ACCENT on ACCENT_FILL is {on_fill:.2}");
    }

    /// Oklab lightness, green–red and blue–yellow of the linear-light `[red, green, blue]`.
    fn oklab([red, green, blue]: [f64; 3]) -> [f64; 3] {
        let long = (0.412_221_47 * red + 0.536_332_54 * green + 0.051_445_993 * blue).cbrt();
        let medium = (0.211_903_5 * red + 0.680_699_55 * green + 0.107_396_96 * blue).cbrt();
        let short = (0.088_302_46 * red + 0.281_718_84 * green + 0.629_978_7 * blue).cbrt();
        [
            0.210_454_26 * long + 0.793_617_8 * medium - 0.004_072_047 * short,
            1.977_998_5 * long - 2.428_592_2 * medium + 0.450_593_7 * short,
            0.025_904_037 * long + 0.782_771_77 * medium - 0.808_675_77 * short,
        ]
    }

    /// OKLCH hue of `colour` in degrees.
    fn hue(colour: Color32) -> f64 {
        let [_, green_red, blue_yellow] = oklab(linear(colour));
        blue_yellow.atan2(green_red).to_degrees().rem_euclid(360.0)
    }

    /// Degrees between the hues of `a` and `b`, at most 180.
    fn apart(a: Color32, b: Color32) -> f64 {
        let gap = (hue(a) - hue(b)).abs();
        gap.min(360.0 - gap)
    }

    #[test]
    fn every_layer_colour_is_told_apart_from_what_shares_the_notefield_with_it() {
        // A layer's colour lands beside the markers and the taps, so inside 15 degrees of hue it
        // stops saying which layer a note is on. ACCENT covers TAP, which aliases it. The other
        // note kinds are left out: a mine's ring and a hold's or roll's body tell them apart, and
        // a layer's stripe sits on a lane-dark gutter beside the head it marks.
        for colour in charter_core::LayerColour::ALL {
            for (name, other) in [
                ("SECTION", SECTION),
                ("BOOKMARK", BOOKMARK),
                ("ACCENT", ACCENT),
            ] {
                let gap = apart(layer_colour(colour), other);
                assert!(gap >= 15.0, "{colour:?} is {gap:.1} degrees from {name}");
            }
        }
    }

    #[test]
    fn the_layer_palette_keeps_its_entries_apart_from_each_other() {
        for one in charter_core::LayerColour::ALL {
            for two in charter_core::LayerColour::ALL {
                if one == two {
                    continue;
                }
                let gap = apart(layer_colour(one), layer_colour(two));
                assert!(gap >= 15.0, "{one:?} is {gap:.1} degrees from {two:?}");
            }
        }
    }

    /// Colour vision as matrices on linear light, after Machado, Oliveira and Fernandes (2009)
    /// at full severity: typical vision, then the three kinds of colour blindness.
    const VISIONS: [(&str, [[f64; 3]; 3]); 4] = [
        (
            "typical",
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        ),
        (
            "protanopia",
            [
                [0.152_286, 1.052_583, -0.204_868],
                [0.114_503, 0.786_281, 0.099_216],
                [-0.003_882, -0.048_116, 1.051_998],
            ],
        ),
        (
            "deuteranopia",
            [
                [0.367_322, 0.860_646, -0.227_968],
                [0.280_085, 0.672_501, 0.047_413],
                [-0.011_82, 0.042_94, 0.968_881],
            ],
        ),
        (
            "tritanopia",
            [
                [1.255_528, -0.076_749, -0.178_779],
                [-0.078_411, 0.930_809, 0.147_602],
                [0.004_733, 0.691_367, 0.303_9],
            ],
        ),
    ];

    #[test]
    fn every_pair_of_layer_colours_stays_apart_to_colour_blind_eyes() {
        // Hue is what colour blindness takes, so the palette spends lightness to keep each pair
        // at least 6 apart in OKLab (×100), some six just-noticeable steps, whatever the vision.
        let colours = charter_core::LayerColour::ALL;
        for (vision, matrix) in VISIONS {
            let seen = |colour| {
                let light = linear(layer_colour(colour));
                oklab(matrix.map(|row| {
                    (row[0] * light[0] + row[1] * light[1] + row[2] * light[2]).clamp(0.0, 1.0)
                }))
            };
            for (index, one) in colours.iter().enumerate() {
                for two in &colours[index + 1..] {
                    let (a, b) = (seen(*one), seen(*two));
                    let gap = 100.0
                        * ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2))
                            .sqrt();
                    assert!(
                        gap >= 6.0,
                        "{one:?} and {two:?} are {gap:.1} apart to {vision}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_refused_anchor_pulses_to_a_colour_an_anchor_is_not_already() {
        // rows.rs lerps an anchor toward REFUSED, so the two must not already match.
        for (name, anchor) in [("ANCHOR", ANCHOR), ("ANCHOR_FOCUS", ANCHOR_FOCUS)] {
            let gap = apart(REFUSED, anchor);
            assert!(gap >= 15.0, "REFUSED is {gap:.1} degrees from {name}");
        }
    }

    #[test]
    fn the_timing_window_separates_its_markers_from_the_waveform_under_them() {
        // Either hue or lightness has to carry each pair; the residuals do it by hue and a
        // selected anchor by lightness.
        for (name, marker) in [
            ("RESIDUAL_EARLY", RESIDUAL_EARLY),
            ("RESIDUAL_LATE", RESIDUAL_LATE),
            ("ANCHOR_SELECTED", ANCHOR_SELECTED),
        ] {
            let split = apart(marker, TIMING_WAVE) >= 15.0
                || (luminance(marker) - luminance(TIMING_WAVE)).abs() > 0.2;
            assert!(split, "{name} does not separate from TIMING_WAVE");
        }
    }

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

    #[test]
    fn a_clock_rounds_across_minute_boundaries_and_keeps_negative_times() {
        assert_eq!(clock(59.9995), "01:00.000");
        assert_eq!(clock(-61.234), "-01:01.234");
        assert_eq!(clock(0.0004), "00:00.000");
    }

    #[test]
    fn a_clock_treats_non_finite_times_as_zero() {
        assert_eq!(clock(f64::NAN), "00:00.000");
        assert_eq!(clock(f64::INFINITY), "00:00.000");
        assert_eq!(clock(f64::NEG_INFINITY), "00:00.000");
    }

    #[test]
    fn a_trimmed_decimal_keeps_requested_precision_without_trailing_zeroes() {
        assert_eq!(trimmed_decimal(120.0, 3), "120");
        assert_eq!(trimmed_decimal(1.25, 3), "1.25");
        assert_eq!(trimmed_decimal(-0.001, 3), "-0.001");
    }

    #[test]
    fn empty_fields_show_the_placeholder_instead_of_a_value_grid() {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            fields(ui, "empty", &[], "No values");
        });
        output.textures_delta.clear();
        assert!(output.shapes.iter().any(|clipped| {
            matches!(&clipped.shape, egui::Shape::Text(text) if text.galley.text() == "No values")
        }));
    }

    #[test]
    fn palette_entries_have_distinct_visible_colours() {
        let colours: Vec<_> = charter_core::LayerColour::ALL
            .into_iter()
            .map(layer_colour)
            .collect();
        assert_eq!(colours.len(), 8);
        for colour in &colours {
            assert!(colour.r().max(colour.g()).max(colour.b()) >= 160);
        }
        for (index, colour) in colours.iter().enumerate() {
            assert!(!colours[index + 1..].contains(colour));
        }
    }
}

/// Proofs for Kani, run with `cargo kani -p charter-style`.
#[cfg(kani)]
mod proofs {
    #[kani::proof]
    fn every_short_millisecond_count_splits_into_minutes_seconds_and_millis() {
        let millis: u16 = kani::any();
        let minutes = millis / 60_000;
        let seconds = (millis / 1000) % 60;
        let remainder = millis % 1000;
        assert!(seconds < 60 && remainder < 1000);
        assert_eq!(
            u32::from(minutes) * 60_000 + u32::from(seconds) * 1000 + u32::from(remainder),
            u32::from(millis)
        );
    }
}
