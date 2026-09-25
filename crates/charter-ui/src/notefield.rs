//! The notefield: a chart drawn down the screen against audio time, with the cursor on the now
//! line.

use std::num::NonZeroU16;

use charter_core::{
    Kind, LaneCount, LaneIndex, MarkerKind, Object, Position, ResolvedTime, Shape, Tick, Ticks,
    Timing, TimingKind, Valid,
};
use egui::{
    self, Align2, FontId, Painter, Pos2, Rangef, Rect, Sense, Stroke, StrokeKind, Ui, Visuals,
    pos2, vec2,
};

use charter_editor::{NoteAt, Selecting, Song, Step};

use crate::style;
use crate::{Action, Note};

/// How tall a second of audio is on screen, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zoom(f32);

impl Zoom {
    /// Where a song opens: a 16th stream at 150 BPM sits about 25 points apart.
    pub(crate) const START: Self = Self(240.0);
    const CLOSEST: f32 = 3000.0;
    const FARTHEST: f32 = 30.0;
    /// How much one zoom step scales by.
    pub const STEP: f32 = 1.25;

    pub fn points_per_second(self) -> f32 {
        self.0
    }

    /// How close the zoom is, against where a song opens.
    pub fn percent(self) -> f32 {
        self.0 / Self::START.0 * 100.0
    }

    /// This zoom scaled by `factor`, as far as it goes either way.
    #[must_use]
    pub fn by(self, factor: f32) -> Self {
        Self((self.0 * factor).clamp(Self::FARTHEST, Self::CLOSEST))
    }
}

impl Default for Zoom {
    fn default() -> Self {
        Self::START
    }
}

/// Where audio times fall on screen in one frame: later times lower, the cursor on the now line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    now_y: f32,
    cursor: f64,
    zoom: Zoom,
}

impl View {
    pub fn new(now_y: f32, cursor: ResolvedTime, zoom: Zoom) -> Self {
        Self {
            now_y,
            cursor: cursor.seconds(),
            zoom,
        }
    }

    /// Where `at` is drawn.
    pub fn y(self, at: ResolvedTime) -> f32 {
        self.y_of(at.seconds())
    }

    /// Where audio `seconds` into the song is drawn.
    pub fn y_of(self, seconds: f64) -> f32 {
        let below = (seconds - self.cursor) * f64::from(self.zoom.points_per_second());
        #[allow(clippy::cast_possible_truncation)] // a screen coordinate, far inside f32
        let below = below as f32;
        self.now_y + below
    }

    /// The audio time drawn at height `y`.
    pub fn seconds_at(self, y: f32) -> f64 {
        self.cursor + f64::from(y - self.now_y) / f64::from(self.zoom.points_per_second())
    }
}

/// The last tick an object on which is hit at or before `seconds`. Tick to time never decreases,
/// so a binary search over every tick finds it without inverting the timing map.
pub fn latest_tick_by(timing: Valid<'_, Timing>, seconds: f64) -> Tick {
    let (mut early, mut late) = (i64::from(i32::MIN), i64::from(i32::MAX));
    let tick = |at: i64| Tick::new(i32::try_from(at).unwrap_or(i32::MAX));
    if timing.time_at(tick(late)).seconds() <= seconds {
        return tick(late);
    }
    while late - early > 1 {
        let middle = early + (late - early) / 2;
        if timing.time_at(tick(middle)).seconds() <= seconds {
            early = middle;
        } else {
            late = middle;
        }
    }
    tick(early)
}

/// The line of a grid `step` ticks apart from tick 0 that is drawn nearest to `seconds`.
pub fn nearest_line(timing: Valid<'_, Timing>, seconds: f64, step: Ticks) -> Tick {
    let latest = latest_tick_by(timing, seconds);
    let below = latest
        .next_on(step)
        .and_then(|next| next.previous_on(step))
        .unwrap_or(latest);
    let above = below.next_on(step).unwrap_or(below);
    let off = |line: Tick| (timing.time_at(line).seconds() - seconds).abs();
    if off(above) < off(below) {
        above
    } else {
        below
    }
}

/// The first tick an object on which is hit at or after `seconds`.
pub fn earliest_tick_from(timing: Valid<'_, Timing>, seconds: f64) -> Tick {
    let latest = latest_tick_by(timing, seconds);
    if timing.time_at(latest).seconds() >= seconds {
        return latest;
    }
    Ticks::new(1)
        .and_then(|one| latest.checked_add(one))
        .unwrap_or(latest)
}

