//! A static mock of the editor window in the book's user interface draft. It draws a made-up
//! song for each of several rulesets, and switching between them is all it responds to.

mod rulesets;

use eframe::egui::{
    self, Align, Align2, Color32, FontId, Layout, Painter, Pos2, Rangef, Rect, RichText, Sense,
    Shape, Stroke, StrokeKind, Ui, Visuals, pos2, vec2,
};

use rulesets::Mock;

const MENUS: [(&str, &[&str]); 7] = [
    (
        "File",
        &[
            "New song...",
            "Open...",
            "Save",
            "Export...",
            "Preferences...",
        ],
    ),
    ("Edit", &["Undo", "Redo", "Cut", "Copy", "Paste"]),
    (
        "Select",
        &["All", "By snap", "By kind", "Before cursor", "After cursor"],
    ),
    ("Transform", &["Mirror", "Expand", "Compress"]),
    (
        "Timing",
        &["Adjust sync...", "Adjust tempo...", "Detect BPM..."],
    ),
    ("View", &["C-mod", "X-mod"]),
    ("Help", &["Commands"]),
];

/// A position in a mock chart, in beats from the start of measure 12.
// ponytail: beats evenly spaced at one tempo, so stops and tempo changes draw nothing. The real
// notefield resolves charter-core ticks through the timing map and the view mode.
#[derive(Clone, Copy)]
struct Beat(f32);

/// The cursor, which the now line shows.
const NOW: Beat = Beat(2.0);
const BEAT_PX: f32 = 56.0;

const RULER_W: f32 = 80.0;
const TIMING_W: f32 = 160.0;
const WAVE_W: f32 = 160.0;
/// The narrowest the timing column and waveform get when the window is too narrow for both.
const TIMING_MIN_W: f32 = 72.0;
const WAVE_MIN_W: f32 = 56.0;
const OVERVIEW_W: f32 = 28.0;
const HEADER_H: f32 = 26.0;
/// Wide enough for every label in the side panels, so their values line up.
const LABEL_W: f32 = 64.0;

const COLUMN_BG: Color32 = Color32::from_gray(22);
const LANES_BG: Color32 = Color32::from_gray(12);
const DIVIDER: Color32 = Color32::from_gray(60);
const BEAT_LINE: Color32 = Color32::from_gray(38);
const MEASURE_LINE: Color32 = Color32::from_gray(95);
const TEXT: Color32 = Color32::from_gray(200);
/// At least 4.5:1 against the panels, which egui's own weak text is not.
const WEAK_TEXT: Color32 = Color32::from_gray(140);
const KEY_CAP: Color32 = Color32::from_gray(34);
const KEY_EDGE: Color32 = Color32::from_gray(70);
const TIMING_MARK: Color32 = Color32::from_rgb(230, 170, 90);
const NOW_LINE: Color32 = Color32::from_rgb(236, 240, 245);
const NOW_BAND: Color32 = Color32::from_rgba_unmultiplied_const(255, 255, 255, 10);
const SECTION: Color32 = Color32::from_rgb(110, 200, 130);
const BOOKMARK: Color32 = Color32::from_rgb(190, 140, 255);
const WAVE: Color32 = Color32::from_rgb(62, 108, 96);
const DENSITY: Color32 = Color32::from_gray(110);

/// Markers, the same in every mock song.
const MARKERS: [(Beat, &str, Color32); 2] = [
    (Beat(8.0), "Chorus", SECTION),
    (Beat(11.0), "fill", BOOKMARK),
];

/// Which way later beats go on screen. A ruleset scrolls the way its game does.
#[derive(Clone, Copy)]
enum Direction {
    /// Later beats appear lower on screen.
    Down,
    /// Later beats appear higher on screen.
    Up,
}

impl Direction {
    /// 1 when later beats sit lower on screen, -1 when they sit higher.
    const fn sign(self) -> f32 {
        match self {
            Self::Down => 1.0,
            Self::Up => -1.0,
        }
    }

    /// The edge of `rows` the chart starts from.
    const fn start(self, rows: Rangef) -> f32 {
        match self {
            Self::Down => rows.min,
            Self::Up => rows.max,
        }
    }
}

/// Where beats are drawn in one frame.
#[derive(Clone, Copy)]
struct View {
    now_y: f32,
    direction: Direction,
}

impl View {
    /// Puts the now line near the edge the chart starts from, leaving the rest of `rows` for what
    /// comes next.
    fn new(rows: Rangef, direction: Direction) -> Self {
        let now_y = direction.start(rows) + direction.sign() * rows.span() * 0.2;
        Self { now_y, direction }
    }