/// Column widths: the ruler; the timing gutter at its widest and narrowest; a lane at its widest
/// and narrowest.
const RULER_W: f32 = 80.0;
const TIMING_W: f32 = 160.0;
const TIMING_MIN_W: f32 = 72.0;
const LANE_W: f32 = 56.0;
const LANE_MIN_W: f32 = 20.0;
const HEADER_H: f32 = 26.0;
/// How far down the notefield the now line sits.
const NOW_AT: f32 = 0.2;
/// How far past the edges of the notefield objects are still drawn, so a note half off screen
/// shows its visible half.
const MARGIN: f32 = 20.0;
/// Most beat lines drawn in one measure, for meters with absurdly many beats.
const MOST_BEATS: usize = 64;

/// Where each notefield column sits across the window.
pub(crate) struct Columns {
    ruler: Rangef,
    timing: Rangef,
    lanes: Rangef,
    lane_w: f32,
}

impl Columns {
    /// Centres the columns in `across`. Short of room, the timing gutter narrows first, then the
    /// lanes, and past that the columns start at the left edge.
    fn laid_out(across: Rangef, lanes: u16) -> Self {
        let count = f32::from(lanes);
        let room = across.span() - RULER_W - TIMING_MIN_W;
        let lane_w = if lanes == 0 {
            0.0
        } else {
            (room / count).clamp(LANE_MIN_W, LANE_W)
        };
        let timing = (across.span() - RULER_W - lane_w * count).clamp(TIMING_MIN_W, TIMING_W);
        let total = RULER_W + timing + lane_w * count;
        let left = (across.center() - total / 2.0).max(across.min);
        let ruler = Rangef::new(left, left + RULER_W);
        let timing = Rangef::new(ruler.max, ruler.max + timing);
        Self {
            ruler,
            timing,
            lanes: Rangef::new(timing.max, timing.max + lane_w * count),
            lane_w,
        }
    }

    /// The lanes a lane position covers.
    pub(crate) fn span(&self, index: LaneIndex, width: LaneCount) -> Rangef {
        let left = self.lanes.min + f32::from(u16::from(index)) * self.lane_w;
        Rangef::new(left, left + f32::from(u16::from(width)) * self.lane_w)
    }

    /// Every column on the time axis.
    fn timeline(&self) -> Rangef {
        Rangef::new(self.ruler.min, self.lanes.max.max(self.timing.max))
    }
}

/// A drag across the notefield in progress.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gesture {
    /// A hold being drawn out of the tap Ctrl placed where the drag began.
    Hold { lane: LaneIndex, from: Tick },
    /// A box selecting the notes whose heads it covers, from a corner kept in audio time so the
    /// box stays put if the chart scrolls under it.
    Marquee {
        how: Selecting,
        x: f32,
        seconds: f64,
    },
}

/// Where the notefield draws things in one frame.
pub(crate) struct Field {
    pub(crate) columns: Columns,
    rows: Rangef,
    pub(crate) view: View,
    lanes: u16,
}

impl Field {
    /// The notefield filling `area` for a chart of `lanes` lanes, with the cursor at `now`.
    pub(crate) fn new(area: Rect, lanes: u16, now: ResolvedTime, zoom: Zoom) -> Self {
        let rows = Rangef::new(area.top() + HEADER_H, area.bottom());
        Self {
            columns: Columns::laid_out(area.x_range(), lanes),
            rows,
            view: View::new(rows.min + rows.span() * NOW_AT, now, zoom),
            lanes,
        }
    }

    /// The lane under `x`, if the chart has one there.
    fn lane_at(&self, x: f32) -> Option<LaneIndex> {
        let lanes = self.columns.lanes;
        if !lanes.contains(x) || self.lanes == 0 {
            return None;
        }
        let across = ((x - lanes.min) / self.columns.lane_w).floor();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // a lane of the chart
        let lane = (across as u16).min(self.lanes - 1);
        Some(LaneIndex::new(lane))
    }

    /// The ticks and lanes a box covers, from a corner at `x` and `seconds` to `pos`, if it
    /// reaches a lane.
    fn boxed(
        &self,
        timing: Valid<'_, Timing>,
        (x, seconds): (f32, f64),
        pos: Pos2,
    ) -> Option<((Tick, Tick), (LaneIndex, LaneIndex))> {
        let lanes = self.lanes_between(x.min(pos.x), x.max(pos.x))?;
        let y = self.view.y_of(seconds);
        let (top, bottom) = (y.min(pos.y), y.max(pos.y));
        let ticks = (
            earliest_tick_from(timing, self.view.seconds_at(top)),
            latest_tick_by(timing, self.view.seconds_at(bottom)),
        );
        Some((ticks, lanes))
    }