    fn y(self, at: Beat) -> f32 {
        self.now_y + self.direction.sign() * (at.0 - NOW.0) * BEAT_PX
    }

    /// Anchors a label for the line at `y` on its later side, so it reads as starting there.
    fn after(self, x: f32, y: f32, horizontal: Align) -> (Pos2, Align2) {
        match self.direction {
            Direction::Down => (pos2(x, y + 2.0), Align2([horizontal, Align::Min])),
            Direction::Up => (pos2(x, y - 2.0), Align2([horizontal, Align::Max])),
        }
    }
}

/// A ruleset's column in one frame: where its lanes are and where each beat is.
struct Lanes {
    /// The lanes and any playfield beyond them.
    playfield: Rangef,
    /// The strip for a second view, when the ruleset has one.
    side: Option<Rangef>,
    /// The vertical part of the chart below the key header.
    rows: Rangef,
    /// The left edge of lane 0.
    left: f32,
    /// Width of each playable lane, in points.
    lane_w: f32,
    /// Number of playable lanes in this chart.
    count: u16,
    /// Mapping from mock beats to screen positions.
    view: View,
}

impl Lanes {
    /// The `width` lanes starting at `lane`.
    fn span(&self, lane: u16, width: u16) -> Rangef {
        let edge = |column: u16| self.left + f32::from(column) * self.lane_w;
        Rangef::new(edge(lane), edge(lane.saturating_add(width)))
    }

    /// Screen height of `at` in this playfield.
    fn y(&self, at: Beat) -> f32 {
        self.view.y(at)
    }
}

/// Opens the static egui window on the StepMania mock.
fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]),
        ..eframe::NativeOptions::default()
    };
    let mut open = &rulesets::STEPMANIA;
    eframe::run_ui_native("charter", options, move |ui, _frame| {
        window(ui, &mut open);
    })
}

fn window(ui: &mut Ui, open: &mut &'static Mock) {
    if ui.visuals().weak_text_color != Some(WEAK_TEXT) {
        style(ui.ctx());
    }
    egui::Panel::top("menu").show(ui, |ui| menu_bar(ui, open));
    let mock = *open;
    egui::Panel::bottom("status").show(ui, status_bar);
    egui::Panel::bottom("problems").show(ui, |ui| problems(ui, mock));
    egui::Panel::left("song").show(ui, |ui| song(ui, mock));
    egui::Panel::right("inspector").show(ui, |ui| inspector(ui, mock));
    egui::CentralPanel::default().show(ui, |ui| notefield(ui, mock));
}

/// Dark whatever the system prefers, since charting means long sessions against a game's dark
/// playfield, and with text bright enough to read at small sizes.
fn style(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        style.visuals.widgets.noninteractive.fg_stroke.color = TEXT;
        style.visuals.weak_text_color = Some(WEAK_TEXT);
    });
}

/// Shows placeholder menus and the ruleset switcher.
fn menu_bar(ui: &mut Ui, open: &mut &'static Mock) {
    egui::MenuBar::new().ui(ui, |ui| {
        for (title, items) in MENUS {
            ui.menu_button(title, |ui| {
                for item in items {
                    drop(ui.button(*item));
                }
            });
        }
        ui.separator();
        ui.weak("Mock");
        for mock in rulesets::ALL {
            if ui
                .selectable_label(open.game == mock.game, mock.game)
                .clicked()
            {
                *open = mock;
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            drop(ui.button("Commands  Ctrl+K"));
        });
    });
}

/// Shows the mock transport, cursor position and editor controls.
fn status_bar(ui: &mut Ui) {
    ui.horizontal(|ui| {
        play_button(ui);
        ui.monospace("12:2:0");
        ui.monospace("00:16.667");
        ui.separator();
        for control in ["Rate 100%", "Snap 16th", "Zoom 100%"] {
            drop(ui.button(control));
        }
        ui.separator();
        for (on, toggle) in [(true, "C-mod"), (false, "X-mod"), (false, "Metronome")] {
            drop(ui.selectable_label(on, toggle));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.strong("Edit"));
    });
}