    /// The first and last lanes a box spanning `left` to `right` reaches, if it reaches any.
    fn lanes_between(&self, left: f32, right: f32) -> Option<(LaneIndex, LaneIndex)> {
        let lanes = self.columns.lanes;
        if self.lanes == 0 || right < lanes.min || left > lanes.max {
            return None;
        }
        let inside = Rangef::new(lanes.min, lanes.max - 0.5);
        let first = self.lane_at(left.clamp(inside.min, inside.max))?;
        let last = self.lane_at(right.clamp(inside.min, inside.max))?;
        Some((first, last))
    }
}

/// Draws the open chart, and turns the wheel, zoom gestures, clicks and drags over it into
/// actions.
pub fn show(
    ui: &mut Ui,
    song: &Song,
    zoom: Zoom,
    scrolled: &mut f32,
    gesture: &mut Option<Gesture>,
    actions: &mut Vec<Action>,
) {
    let Some(chart) = song.chart() else {
        ui.weak("This song has no charts yet.");
        return;
    };
    let lanes = chart.lanes.map_or(0, u16::from);
    let indent = Columns::laid_out(ui.available_rect_before_wrap().x_range(), lanes)
        .ruler
        .min;
    let planar = chart
        .objects()
        .filter(|object| matches!(object.position, Position::Plane { .. }))
        .count();
    ui.horizontal(|ui| {
        ui.add_space((indent - ui.cursor().left()).max(0.0));
        ui.strong(chart.name.as_str());
        ui.weak(
            chart
                .lanes
                .map_or_else(|| "no lanes".to_owned(), |count| format!("{count} lanes")),
        );
        if planar > 0 {
            let ruleset = &song.document().document().ruleset.id;
            let (objects, need) = if planar == 1 {
                ("object", "needs")
            } else {
                ("objects", "need")
            };
            ui.weak(format!(
                "{planar} {objects} on the plane {need} the {ruleset} plugin to be drawn"
            ));
        }
    });
    // An id of its own, so a drag survives a message appearing above the notefield mid-drag.
    let (_, area) = ui.allocate_space(ui.available_size());
    let response = ui.interact(area, ui.id().with("notefield"), Sense::click_and_drag());
    let painter = ui.painter_at(area);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Notefield"));
    if response.hovered() {
        let (steps, zoom) =
            ui.input(|input| (wheel_steps(&input.events, scrolled), input.zoom_delta()));
        let step = if steps > 0 {
            Step::Later
        } else {
            Step::Earlier
        };
        actions.extend((0..steps.unsigned_abs()).map(|_| Action::Move(step)));
        if (zoom - 1.0).abs() > f32::EPSILON {
            actions.push(Action::ZoomBy(zoom));
        }
    }
    let area = response.rect;
    let timing = chart.timing();
    let field = Field::new(area, lanes, timing.time_at(song.cursor()), zoom);
    let (columns, rows, view) = (&field.columns, field.rows, field.view);
    // ponytail: every visible item still resolves its time from tick 0, which is slow once a
    // timing map holds thousands of events; walk the map once per frame when that shows.
    let first = latest_tick_by(timing, view.seconds_at(rows.min - MARGIN));
    let last = latest_tick_by(timing, view.seconds_at(rows.max + MARGIN));
    let own_timing = chart.timing.is_some();
    header(
        &painter,
        columns,
        lanes,
        own_timing,
        Rangef::new(area.top(), rows.min),
        ui.visuals(),
    );
    let clipped = painter.with_clip_rect(Rect::from_x_y_ranges(area.x_range(), rows));
    backdrop(&clipped, columns, lanes, rows);
    grid(&clipped, columns, song, view, rows);
    markers(&clipped, columns, song, view, (first, last));
    events(&clipped, columns, song, view, (first, last));
    let band = Rect::from_x_y_ranges(columns.timeline(), view.now_y - 12.0..=view.now_y + 12.0);
    clipped.rect_filled(band, 0.0, style::NOW_BAND);
    let mut shown = reaching(&chart.objects, first, last).peekable();
    for (index, object) in chart.objects().enumerate() {
        if shown.next_if_eq(&index).is_some() {
            let selected = song.is_selected(&object);
            draw_object(&clipped, columns, timing, view, object, selected);
        }
    }
    actions.extend(pointer(ui, &response, song, &field, (first, last), gesture));
    if let Some(Gesture::Marquee { x, seconds, .. }) = *gesture
        && let Some(latest) = ui.input(|input| input.pointer.latest_pos())
    {
        let corner = pos2(x, view.y_of(seconds));
        clipped.rect(
            Rect::from_two_pos(corner, latest),
            0.0,
            style::MARQUEE_FILL,
            Stroke::new(1.0, style::MARQUEE),
            StrokeKind::Inside,
        );
    }
    now_line(&clipped, columns, view.now_y);
}