/// The transport button, drawn rather than typed so it matches the notefield's shapes.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "egui positions are offset by vectors"
)]
fn play_button(ui: &mut Ui) {
    let (rect, response) = ui.allocate_exact_size(vec2(24.0, 20.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Play"));
    let look = ui.style().interact(&response);
    ui.painter()
        .rect_filled(rect, look.corner_radius, look.weak_bg_fill);
    let middle = rect.center();
    let triangle = vec![
        middle + vec2(-3.5, -5.0),
        middle + vec2(5.0, 0.0),
        middle + vec2(-3.5, 5.0),
    ];
    ui.painter().add(Shape::convex_polygon(
        triangle,
        look.fg_stroke.color,
        Stroke::NONE,
    ));
    drop(response.on_hover_text("Play"));
}

/// Shows the selected song's problem, or its empty state.
fn problems(ui: &mut Ui, mock: &Mock) {
    ui.horizontal(|ui| {
        ui.label(title("Problems"));
        if let Some(problem) = mock.problem {
            let warning = RichText::new(problem).color(ui.visuals().warn_fg_color);
            drop(ui.link(warning));
        } else {
            ui.weak("No problems");
        }
    });
}

/// Draws the song metadata, charts, settings and sample lists.
fn song(ui: &mut Ui, mock: &Mock) {
    heading(ui, "Song");
    fields(
        ui,
        "song",
        &[
            ("Title", mock.title),
            ("Artist", "Someone"),
            ("Audio", "song.ogg"),
            ("Preview", "45.0 s"),
        ],
        "",
    );
    heading(ui, "Charts");
    table(ui, "charts", |ui| {
        for &(name, lanes, open) in mock.charts {
            drop(ui.selectable_label(open, name));
            ui.weak(lanes);
            ui.end_row();
        }
    });
    ui.horizontal(|ui| {
        for action in ["New", "Copy", "Delete"] {
            drop(ui.small_button(action));
        }
    });
    heading(ui, "Chart settings");
    fields(ui, "settings", mock.settings, "No chart settings");
    heading(ui, "Samples");
    fields(ui, "samples", mock.samples, "No samples");
}

fn inspector(ui: &mut Ui, mock: &Mock) {
    heading(ui, "Inspector");
    ui.strong(mock.selected);
    fields(ui, "inspector", mock.inspector, "Nothing selected");
}

/// A section title in small capitals.
fn title(text: &str) -> RichText {
    RichText::new(text.to_uppercase())
        .size(11.0)
        .strong()
        .extra_letter_spacing(0.8)
}

/// Starts a panel section, with more room above its title than below.
fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(12.0);
    ui.label(title(text));
    ui.add_space(2.0);
}

/// Labels and values, or `empty` when there are none.
fn fields(ui: &mut Ui, id: &str, rows: &[(&str, &str)], empty: &str) {
    if rows.is_empty() {
        ui.weak(empty);
        return;
    }
    table(ui, id, |ui| {
        for (name, value) in rows {
            ui.weak(*name);
            ui.label(*value);
            ui.end_row();
        }
    });
}

/// Two columns whose second starts at the same place in every section.
fn table(ui: &mut Ui, id: &str, rows: impl FnOnce(&mut Ui)) {
    egui::Grid::new(id)
        .num_columns(2)
        .min_col_width(LABEL_W)
        .spacing([10.0, 5.0])
        .show(ui, rows);
}

/// Where each notefield column sits across the window.
struct Columns {
    ruler: Rangef,
    timing: Rangef,
    /// The column the ruleset draws.
    ruleset: Rangef,
    wave: Rangef,
    overview: Rangef,
}

impl Columns {
    /// Centres the columns in `across`. Short of room, the timing column and the waveform narrow
    /// first, and when even that is not enough the columns start at the left edge.
    fn laid_out(across: Rangef, ruleset: f32) -> Self {
        let fixed = RULER_W + ruleset + OVERVIEW_W;
        let give = (TIMING_W - TIMING_MIN_W) + (WAVE_W - WAVE_MIN_W);
        let room = (across.span() - fixed - TIMING_MIN_W - WAVE_MIN_W).clamp(0.0, give);
        let timing = TIMING_MIN_W + (TIMING_W - TIMING_MIN_W) * room / give;
        let wave = WAVE_MIN_W + (WAVE_W - WAVE_MIN_W) * room / give;
        let total = fixed + timing + wave;
        let mut left = (across.center() - total / 2.0).max(across.min);
        let mut next = |width: f32| {
            left += width;
            Rangef::new(left - width, left)
        };
        Self {
            ruler: next(RULER_W),
            timing: next(timing),
            ruleset: next(ruleset),
            wave: next(wave),
            overview: next(OVERVIEW_W),
        }
    }

    /// The columns sharing the now line's time axis. The overview shows the whole chart instead.
    fn timeline(&self) -> Rangef {
        Rangef::new(self.ruler.min, self.wave.max)
    }
}

fn notefield(ui: &mut Ui, mock: &Mock) {
    let open = mock
        .charts
        .iter()
        .find(|chart| chart.2)
        .map_or("", |chart| chart.0);
    let columns = Columns::laid_out(ui.available_rect_before_wrap().x_range(), mock.width());
    ui.horizontal(|ui| {
        ui.add_space((columns.ruler.min - ui.cursor().left()).max(0.0));
        ui.strong(open);
        ui.weak(mock.layout);
    });
    let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::hover());
    let area = response.rect;
    let rows = Rangef::new(area.top() + HEADER_H, area.bottom());
    let view = View::new(rows, mock.direction);
    let playfield = Rangef::new(
        columns.ruleset.min,
        columns.ruleset.min + mock.playfield_w(),
    );
    let lanes = Lanes {
        playfield,
        side: mock
            .side
            .map(|_| Rangef::new(playfield.max, columns.ruleset.max)),
        rows,
        left: playfield.min + mock.sky_w,
        lane_w: mock.lane_w,
        count: mock.lanes(),
        view,
    };
    let visuals = ui.visuals();
    let header_row = Rangef::new(area.top(), rows.min);
    header(&painter, &columns, &lanes, mock, header_row, visuals);
    let field = painter.with_clip_rect(Rect::from_x_y_ranges(area.x_range(), rows));
    backdrop(&field, &columns, &lanes);
    grid(&field, &columns, view, mock.timing);
    let band = Rect::from_x_y_ranges(columns.timeline(), view.now_y - 12.0..=view.now_y + 12.0);
    field.rect_filled(band, 0.0, NOW_BAND);
    waveform(&field, columns.wave, rows, view);
    (mock.draw)(&field, &lanes);
    now_line(&field, &columns, view.now_y);
    let warning = mock.problem.map(|_| visuals.warn_fg_color);
    overview(&field, columns.overview, rows, mock.direction, warning);
}