/// What clicks and drags over the notefield ask for. With Ctrl they place notes, a tap or under
/// Shift a mine, and a drag draws a hold; otherwise a click selects the note under the pointer,
/// Shift adding it and Alt taking it away, and a drag selects with a box.
fn pointer(
    ui: &Ui,
    response: &egui::Response,
    song: &Song,
    field: &Field,
    visible: (Tick, Tick),
    gesture: &mut Option<Gesture>,
) -> Vec<Action> {
    let mut actions = Vec::new();
    let Some(chart) = song.chart() else {
        return actions;
    };
    let timing = chart.timing();
    let (modifiers, origin, latest, escaped) = ui.input(|input| {
        let pointer = &input.pointer;
        let escaped = input.key_pressed(egui::Key::Escape);
        (
            input.modifiers,
            pointer.press_origin(),
            pointer.latest_pos(),
            escaped,
        )
    });
    // A gesture lasts as long as its drag, and Escape drops it.
    if escaped || !(response.dragged() || response.drag_stopped()) {
        *gesture = None;
    }
    let step = song
        .document()
        .document()
        .ticks_per_beat
        .note_length(song.snap());
    let line =
        |pos: Pos2| step.map(|step| nearest_line(timing, field.view.seconds_at(pos.y), step));
    let editable = song.read_only().is_none();
    let spot = |pos: Pos2| {
        if !editable {
            return None;
        }
        Some((field.lane_at(pos.x)?, line(pos)?))
    };
    let how = selecting(modifiers);
    if response.clicked()
        && let Some(pos) = latest
    {
        if !modifiers.command {
            actions.push(Action::Select(
                hit(&chart.objects, field, timing, pos, visible),
                how,
            ));
        } else if let Some((lane, tick)) = spot(pos) {
            let note = if modifiers.shift {
                Note::Mine
            } else {
                Note::Tap
            };
            actions.push(Action::Place { tick, lane, note });
        }
    }
    if response.drag_started()
        && !escaped
        && let Some(origin) = origin
    {
        *gesture = if modifiers.command {
            spot(origin).map(|(lane, from)| {
                let note = Note::Tap;
                actions.push(Action::Place {
                    tick: from,
                    lane,
                    note,
                });
                Gesture::Hold { lane, from }
            })
        } else {
            let seconds = field.view.seconds_at(origin.y);
            Some(Gesture::Marquee {
                how,
                x: origin.x,
                seconds,
            })
        };
    }
    if response.dragged()
        && let (Some(Gesture::Hold { lane, from }), Some(pos)) = (*gesture, latest)
        && let Some(to) = line(pos)
    {
        actions.push(Action::Stretch { lane, from, to });
    }
    if response.drag_stopped() {
        match (gesture.take(), latest) {
            (Some(Gesture::Hold { lane, from }), Some(pos)) => {
                actions.extend(line(pos).map(|to| Action::Stretch { lane, from, to }));
            }
            (Some(Gesture::Marquee { how, x, seconds }), Some(pos)) => {
                let corner = (x, seconds);
                actions.extend(
                    field
                        .boxed(timing, corner, pos)
                        .map(|(ticks, lanes)| Action::SelectBetween { ticks, lanes, how }),
                );
            }
            _ => {}
        }
    }
    actions
}

/// How a click or a box meets the selection: Shift adds to it and Alt takes from it.
fn selecting(modifiers: egui::Modifiers) -> Selecting {
    if modifiers.alt {
        Selecting::Subtract
    } else if modifiers.shift {
        Selecting::Add
    } else {
        Selecting::Replace
    }
}

/// How far from a note, in points, a click still takes it.
const GRAB: f32 = 6.0;