/// Says whose timing the chart uses, which key places a note in each lane, and what any side
/// view shows.
fn header(
    painter: &Painter,
    columns: &Columns,
    lanes: &Lanes,
    mock: &Mock,
    row: Rangef,
    visuals: &Visuals,
) {
    let caption = |x: f32, text: &str| {
        painter.text(
            pos2(x, row.center()),
            Align2::CENTER_CENTER,
            text,
            FontId::proportional(12.0),
            visuals.weak_text_color(),
        );
    };
    caption(columns.timing.center(), "song");
    if let (Some(side), Some((title, _))) = (lanes.side, mock.side) {
        caption(side.center(), title);
    }
    let half = (lanes.lane_w - 6.0).min(32.0) / 2.0;
    for (lane, key) in (0_u16..).zip(mock.keys) {
        let middle = lanes.span(lane, 1).center();
        let cap = Rect::from_x_y_ranges(middle - half..=middle + half, row.shrink(3.0));
        painter.rect_filled(cap, 3.0, KEY_CAP);
        painter.rect_stroke(cap, 3.0, Stroke::new(1.0, KEY_EDGE), StrokeKind::Inside);
        painter.text(
            cap.center(),
            Align2::CENTER_CENTER,
            *key,
            FontId::monospace(12.0),
            visuals.text_color(),
        );
    }
}

fn backdrop(painter: &Painter, columns: &Columns, lanes: &Lanes) {
    let rows = lanes.rows;
    for column in [
        columns.ruler,
        columns.timing,
        columns.wave,
        columns.overview,
    ] {
        painter.rect_filled(Rect::from_x_y_ranges(column, rows), 0.0, COLUMN_BG);
    }
    painter.rect_filled(Rect::from_x_y_ranges(columns.ruleset, rows), 0.0, LANES_BG);
    for lane in 0..=lanes.count {
        painter.vline(lanes.span(lane, 0).min, rows, Stroke::new(1.0, BEAT_LINE));
    }
    if let Some(side) = lanes.side {
        painter.vline(side.min, rows, Stroke::new(1.0, DIVIDER));
    }
    let edges = [
        columns.ruler.min,
        columns.timing.min,
        columns.ruleset.min,
        columns.wave.min,
        columns.overview.min,
        columns.overview.max,
    ];
    for x in edges {
        painter.vline(x, rows, Stroke::new(1.0, DIVIDER));
    }
}

/// Beat and measure lines, measure numbers, markers and timing events. Labels stay inside their
/// columns when the columns narrow.
fn grid(painter: &Painter, columns: &Columns, view: View, timing: &[(Beat, &str)]) {
    let rows = painter.clip_rect().y_range();
    let ruler = painter.with_clip_rect(Rect::from_x_y_ranges(columns.ruler, rows));
    let gutter = painter.with_clip_rect(Rect::from_x_y_ranges(columns.timing, rows));
    for beat in -8_i16..=28 {
        let y = view.y(Beat(f32::from(beat)));
        painter.hline(columns.timeline(), y, Stroke::new(1.0, BEAT_LINE));
    }
    for measure in -2_i16..=7 {
        let y = view.y(Beat(f32::from(measure.saturating_mul(4))));
        painter.hline(columns.timeline(), y, Stroke::new(1.0, MEASURE_LINE));
        let (at, anchor) = view.after(columns.ruler.max - 14.0, y, Align::Max);
        let number = (12_i16.saturating_add(measure)).to_string();
        ruler.text(at, anchor, number, FontId::monospace(12.0), TEXT);
    }
    for (beat, name, colour) in MARKERS {
        let y = view.y(beat);
        painter.hline(columns.timeline(), y, Stroke::new(2.0, colour));
        let (at, anchor) = view.after(columns.ruler.min + 6.0, y, Align::Min);
        ruler.text(at, anchor, name, FontId::proportional(12.0), colour);
    }
    for &(beat, event) in timing {
        let y = view.y(beat);
        painter.hline(columns.timing, y, Stroke::new(2.0, TIMING_MARK));
        let (at, anchor) = view.after(columns.timing.min + 6.0, y, Align::Min);
        gutter.text(at, anchor, event, FontId::monospace(11.0), TEXT);
    }
}

/// The cursor: a line across every column on the time axis, with a handle on the ruler.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "egui positions are offset by vectors"
)]
fn now_line(painter: &Painter, columns: &Columns, now_y: f32) {
    painter.hline(columns.timeline(), now_y, Stroke::new(2.0, NOW_LINE));
    let tip = pos2(columns.ruler.max, now_y);
    let handle = vec![tip + vec2(-11.0, -6.0), tip, tip + vec2(-11.0, 6.0)];
    painter.add(Shape::convex_polygon(handle, NOW_LINE, Stroke::NONE));
}

/// A made-up signal with an onset on every eighth note.
fn waveform(painter: &Painter, column: Rangef, rows: Rangef, view: View) {
    let half = column.span() / 2.0 - 8.0;
    let onset = view.y(Beat(0.0));
    let eighth = BEAT_PX / 2.0;
    let mut y = rows.min;
    while y < rows.max {
        let since = (view.direction.sign() * (y - onset)).rem_euclid(eighth) / eighth;
        let level = (0.15 + 0.6 * (-4.0 * since).exp() + 0.25 * (y * 0.37).sin().abs()).min(1.0);
        let reach = Rangef::new(
            column.center() - half * level,
            column.center() + half * level,
        );
        painter.hline(reach, y, Stroke::new(2.0, WAVE));
        y += 2.0;
    }
}

/// The whole chart at once: density, a section, a bookmark, any warning and the visible range.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "egui positions are offset by vectors"
)]
fn overview(
    painter: &Painter,
    column: Rangef,
    rows: Rangef,
    direction: Direction,
    warning: Option<Color32>,
) {
    let bars = column.shrink(4.0);
    let mut y = rows.min + 4.0;
    while y < rows.max - 4.0 {
        let density = 0.25 + 0.75 * ((y - rows.min) * 0.023).sin().abs();
        let bar = Rangef::new(bars.min, bars.min + bars.span() * density);
        painter.hline(bar, y, Stroke::new(2.0, DENSITY));
        y += 3.0;
    }
    let at = |fraction: f32| direction.start(rows) + direction.sign() * rows.span() * fraction;
    painter.hline(column, at(0.34), Stroke::new(2.0, SECTION));
    painter.hline(column, at(0.41), Stroke::new(2.0, BOOKMARK));
    if let Some(colour) = warning {
        let mark = pos2(column.center(), at(0.72));
        let triangle = vec![
            mark + vec2(0.0, -5.0),
            mark + vec2(5.0, 4.0),
            mark + vec2(-5.0, 4.0),
        ];
        painter.add(Shape::convex_polygon(triangle, colour, Stroke::NONE));
    }
    let visible = Rect::from_two_pos(pos2(column.min, at(0.3)), pos2(column.max, at(0.38)));
    painter.rect_stroke(
        visible,
        2.0,
        Stroke::new(1.5, Color32::WHITE),
        StrokeKind::Inside,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn later_beats_move_the_way_the_ruleset_scrolls() {
        let rows = Rangef::new(0.0, 1000.0);
        let down = View::new(rows, Direction::Down);
        let up = View::new(rows, Direction::Up);
        assert!(down.now_y < 500.0 && down.y(Beat(3.0)) > down.now_y);
        assert!(up.now_y > 500.0 && up.y(Beat(3.0)) < up.now_y);
    }
}