/// The note drawn under `pos`, the one whose head is nearest when several are.
fn hit(
    objects: &[Object],
    field: &Field,
    timing: Valid<'_, Timing>,
    pos: Pos2,
    (first, last): (Tick, Tick),
) -> Option<NoteAt> {
    let under = reaching(objects, first, last).filter_map(|index| {
        let object = objects.get(index)?;
        let Position::Lane { index, width } = object.position else {
            return None;
        };
        let head = field.view.y(timing.time_at(object.tick));
        let tail = field.view.y(timing.time_at(object.shape.end(object.tick)?));
        let reach = Rangef::new(head.min(tail) - GRAB, head.max(tail) + GRAB);
        let over = field.columns.span(index, width).contains(pos.x) && reach.contains(pos.y);
        let note = NoteAt {
            tick: object.tick,
            lane: index,
        };
        over.then_some((note, (pos.y - head).abs()))
    });
    under
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(note, _)| note)
}

/// Says whose timing the chart uses and numbers its lanes.
fn header(
    painter: &Painter,
    columns: &Columns,
    lanes: u16,
    own_timing: bool,
    row: Rangef,
    visuals: &Visuals,
) {
    let caption = |x: f32, text: String| {
        painter.text(
            pos2(x, row.center()),
            Align2::CENTER_CENTER,
            text,
            FontId::proportional(12.0),
            visuals.weak_text_color(),
        );
    };
    caption(
        columns.timing.center(),
        if own_timing { "chart" } else { "song" }.to_owned(),
    );
    for lane in 0..lanes {
        let span = columns.span(LaneIndex::new(lane), LaneCount::ONE);
        caption(span.center(), (lane + 1).to_string());
    }
}

fn backdrop(painter: &Painter, columns: &Columns, lanes: u16, rows: Rangef) {
    for column in [columns.ruler, columns.timing] {
        painter.rect_filled(Rect::from_x_y_ranges(column, rows), 0.0, style::COLUMN_BG);
    }
    painter.rect_filled(
        Rect::from_x_y_ranges(columns.lanes, rows),
        0.0,
        style::LANES_BG,
    );
    for lane in 1..lanes {
        let x = columns.span(LaneIndex::new(lane), LaneCount::ONE).min;
        painter.vline(x, rows, Stroke::new(1.0, style::BEAT_LINE));
    }
    for x in [
        columns.ruler.min,
        columns.timing.min,
        columns.lanes.min,
        columns.lanes.max,
    ] {
        painter.vline(x, rows, Stroke::new(1.0, style::DIVIDER));
    }
}

/// Measure and beat lines over the visible time, with measure numbers.
fn grid(painter: &Painter, columns: &Columns, song: &Song, view: View, rows: Rangef) {
    let timing = song.timing();
    let beat = song
        .document()
        .document()
        .ticks_per_beat
        .note_length(QUARTER);
    let ruler = painter.with_clip_rect(Rect::from_x_y_ranges(columns.ruler, rows));
    let first = latest_tick_by(timing, view.seconds_at(rows.min));
    for measure in timing.measures_from(first) {
        let y = view.y(timing.time_at(measure.start));
        if y > rows.max {
            break;
        }
        painter.hline(columns.timeline(), y, Stroke::new(1.0, style::MEASURE_LINE));
        let label = pos2(columns.ruler.max - 14.0, y + 2.0);
        ruler.text(
            label,
            Align2::RIGHT_TOP,
            measure.number.to_string(),
            FontId::monospace(12.0),
            style::TEXT,
        );
        let (Some(beat), Some(end)) = (beat, measure.start.checked_add(measure.length)) else {
            continue;
        };
        let beats = std::iter::successors(measure.start.checked_add(beat), |tick| {
            tick.checked_add(beat)
        });
        for tick in beats.take_while(|tick| *tick < end).take(MOST_BEATS) {
            let y = view.y(timing.time_at(tick));
            painter.hline(columns.timeline(), y, Stroke::new(1.0, style::BEAT_LINE));
        }
    }
}

const QUARTER: NonZeroU16 = NonZeroU16::new(4).unwrap();

/// Sections, bookmarks and plugin markers as lines across the time axis, named in the ruler.
fn markers(
    painter: &Painter,
    columns: &Columns,
    song: &Song,
    view: View,
    (first, last): (Tick, Tick),
) {
    let timing = song.timing();
    let ruler = painter.with_clip_rect(Rect::from_x_y_ranges(
        columns.ruler,
        painter.clip_rect().y_range(),
    ));
    let markers = &song.document().document().markers;
    for marker in between(markers, |marker| marker.tick, first, last) {
        let colour = match marker.kind {
            MarkerKind::Section => style::SECTION,
            MarkerKind::Bookmark => style::BOOKMARK,
            MarkerKind::Other(_) => style::OTHER,
        };
        let y = view.y(timing.time_at(marker.tick));
        painter.hline(columns.timeline(), y, Stroke::new(2.0, colour));
        let at = pos2(columns.ruler.min + 6.0, y + 3.0);
        ruler.text(
            at,
            Align2::LEFT_TOP,
            &marker.text,
            FontId::proportional(12.0),
            colour,
        );
    }
}

/// The chart's timing events in the gutter, each named with its value.
fn events(
    painter: &Painter,
    columns: &Columns,
    song: &Song,
    view: View,
    (first, last): (Tick, Tick),
) {
    let timing = song.timing();
    let gutter = painter.with_clip_rect(Rect::from_x_y_ranges(
        columns.timing,
        painter.clip_rect().y_range(),
    ));
    for event in between(&timing.events, |event| event.tick, first, last) {
        let y = view.y(timing.time_at(event.tick));
        painter.hline(columns.timing, y, Stroke::new(2.0, style::TIMING_MARK));
        let text = match &event.kind {
            TimingKind::Bpm(bpm) => format!("bpm {bpm}"),
            TimingKind::Stop(pause) => format!("stop {pause} s"),
            TimingKind::Delay(pause) => format!("delay {pause} s"),
            TimingKind::Warp(ticks) => format!("warp {ticks} ticks"),
            TimingKind::TimeSignature(meter) => format!("meter {meter}"),
            TimingKind::Other { name, value } => format!("{name} {value}"),
        };
        let at = pos2(columns.timing.min + 6.0, y + 3.0);
        gutter.text(
            at,
            Align2::LEFT_TOP,
            text,
            FontId::monospace(11.0),
            style::TEXT,
        );
    }
}

/// One object on lanes, in the shared vocabulary's glyphs: the ruleset draws its own kinds
/// once plugins can draw.
fn draw_object(
    painter: &Painter,
    columns: &Columns,
    timing: Valid<'_, Timing>,
    view: View,
    object: Valid<'_, Object>,
    selected: bool,
) {
    let Position::Lane { index, width } = object.position else {
        return;
    };
    let start = view.y(timing.time_at(object.tick));
    let end = view.y(timing.time_at(object.end()));
    let across = columns
        .span(index, width)
        .shrink((columns.lane_w * 0.12).max(2.0));
    let (head, body) = match object.kind {
        Kind::Tap | Kind::Lift | Kind::Hold => (style::TAP, style::HOLD),
        Kind::Roll => (style::ROLL, style::ROLL.gamma_multiply(0.5)),
        Kind::Mine => (style::MINE, style::MINE.gamma_multiply(0.5)),
        Kind::Fake => (style::FAKE, style::FAKE.gamma_multiply(0.5)),
        Kind::Other(_) => (style::OTHER, style::OTHER.gamma_multiply(0.5)),
    };
    match &object.shape {
        Shape::Instant => {}
        Shape::Sustain { .. } => {
            let tail = Rect::from_two_pos(
                pos2(across.min + across.span() * 0.25, start),
                pos2(across.max - across.span() * 0.25, end),
            );
            painter.rect_filled(tail, 2.0, body);
        }
        Shape::Path { points } => {
            let mut line = vec![pos2(across.center(), start)];
            for point in points.as_slice() {
                if let Position::Lane { index, width } = point.position {
                    line.push(pos2(
                        columns.span(index, width).center(),
                        view.y(timing.time_at(point.tick)),
                    ));
                }
            }
            painter.add(egui::Shape::line(line, Stroke::new(4.0, body)));
        }
    }
    let rect = Rect::from_x_y_ranges(across, start - 5.0..=start + 5.0);
    if selected {
        painter.rect_stroke(
            rect.expand(3.0),
            5.0,
            Stroke::new(2.0, style::SELECTED),
            StrokeKind::Outside,
        );
    }
    match object.kind {
        Kind::Mine => {
            painter.circle_stroke(rect.center(), 6.0, Stroke::new(2.0, head));
        }
        Kind::Lift => {
            painter.rect_stroke(rect, 3.0, Stroke::new(2.0, head), StrokeKind::Inside);
        }
        _ => {
            painter.rect_filled(rect, 3.0, head);
        }
    }
    for child in object.children() {
        if let Position::Lane { index, width } = child.position {
            let centre = pos2(
                columns.span(index, width).center(),
                view.y(timing.time_at(child.tick)),
            );
            painter.circle_filled(centre, 3.0, style::TEXT);
        }
    }
}

/// Where in `objects`, sorted by tick, are those whose spans reach into `first..=last`.
fn reaching(objects: &[Object], first: Tick, last: Tick) -> impl Iterator<Item = usize> + '_ {
    let until = objects.partition_point(|object| object.tick <= last);
    (0..until).filter(move |&index| {
        objects
            .get(index)
            .is_some_and(|object| object.shape.end(object.tick).is_none_or(|end| end >= first))
    })
}

/// The part of `items`, sorted by tick, that sits on `first..=last`.
fn between<T>(items: &[T], tick: impl Fn(&T) -> Tick, first: Tick, last: Tick) -> &[T] {
    let from = items.partition_point(|item| tick(item) < first);
    let to = items.partition_point(|item| tick(item) <= last);
    items.get(from..to).unwrap_or_default()
}

/// The cursor: a line across every column on the time axis, with a handle on the ruler.
fn now_line(painter: &Painter, columns: &Columns, now_y: f32) {
    painter.hline(columns.timeline(), now_y, Stroke::new(2.0, style::NOW_LINE));
    let tip = pos2(columns.ruler.max, now_y);
    let handle = vec![tip + vec2(-11.0, -6.0), tip, tip + vec2(-11.0, 6.0)];
    painter.add(egui::Shape::convex_polygon(
        handle,
        style::NOW_LINE,
        Stroke::NONE,
    ));
}

/// Cursor steps the mouse wheel asks for in `events`, later first, keeping part-steps in
/// `remainder`: a whole line of scrolling, one wheel notch, is one step, and touchpads and smooth
/// wheels send fractions that add up.
pub fn wheel_steps(events: &[egui::Event], remainder: &mut f32) -> i32 {
    let mut steps = 0.0;
    for event in events {
        let egui::Event::MouseWheel {
            unit,
            delta,
            modifiers,
            ..
        } = event
        else {
            continue;
        };
        if modifiers.command || modifiers.ctrl {
            continue;
        }
        let points = match unit {
            egui::MouseWheelUnit::Point => -delta.y,
            egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page => -delta.y * POINTS_PER_STEP,
        };
        *remainder += points;
        let whole = (*remainder / POINTS_PER_STEP).trunc();
        *remainder -= whole * POINTS_PER_STEP;
        steps += whole;
    }
    #[allow(clippy::cast_possible_truncation)] // a handful of notches a frame
    let steps = steps as i32;
    steps
}

/// Points of touchpad scrolling that make one cursor step.
const POINTS_PER_STEP: f32 = 40.0;

#[cfg(test)]
mod tests {
    use egui::{Modifiers, MouseWheelUnit, TouchPhase, Vec2};

    use super::*;
    use charter_editor::demo;

    fn wheel(unit: MouseWheelUnit, down: f32, modifiers: Modifiers) -> egui::Event {
        egui::Event::MouseWheel {
            unit,
            delta: Vec2::new(0.0, -down),
            phase: TouchPhase::Move,
            modifiers,
        }
    }

    #[test]
    fn a_sustain_reaching_through_the_view_counts_as_visible() {
        let objects: Vec<Object> = serde_json::from_value(serde_json::json!([
            { "tick": 0, "position": { "lane": { "index": 0 } }, "kind": "hold",
              "shape": { "sustain": { "duration": 10000 } } },
            { "tick": 100, "position": { "lane": { "index": 1 } }, "kind": "tap", "shape": "instant" },
            { "tick": 4500, "position": { "lane": { "index": 1 } }, "kind": "tap", "shape": "instant" },
            { "tick": 6000, "position": { "lane": { "index": 1 } }, "kind": "tap", "shape": "instant" },
        ]))
        .unwrap();
        let shown: Vec<_> = reaching(&objects, Tick::new(4000), Tick::new(5000)).collect();
        assert_eq!(shown, [0, 2]);
    }

    #[test]
    fn fractions_of_a_line_add_up_to_a_step() {
        let mut remainder = 0.0;
        let quarter = wheel(MouseWheelUnit::Line, 0.25, Modifiers::NONE);
        let events = [quarter.clone(), quarter.clone(), quarter.clone()];
        assert_eq!(wheel_steps(&events, &mut remainder), 0);
        assert_eq!(wheel_steps(&[quarter], &mut remainder), 1);
    }

    #[test]
    fn a_wheel_notch_steps_once_and_touchpad_scrolling_adds_up() {
        let mut remainder = 0.0;
        let notch = wheel(MouseWheelUnit::Line, 1.0, Modifiers::NONE);
        assert_eq!(wheel_steps(&[notch], &mut remainder), 1);
        let nudge = wheel(MouseWheelUnit::Point, -15.0, Modifiers::NONE);
        assert_eq!(
            wheel_steps(&[nudge.clone(), nudge.clone()], &mut remainder),
            0
        );
        assert_eq!(wheel_steps(&[nudge], &mut remainder), -1);
        let zoom = wheel(MouseWheelUnit::Line, 1.0, Modifiers::COMMAND);
        assert_eq!(wheel_steps(&[zoom], &mut remainder), 0);
    }

    fn song() -> Song {
        Song::new(demo::song(), &[])
    }

    #[test]
    fn zoom_grows_and_shrinks_by_a_quarter_within_bounds() {
        let start = Zoom::START;
        let closer = start.by(Zoom::STEP);
        assert!((closer.points_per_second() - start.points_per_second() * 1.25).abs() < 1e-3);
        let closest = (0..50).fold(start, |zoom, _| zoom.by(Zoom::STEP));
        let widest = closest.points_per_second();
        assert!((closest.by(Zoom::STEP).points_per_second() - widest).abs() < 1e-3);
        let farthest = (0..100).fold(closest, |zoom, _| zoom.by(Zoom::STEP.recip()));
        assert!(farthest.points_per_second() > 0.0);
    }

    #[test]
    fn zoom_opens_at_a_hundred_percent_and_scales_within_bounds() {
        let zoom = Zoom::START;
        assert!((zoom.percent() - 100.0).abs() < 1e-3);
        assert!((zoom.by(2.0).percent() - 200.0).abs() < 1e-3);
        assert!((zoom.by(2.0).by(1e6).points_per_second() - 3000.0).abs() < 1e-3);
    }

    #[test]
    fn later_audio_sits_lower_by_the_zoom_per_second() {
        let song = song();
        let timing = song.timing();
        let view = View::new(100.0, timing.time_at(Tick::new(0)), Zoom::START);
        let beat_later = view.y(timing.time_at(Tick::new(960)));
        let per_second = Zoom::START.points_per_second();
        assert!(
            (beat_later - (100.0 + 0.4 * per_second)).abs() < 1e-3,
            "{beat_later}"
        );
        assert!(view.y(timing.time_at(Tick::new(-960))) < 100.0);
    }

    #[test]
    fn a_height_turns_back_into_the_time_drawn_there() {
        let song = song();
        let timing = song.timing();
        let view = View::new(100.0, timing.time_at(Tick::new(960)), Zoom::START);
        let y = view.y(timing.time_at(Tick::new(1920)));
        assert!((view.seconds_at(y) - 0.8).abs() < 1e-6);
    }

    #[test]
    fn a_time_snaps_to_the_nearest_line_of_the_grid() {
        let song = song();
        let timing = song.timing();
        let sixteenth = Ticks::new(240).unwrap();
        assert_eq!(nearest_line(timing, 0.26, sixteenth), Tick::new(720));
        assert_eq!(nearest_line(timing, 0.24, sixteenth), Tick::new(480));
        assert_eq!(nearest_line(timing, -0.01, sixteenth), Tick::new(0));
    }

    #[test]
    fn the_earliest_tick_from_a_time_skips_what_a_stop_left_behind() {
        let song = song();
        let timing = song.timing();
        assert_eq!(earliest_tick_from(timing, 0.5), Tick::new(1200));
        assert_eq!(earliest_tick_from(timing, 0.50001), Tick::new(1201));
        let stop = Tick::new(30720);
        let paused = timing.time_at(stop).seconds() + 0.25;
        assert_eq!(earliest_tick_from(timing, paused), Tick::new(30721));
    }

    #[test]
    fn a_box_over_a_chart_without_lanes_reaches_no_lane() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
        let now = song().timing().time_at(Tick::new(0));
        let field = Field::new(area, 0, now, Zoom::START);
        assert_eq!(field.lanes_between(0.0, 800.0), None);
    }

    #[test]
    fn the_latest_tick_by_a_time_waits_out_a_stop() {
        let song = song();
        let timing = song.timing();
        assert_eq!(latest_tick_by(timing, 0.5), Tick::new(1200));
        let stop = Tick::new(30720);
        let paused = timing.time_at(stop).seconds() + 0.25;
        assert_eq!(latest_tick_by(timing, paused), stop);
    }
}
