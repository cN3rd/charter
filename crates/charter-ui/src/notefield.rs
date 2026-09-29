//! The notefield: a chart drawn down the screen against audio time, with the cursor on the now
//! line.

use std::num::NonZeroU16;

use charter_core::{
    Chart, Kind, LaneCount, LaneIndex, MarkerKind, Measure, Object, Position, ResolvedTime, Shape,
    Tick, Ticks, TicksPerBeat, Timing, TimingKind, Valid,
};
use egui::{
    self, Align2, FontId, Pos2, Rangef, Rect, Sense, Stroke, StrokeKind, Ui, Visuals, pos2, vec2,
};

use charter_editor::{NoteAt, Selecting, Song, Step};

use charter_audio::{Peaks, Spectrum};

use crate::flip::{Canvas, Direction, Flip};
use crate::style;
use crate::{Action, Note};

/// How tall a second of audio is on screen, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zoom(f32);

impl Default for Zoom {
    /// Where a song opens: a 16th stream at 150 BPM sits about 25 points apart.
    fn default() -> Self {
        Self(240.0)
    }
}

impl Zoom {
    /// Highest permitted scale, in points per second.
    const CLOSEST: f32 = 3000.0;
    /// Lowest permitted scale, in points per second.
    const FARTHEST: f32 = 30.0;
    /// How much one zoom step scales by.
    pub const STEP: f32 = 1.25;

    /// The vertical scale in screen points per audio second.
    pub const fn points_per_second(self) -> f32 {
        self.0
    }

    /// How close the zoom is, against where a song opens.
    pub fn percent(self) -> f32 {
        self.0 / Self::default().0 * 100.0
    }

    /// This zoom scaled by `factor`, as far as it goes either way.
    #[must_use]
    pub fn by(self, factor: f32) -> Self {
        Self((self.0 * factor).clamp(Self::FARTHEST, Self::CLOSEST))
    }
}

/// Where audio times fall on screen in one frame: later times lower, the cursor on the now line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// Screen height of the cursor's now line, in points.
    pub(crate) now_y: f32,
    /// Cursor position in audio seconds.
    cursor: f64,
    /// Vertical scale of the audio timeline.
    zoom: Zoom,
}

impl View {
    /// A timeline with `cursor` aligned to `now_y`.
    pub const fn new(now_y: f32, cursor: ResolvedTime, zoom: Zoom) -> Self {
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
        let below = crate::cast::f64_f32(below);
        self.now_y + below
    }

    /// The audio time drawn at height `y`.
    pub fn seconds_at(self, y: f32) -> f64 {
        self.cursor + f64::from(y - self.now_y) / f64::from(self.zoom.points_per_second())
    }
}

/// The line of a grid `step` ticks apart from tick 0 that is drawn nearest to `seconds`.
pub fn nearest_line(timing: Valid<'_, Timing>, seconds: f64, step: Ticks) -> Tick {
    let latest = timing.latest_tick_by(seconds);
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
    let latest = timing.latest_tick_by(seconds);
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
/// Preferred timing gutter width, in points.
const TIMING_W: f32 = 160.0;
/// Smallest timing gutter width, in points.
const TIMING_MIN_W: f32 = 72.0;
/// Preferred width of each note lane, in points.
const LANE_W: f32 = 56.0;
/// How wide the waveform strip is, when there is a waveform to show.
const WAVE_W: f32 = 64.0;
/// Smallest width of each note lane, in points.
const LANE_MIN_W: f32 = 20.0;
/// Space above the timeline for lane labels, in points.
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
    /// Screen range reserved for measure numbers and markers.
    ruler: Rangef,
    /// Screen range reserved for timing events.
    timing: Rangef,
    /// The waveform strip, empty when there is no waveform.
    wave: Rangef,
    /// Screen range spanning all note lanes.
    lanes: Rangef,
    /// Width of one lane after fitting the columns into the window, in points.
    lane_w: f32,
}

impl Columns {
    /// Centres the columns in `across`. Short of room, the timing gutter narrows first, then the
    /// lanes, and past that the columns start at the left edge.
    fn laid_out(across: Rangef, lanes: u16, wave: bool) -> Self {
        let count = f32::from(lanes);
        let wave_w = if wave { WAVE_W } else { 0.0 };
        let room = across.span() - RULER_W - TIMING_MIN_W - wave_w;
        let lane_w = if lanes == 0 {
            0.0
        } else {
            (room / count).clamp(LANE_MIN_W, LANE_W)
        };
        let timing =
            (across.span() - RULER_W - wave_w - lane_w * count).clamp(TIMING_MIN_W, TIMING_W);
        let total = RULER_W + timing + wave_w + lane_w * count;
        let left = (across.center() - total / 2.0).max(across.min);
        let ruler = Rangef::new(left, left + RULER_W);
        let timing = Rangef::new(ruler.max, ruler.max + timing);
        let waveform = Rangef::new(timing.max, timing.max + wave_w);
        Self {
            ruler,
            timing,
            wave: waveform,
            lanes: Rangef::new(waveform.max, waveform.max + lane_w * count),
            lane_w,
        }
    }

    /// The lanes a lane position covers.
    pub(crate) fn span(&self, index: LaneIndex, width: LaneCount) -> Rangef {
        let left = self.lanes.min + f32::from(u16::from(index)) * self.lane_w;
        Rangef::new(left, left + f32::from(u16::from(width)) * self.lane_w)
    }

    /// The middle of the timing gutter, where its events are grabbed.
    #[cfg(test)]
    pub(crate) fn timing_x(&self) -> f32 {
        self.timing.center()
    }

    /// The middle of the ruler, where markers are grabbed.
    #[cfg(test)]
    pub(crate) fn ruler_x(&self) -> f32 {
        self.ruler.center()
    }

    /// The middle of the cursor's handle on the ruler.
    pub(crate) fn handle_x(&self) -> f32 {
        self.ruler.max - 5.0
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
    Hold {
        /// Lane where the initial tap was placed.
        lane: LaneIndex,
        /// Tick where the hold begins.
        from: Tick,
    },
    /// A box selecting the notes whose heads it covers, from a corner kept in audio time so the
    /// box stays put if the chart scrolls under it.
    Marquee {
        /// Whether covered notes replace or extend the current selection.
        how: Selecting,
        /// Screen x of the fixed corner.
        x: f32,
        /// Audio seconds at the fixed corner.
        seconds: f64,
    },
    /// The timing point being dragged to another line.
    MoveTiming {
        /// Tick shared by the events at this timing point.
        from: Tick,
    },
    /// The song's marker being dragged to another line.
    MoveMarker {
        /// Index into the song's markers.
        index: usize,
    },
    /// The cursor's handle, dragged from `y` on screen while the cursor was at `seconds`.
    Scrub {
        /// Cursor's audio time when the drag began, in seconds.
        seconds: f64,
        /// Screen height at the start of the drag, in points.
        y: f32,
    },
    /// The chart, grabbed with the middle button from `y` on screen while the cursor was at
    /// `seconds`, and dragged along as a page is.
    Pan {
        /// Cursor's audio time when the drag began, in seconds.
        seconds: f64,
        /// Screen height where the chart was grabbed, in points.
        y: f32,
    },
    /// The selected notes, grabbed on the snap line `tick` over `lane`. They can move `room.0`
    /// lanes left, a negative count, to `room.1` lanes right and stay on the lanes.
    MoveNotes {
        /// Snap line where the notes were grabbed.
        tick: Tick,
        /// Lane where the notes were grabbed.
        lane: LaneIndex,
        /// Permitted lane offsets to the left and right.
        room: (i32, i32),
    },
}

impl Gesture {
    /// How far the selection grabbed in a [`Gesture::MoveNotes`] has been dragged to `pos`, in
    /// ticks along the snap and lanes across: the snap line nearest `pos` from the one it was
    /// grabbed on, and the lane under `pos`, kept within the lanes, from the one it was grabbed
    /// over. With `keep_time`, as while Shift is held, the notes keep their ticks and only
    /// change lanes.
    fn moved_by(self, field: &Field, to: Tick, pos: Pos2, keep_time: bool) -> Option<(i32, i32)> {
        let Self::MoveNotes { tick, lane, room } = self else {
            return None;
        };
        let ticks = if keep_time {
            0
        } else {
            i32::try_from(i64::from(to).checked_sub(i64::from(tick))?).ok()?
        };
        let inside = field.columns.lanes;
        let over = field.lane_at(pos.x.clamp(inside.min, inside.max - 0.5))?;
        let lanes = i32::from(u16::from(over)).saturating_sub(i32::from(u16::from(lane)));
        Some((ticks, lanes.clamp(room.0, room.1.max(room.0))))
    }
}

/// The notes a drag from `note` moves, the selection when `note` is in it and otherwise `note`
/// alone, and how many lanes they have room to move left, a negative count, and right.
fn room(chart: Valid<'_, Chart>, song: &Song, note: NoteAt, lanes: u16) -> (bool, (i32, i32)) {
    let grabbed = |object: &Object| {
        object.tick == note.tick
            && matches!(object.position, Position::Lane { index, .. } if index == note.lane)
    };
    let selected = chart
        .objects
        .iter()
        .any(|object| grabbed(object) && song.is_selected(object));
    let spans = chart
        .objects
        .iter()
        .filter(|object| {
            if selected {
                song.is_selected(object)
            } else {
                grabbed(object)
            }
        })
        .filter_map(|object| match object.position {
            Position::Lane { index, width } => Some((index, width)),
            Position::Plane { .. } => None,
        });
    let (mut first, mut last) = (i32::MAX, i32::MIN);
    for (index, width) in spans {
        let from = i32::from(u16::from(index));
        first = first.min(from);
        last = last.max(
            from.saturating_add(i32::from(u16::from(width)))
                .saturating_sub(1),
        );
    }
    (
        selected,
        (
            first.saturating_neg(),
            i32::from(lanes).saturating_sub(1).saturating_sub(last),
        ),
    )
}

/// The handle under `pos`, if one is: the cursor's on the ruler, a timing point (every event at one
/// tick) in the gutter or a marker on the ruler, each within grabbing distance of its line. The song's first BPM
/// change stays put, so the song keeps a tempo to start from. Only the cursor's handle moves in a
/// song that cannot be edited.
fn handle_at(song: &Song, field: &Field, pos: Pos2) -> Option<Gesture> {
    let (columns, view) = (&field.columns, field.view);
    let near = |y: f32| (y - pos.y).abs() <= GRAB;
    let on_handle = Rangef::new(columns.handle_x() - 9.0, columns.handle_x() + 7.0);
    if on_handle.contains(pos.x) && near(view.now_y) {
        let seconds = view.seconds_at(view.now_y);
        return Some(Gesture::Scrub { seconds, y: pos.y });
    }
    if song.read_only().is_some() {
        return None;
    }
    let timing = song.timing();
    let nearest = |ticks: &mut dyn Iterator<Item = (usize, Tick)>| {
        ticks
            .map(|(index, tick)| ((index, tick), (view.y(timing.time_at(tick)) - pos.y).abs()))
            .filter(|(_, off)| *off <= GRAB)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(position, _)| position)
    };
    if columns.timing.contains(pos.x) {
        let mut points = timing
            .events
            .chunk_by(|a, b| a.tick == b.tick)
            .filter_map(|point| point.first().map(|event| event.tick))
            .chain(timing.anchors.iter().map(|anchor| anchor.tick))
            .enumerate();
        return nearest(&mut points).map(|(_, from)| Gesture::MoveTiming { from });
    }
    if columns.ruler.contains(pos.x) {
        let markers = &song.document().document().markers;
        let mut ticks = markers
            .iter()
            .enumerate()
            .map(|(index, marker)| (index, marker.tick));
        return nearest(&mut ticks).map(|(index, _)| Gesture::MoveMarker { index });
    }
    None
}

/// Where the notefield draws things in one frame.
pub(crate) struct Field {
    /// Fitted horizontal positions of the notefield columns.
    pub(crate) columns: Columns,
    /// Vertical drawing range below the header, in points.
    rows: Rangef,
    /// Timeline placing the cursor at the now line.
    pub(crate) view: View,
    /// Number of lanes in the open chart.
    lanes: u16,
}

impl Field {
    /// The notefield filling `area` for a chart of `lanes` lanes, with the cursor at `now`.
    pub(crate) fn new(area: Rect, lanes: u16, now: ResolvedTime, zoom: Zoom, wave: bool) -> Self {
        let rows = Rangef::new(area.top() + HEADER_H, area.bottom());
        Self {
            columns: Columns::laid_out(area.x_range(), lanes, wave),
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
        let lane = crate::cast::f32_u16(across).min(self.lanes.saturating_sub(1));
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
            timing.latest_tick_by(self.view.seconds_at(bottom)),
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
/// actions. Gives the seconds of audio it shows, earliest first, when there is a chart.
#[expect(
    clippy::too_many_lines,
    reason = "one pass over the frame in drawing order"
)]
pub fn show(
    ui: &mut Ui,
    song: &Song,
    Shown {
        zoom,
        direction,
        strip,
        grid: finest,
        colour_notes_by_layer,
    }: Shown<'_>,
    scrolled: &mut [f32; 2],
    gesture: &mut Option<Gesture>,
    actions: &mut Vec<Action>,
) -> Option<(f64, f64)> {
    let Some(chart) = song.chart() else {
        ui.weak("This song has no charts yet.");
        return None;
    };
    let lanes = chart.lanes.map_or(0, u16::from);
    let across = ui.available_rect_before_wrap().x_range();
    let indent = match direction {
        Direction::Up | Direction::Down => {
            Columns::laid_out(across, lanes, strip.is_some()).ruler.min
        }
        Direction::Left | Direction::Right => across.min,
    };
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
    let (_, allocated_area) = ui.allocate_space(ui.available_size());
    let response = ui.interact(
        allocated_area,
        ui.id().with("notefield"),
        Sense::click_and_drag(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Notefield"));
    if response.hovered() {
        let (steps, measures, zoom_delta) = ui.input(|input| {
            let [steps_left, measures_left] = scrolled;
            let steps = wheel_steps(&input.events, steps_left);
            let measures = measure_steps(&input.events, measures_left);
            (steps, measures, input.zoom_delta())
        });
        let way = |amount: i32| {
            if amount > 0 {
                Step::Later
            } else {
                Step::Earlier
            }
        };
        actions.extend(std::iter::repeat_n(
            Action::Move(way(steps)),
            usize::try_from(steps.unsigned_abs()).unwrap_or(usize::MAX),
        ));
        actions.extend(std::iter::repeat_n(
            Action::MoveMeasure(way(measures)),
            usize::try_from(measures.unsigned_abs()).unwrap_or(usize::MAX),
        ));
        if (zoom_delta - 1.0).abs() > f32::EPSILON {
            actions.push(Action::ZoomBy(zoom_delta));
        }
    }
    let flip = Flip {
        area: response.rect,
        direction,
    };
    let painter = Canvas::new(ui.painter_at(response.rect), flip);
    let area = flip.field();
    let timing = chart.timing();
    let field = Field::new(
        area,
        lanes,
        timing.time_at(song.cursor()),
        zoom,
        strip.is_some(),
    );
    let (columns, rows, view) = (&field.columns, field.rows, field.view);
    // ponytail: every visible item still resolves its time from tick 0, which is slow once a
    // timing map holds thousands of events; walk the map once per frame when that shows.
    let first = timing.latest_tick_by(view.seconds_at(rows.min - MARGIN));
    let last = timing.latest_tick_by(view.seconds_at(rows.max + MARGIN));
    let own_timing = chart.timing.is_some();
    header(
        &painter,
        columns,
        lanes,
        own_timing,
        Rangef::new(area.top(), rows.min),
        ui.visuals(),
    );
    let clipped = painter.clipped(Rect::from_x_y_ranges(area.x_range(), rows));
    backdrop(&clipped, columns, lanes, rows);
    match strip {
        Some(Strip::Waveform(peaks)) => waveform(&clipped, columns, view, rows, peaks),
        Some(Strip::Frequencies(tiles)) => {
            frequencies(&clipped, ui.ctx(), (columns, view, rows), tiles);
        }
        None => {}
    }
    grid(&clipped, columns, (song, finest), view, rows);
    markers(&clipped, columns, song, view, rows, (first, last));
    events(&clipped, columns, song, view, rows, (first, last));
    let band = Rect::from_x_y_ranges(columns.timeline(), view.now_y - 12.0..=view.now_y + 12.0);
    clipped.rect_filled(band, 0.0, style::NOW_BAND);
    for object in ordered_objects(song, (first, last)) {
        let (layer_view, colour) = song.layer_look(&object);
        if layer_view == charter_core::LayerView::Hidden {
            continue;
        }
        let selected = song.is_selected(&object);
        draw_object(
            &clipped,
            columns,
            timing,
            view,
            object,
            selected,
            (layer_view, colour, colour_notes_by_layer),
        );
    }

    actions.extend(pointer(
        ui,
        &response,
        song,
        &field,
        flip,
        (first, last),
        gesture,
    ));
    menu(ui, &response, song, (&field, flip, (first, last)), actions);
    let latest = ui.input(|input| input.pointer.latest_pos().map(|at| flip.back(at)));
    let line = |pos: Pos2| snap_line(song, view, pos);
    if let Some(Gesture::Marquee { x, seconds, .. }) = *gesture
        && let Some(latest) = latest
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
    if let Some(Gesture::Hold { lane, from }) = *gesture
        && let Some(to) = latest.and_then(&line)
        && let Some(rect) = hold_preview_rect(columns, timing, view, lane, from, to)
    {
        clipped.rect_filled(rect, 2.0, style::HOLD.gamma_multiply(0.7));
        clipped.rect_stroke(
            rect,
            2.0,
            Stroke::new(1.0, style::SELECTED),
            StrokeKind::Outside,
        );
    }
    if let Some(Gesture::MoveTiming { .. } | Gesture::MoveMarker { .. }) = *gesture
        && let Some(latest) = latest
    {
        let free =
            matches!(*gesture, Some(Gesture::MoveTiming { from }) if anchored(song.timing(), from));
        drop_guide(&clipped, columns, (song, free), view, latest.y);
    }
    if let Some(moving @ Gesture::MoveNotes { .. }) = *gesture
        && let Some(latest) = latest
        && let Some(by) = line(latest).and_then(|to| {
            let keep_time = ui.input(|input| input.modifiers.shift);
            moving.moved_by(&field, to, latest, keep_time)
        })
    {
        for object in chart
            .objects
            .iter()
            .filter(|object| song.is_selected(object))
        {
            ghost(&clipped, (columns, timing, view), object, by, (first, last));
        }
    }
    let (hovered, modifiers) = ui.input(|input| {
        let hovered = input.pointer.hover_pos().map(|at| flip.back(at));
        (hovered, input.modifiers)
    });
    if matches!(
        *gesture,
        Some(Gesture::MoveNotes { .. } | Gesture::Pan { .. })
    ) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if gesture.is_none()
        && response.hovered()
        && hovered
            .is_some_and(|at| grab(song, &field, at, (first, last), modifiers, &line).is_some())
    {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    } else {
    }
    if gesture.is_some()
        || hovered.is_some_and(|at| response.hovered() && handle_at(song, &field, at).is_some())
    {
        let horizontal = matches!(direction, Direction::Left | Direction::Right);
        let icon = if horizontal {
            egui::CursorIcon::ResizeHorizontal
        } else {
            egui::CursorIcon::ResizeVertical
        };
        if !matches!(
            *gesture,
            Some(
                Gesture::Marquee { .. }
                    | Gesture::Hold { .. }
                    | Gesture::MoveNotes { .. }
                    | Gesture::Pan { .. }
            )
        ) {
            ui.ctx().set_cursor_icon(icon);
        }
    }
    now_line(&clipped, columns, view.now_y);
    let (top, bottom) = (view.seconds_at(rows.min), view.seconds_at(rows.max));
    Some((top.min(bottom), top.max(bottom)))
}

/// Draw lower rows first, preserving note order within each row.
fn ordered_objects(song: &Song, (first, last): (Tick, Tick)) -> Vec<Valid<'_, Object>> {
    let Some(chart) = song.chart() else {
        return Vec::new();
    };
    let mut reaching = reaching(&chart.objects, first, last).peekable();
    let mut objects: Vec<_> = chart
        .objects()
        .enumerate()
        .filter_map(|(index, object)| reaching.next_if_eq(&index).map(|_| object))
        .collect();
    objects.sort_by_key(|object| std::cmp::Reverse(chart.layer_index(object).unwrap_or(0)));
    objects
}

/// The patterns the right-click menu places, by the lanes each step counts from the lane
/// clicked, and the spacings it places them at, as how many to a beat.
const PATTERNS: [(&str, &[u16]); 3] = [
    ("1-1-1-1", &[0, 0, 0, 0]),
    ("1-2-3-1-2-3", &[0, 1, 2, 0, 1, 2]),
    ("1-2-1-2", &[0, 1, 0, 1]),
];
/// Note values offered for the spacing between pattern steps.
const SPACINGS: [u16; 7] = [1, 2, 4, 8, 16, 32, 64];

/// The right-click menu. A right click selects the note under the pointer unless it is selected
/// already, and leaves the cursor alone; what the menu places goes on the snap line clicked, from
/// the lane clicked.
fn menu(
    ui: &Ui,
    response: &egui::Response,
    song: &Song,
    (field, flip, visible): (&Field, Flip, (Tick, Tick)),
    actions: &mut Vec<Action>,
) {
    let spot_id = ui.id().with("menu spot");
    if response.secondary_clicked()
        && let Some(pos) = ui.input(|input| input.pointer.latest_pos().map(|at| flip.back(at)))
    {
        let tick = snap_line(song, field.view, pos);
        if let Some(chart) = song.chart()
            && let Some(note) = hit_on_layers(song, field, chart.timing(), pos, visible)
            && !song.selection().any(|selected| selected == note)
        {
            actions.push(Action::Select(Some(note), Selecting::Replace));
        }
        let lane = field.lane_at(pos.x).unwrap_or(LaneIndex::new(0));
        let tick = tick.unwrap_or_else(|| song.cursor());
        ui.data_mut(|data| data.insert_temp(spot_id, (lane, tick)));
    }
    // Where it was clicked, since playback moves the cursor on while the menu is open.
    let spot: (LaneIndex, Tick) = ui
        .data(|data| data.get_temp(spot_id))
        .unwrap_or_else(|| (LaneIndex::new(0), song.cursor()));
    response.context_menu(|menu_ui| menu_items(menu_ui, song, spot, actions));
}

/// The right-click menu's items for `song`, with `lane` and `at` the lane and snap line clicked,
/// where what it places goes.
#[expect(clippy::too_many_lines, reason = "menu items in display order")]
fn menu_items(ui: &mut Ui, song: &Song, (lane, at): (LaneIndex, Tick), actions: &mut Vec<Action>) {
    let editable = song.read_only().is_none();
    let selected = song.selection().next().is_some();
    let (edits, chosen) = (editable, editable && selected);
    item(ui, actions, ("Cut", "Ctrl+X", chosen), Action::Cut);
    item(ui, actions, ("Copy", "Ctrl+C", selected), Action::Copy);
    item(
        ui,
        actions,
        ("Paste here", "Ctrl+V", edits),
        Action::PasteHere(at),
    );
    item(
        ui,
        actions,
        ("Duplicate", "Ctrl+D", chosen),
        Action::Duplicate,
    );
    item(
        ui,
        actions,
        ("Delete", "Delete", chosen),
        Action::DeleteSelection,
    );
    if song.layers().len() > 1 {
        ui.add_enabled_ui(chosen, |ui| {
            ui.menu_button("Move to layer", |ui| {
                for (index, row) in song.layers().iter().enumerate() {
                    let already_there = song.chart().is_some_and(|chart| {
                        chart
                            .objects()
                            .filter(|object| song.is_selected(object))
                            .all(|object| chart.layer_index(&object) == Some(index))
                    });
                    if !already_there && ui.button(&row.name).clicked() {
                        actions.push(Action::MoveSelectionToLayer(index));
                        ui.close();
                    }
                }
            });
        });
    }
    ui.separator();
    let move_left = Action::ShiftSelection {
        snaps: 0,
        lanes: -1,
    };
    let move_right = Action::ShiftSelection { snaps: 0, lanes: 1 };
    item(
        ui,
        actions,
        ("Move a lane left", "Ctrl+Left", chosen),
        move_left,
    );
    item(
        ui,
        actions,
        ("Move a lane right", "Ctrl+Right", chosen),
        move_right,
    );
    item(
        ui,
        actions,
        ("Mirror across the lanes", "Ctrl+M", chosen),
        Action::Mirror,
    );
    item(
        ui,
        actions,
        ("Snap to the grid", "Q", chosen),
        Action::Quantize,
    );
    item(
        ui,
        actions,
        ("Lengthen by the snap", "]", edits),
        Action::ResizeNotes(Step::Later),
    );
    item(
        ui,
        actions,
        ("Shorten by the snap", "[", edits),
        Action::ResizeNotes(Step::Earlier),
    );
    let changes = song
        .plugin()
        .map(charter_core::RulesetPlugin::note_changes)
        .unwrap_or_default();
    if !changes.is_empty() {
        ui.add_enabled_ui(chosen, |ui| {
            ui.menu_button("Change to", |ui| {
                for change in changes {
                    let button = ui.button(&change.name).on_hover_text(&change.about);
                    if button.clicked() {
                        actions.push(Action::ChangeNotes(change.id));
                    }
                }
            });
        });
    }
    ui.separator();
    ui.add_enabled_ui(edits, |ui| patterns(ui, (lane, at), actions));
    let lane_name = format!("Select lane {}", u16::from(lane).saturating_add(1));
    item(
        ui,
        actions,
        (&lane_name, "", true),
        Action::SelectLane(lane),
    );
    item(
        ui,
        actions,
        ("Select all", "Ctrl+A", true),
        Action::SelectAll,
    );
    item(
        ui,
        actions,
        ("Select none", "Escape", selected),
        Action::ClearSelection,
    );
    ui.separator();
    item(
        ui,
        actions,
        ("Add a timing point here", "", edits),
        Action::Here(at, Box::new(Action::AddBpm)),
    );
    item(
        ui,
        actions,
        ("Bookmark here", "Ctrl+B", edits),
        Action::Here(at, Box::new(Action::ToggleBookmark)),
    );
}

/// Where a pan the middle button grabbed at `y` with the cursor at `seconds` puts the cursor with
/// the pointer at `pos`: the chart follows the pointer, so dragging it down goes back, onto the
/// snap line there.
fn panned(
    field: &Field,
    timing: Valid<'_, Timing>,
    step: Option<Ticks>,
    (seconds, y): (f64, f32),
    pos: Pos2,
) -> Tick {
    let earlier = f64::from((pos.y - y) / field.view.zoom.points_per_second());
    match step {
        Some(step) => nearest_line(timing, seconds - earlier, step),
        None => timing.latest_tick_by(seconds - earlier),
    }
}

/// The right-click menu's patterns, each at every spacing, placed from `lane` on.
fn patterns(ui: &mut Ui, (lane, at): (LaneIndex, Tick), actions: &mut Vec<Action>) {
    ui.menu_button("Insert pattern here", |ui| {
        for (name, steps) in PATTERNS {
            ui.menu_button(name, |ui| {
                for per_beat in SPACINGS {
                    let label = if per_beat == 1 {
                        "Every beat".to_owned()
                    } else {
                        format!("Every 1/{per_beat} beat")
                    };
                    if ui.button(label).clicked() {
                        let pattern = Action::Pattern {
                            steps,
                            per_beat,
                            lane,
                        };
                        actions.push(Action::Here(at, Box::new(pattern)));
                    }
                }
            });
        }
    });
}

/// A menu item named `name`, showing `keys`, that asks for `action` when clicked.
fn item(
    ui: &mut Ui,
    actions: &mut Vec<Action>,
    (name, keys, enabled): (&str, &str, bool),
    action: Action,
) {
    let button = egui::Button::new(name);
    let button = if keys.is_empty() {
        button
    } else {
        button.shortcut_text(keys)
    };
    if ui.add_enabled(enabled, button).clicked() {
        actions.push(action);
    }
}

/// What clicks and drags over the notefield ask for. With Ctrl they place notes, a tap or under
/// Shift a mine, and a drag draws a hold; otherwise a click selects the note under the pointer,
/// Shift adding it and Alt taking it away, and a drag selects with a box.
#[expect(clippy::too_many_lines, reason = "one branch per gesture")]
fn pointer(
    ui: &Ui,
    response: &egui::Response,
    song: &Song,
    field: &Field,
    flip: Flip,
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
            pointer.press_origin().map(|at| flip.back(at)),
            pointer.latest_pos().map(|at| flip.back(at)),
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
        if !editable || !field.rows.contains(pos.y) {
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
                hit_on_layers(song, field, timing, pos, visible),
                how,
            ));
        } else if let Some((lane, tick)) = spot(pos) {
            let note = if modifiers.shift {
                Note::Mine
            } else {
                Note::Tap
            };
            actions.push(Action::Place { tick, lane, note });
        } else {
        }
    }
    let (middle, secondary) = ui.input(|input| {
        let pointer = &input.pointer;
        (
            pointer.button_down(egui::PointerButton::Middle),
            pointer.secondary_down(),
        )
    });
    if response.drag_started()
        && !escaped
        && let Some(origin) = origin
    {
        *gesture = if middle {
            let seconds = field.view.seconds_at(field.view.now_y);
            Some(Gesture::Pan {
                seconds,
                y: origin.y,
            })
        } else if secondary {
            None
        } else {
            started(song, field, (origin, modifiers), visible, &mut actions)
        };
    }
    if response.dragged()
        && let (Some(Gesture::Pan { seconds, y }), Some(pos)) = (*gesture, latest)
    {
        actions.push(Action::JumpTo(panned(
            field,
            timing,
            step,
            (seconds, y),
            pos,
        )));
    }
    if response.dragged()
        && let (Some(Gesture::Scrub { seconds, y }), Some(pos)) = (*gesture, latest)
        && let Some(step) = step
    {
        let later = f64::from((pos.y - y) / field.view.zoom.points_per_second());
        let to = nearest_line(timing, seconds + later, step);
        actions.push(Action::JumpTo(to));
    }
    if response.dragged()
        && let (Some(Gesture::Hold { lane, from }), Some(pos)) = (*gesture, latest)
        && let Some(to) = line(pos)
    {
        actions.push(Action::Stretch { lane, from, to });
    }
    if response.drag_stopped() {
        let keep_time = modifiers.shift;
        actions.extend(let_go(
            gesture.take(),
            (latest, keep_time),
            field,
            timing,
            &line,
        ));
    }
    actions
}

/// The gesture a drag from `origin` starts, and what it asks for at once: a handle's move, the
/// selection's move from a note, a hold drawn with Ctrl, or else a box.
fn started(
    song: &Song,
    field: &Field,
    (origin, modifiers): (Pos2, egui::Modifiers),
    visible: (Tick, Tick),
    actions: &mut Vec<Action>,
) -> Option<Gesture> {
    let snapped = |pos: Pos2| snap_line(song, field.view, pos);
    if let Some(handle) = handle_at(song, field, origin) {
        Some(handle)
    } else if let Some((unselected, moving)) =
        grab(song, field, origin, visible, modifiers, &snapped)
    {
        if let Some(note) = unselected {
            actions.push(Action::Select(Some(note), selecting(modifiers)));
        }
        Some(moving)
    } else if modifiers.command {
        if song.read_only().is_some() || !field.rows.contains(origin.y) {
            return None;
        }
        let (lane, from) = (field.lane_at(origin.x)?, snapped(origin)?);
        // A note already there is stretched as it is, rather than toggled away.
        if !song.starts_at(from, lane) {
            let note = Note::Tap;
            actions.push(Action::Place {
                tick: from,
                lane,
                note,
            });
        }
        Some(Gesture::Hold { lane, from })
    } else {
        Some(Gesture::Marquee {
            how: selecting(modifiers),
            x: origin.x,
            seconds: field.view.seconds_at(origin.y),
        })
    }
}

/// The gesture a plain drag, or one with Shift, from `origin` over a note starts in an editable
/// song: moving the selection when the note is in it, and otherwise the note alone, returned to
/// be selected first, or added to the selection under Shift.
fn grab(
    song: &Song,
    field: &Field,
    origin: Pos2,
    visible: (Tick, Tick),
    modifiers: egui::Modifiers,
    line: &dyn Fn(Pos2) -> Option<Tick>,
) -> Option<(Option<NoteAt>, Gesture)> {
    // Shift keeps the notes' ticks as they move.
    let plain = modifiers.is_none() || modifiers.shift_only();
    if song.read_only().is_some() || !plain || !field.rows.contains(origin.y) {
        return None;
    }
    let chart = song.chart()?;
    let note = hit_on_layers(song, field, chart.timing(), origin, visible)?;
    let (selected, room) = room(chart, song, note, field.lanes);
    let gesture = Gesture::MoveNotes {
        tick: line(origin)?,
        lane: field.lane_at(origin.x)?,
        room,
    };
    Some(((!selected).then_some(note), gesture))
}

/// Whether the timing point on `tick` is a tempo point, an anchor of the audio.
fn anchored(timing: Valid<'_, Timing>, tick: Tick) -> bool {
    timing.anchors.iter().any(|anchor| anchor.tick == tick)
}

/// What a drag asks for when it lets go at `latest`: the hold drawn to there, the handle
/// dropped on the line there, or the notes the box covers selected.
fn let_go(
    gesture: Option<Gesture>,
    (latest, keep_time): (Option<Pos2>, bool),
    field: &Field,
    timing: Valid<'_, Timing>,
    line: &dyn Fn(Pos2) -> Option<Tick>,
) -> Option<Action> {
    let (pos, gesture) = (latest?, gesture?);
    match gesture {
        Gesture::Hold { lane, from } => line(pos).map(|to| Action::Stretch { lane, from, to }),
        // A tempo point is a moment of the audio, so it goes where it is dropped; a point of
        // meters and stops alone is a beat, so it goes to the line there.
        Gesture::MoveTiming { from } if anchored(timing, from) => {
            Some(Action::MovePointTo(from, field.view.seconds_at(pos.y)))
        }
        Gesture::MoveTiming { from } => line(pos).map(|to| Action::MoveTiming { from, to }),
        Gesture::MoveMarker { index } => line(pos).map(|to| Action::MoveMarker { index, to }),
        Gesture::Marquee { how, x, seconds } => field
            .boxed(timing, (x, seconds), pos)
            .map(|(ticks, lanes)| Action::SelectBetween { ticks, lanes, how }),
        Gesture::MoveNotes { .. } => {
            let (ticks, lanes) = gesture.moved_by(field, line(pos)?, pos, keep_time)?;
            Some(Action::MoveSelection { ticks, lanes })
        }
        Gesture::Scrub { .. } | Gesture::Pan { .. } => None,
    }
}

/// How a click or a box meets the selection: Shift adds to it and Alt takes from it.
const fn selecting(modifiers: egui::Modifiers) -> Selecting {
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
#[cfg(test)]
fn hit(
    objects: &[Object],
    field: &Field,
    timing: Valid<'_, Timing>,
    pos: Pos2,
    (first, last): (Tick, Tick),
) -> Option<NoteAt> {
    hit_filtered(objects, field, timing, pos, (first, last), |_| true)
}

/// Only shown notes participate in pointer gestures or context-menu selection.
fn hit_on_layers(
    song: &Song,
    field: &Field,
    timing: Valid<'_, Timing>,
    pos: Pos2,
    visible: (Tick, Tick),
) -> Option<NoteAt> {
    let chart = song.chart()?;
    hit_filtered(&chart.objects, field, timing, pos, visible, |object| {
        song.layer_look(object).0 == charter_core::LayerView::Shown
    })
}

/// The nearest note whose layer is eligible for selection.
fn hit_filtered(
    objects: &[Object],
    field: &Field,
    timing: Valid<'_, Timing>,
    pos: Pos2,
    (first, last): (Tick, Tick),
    eligible: impl Fn(&Object) -> bool,
) -> Option<NoteAt> {
    let under = reaching(objects, first, last).filter_map(|index| {
        let object = objects.get(index)?;
        if !eligible(object) {
            return None;
        }
        let Position::Lane {
            index: lane_index,
            width,
        } = object.position
        else {
            return None;
        };
        let head = field.view.y(timing.time_at(object.tick));
        let tail = field.view.y(timing.time_at(object.shape.end(object.tick)?));
        let reach = Rangef::new(head.min(tail) - GRAB, head.max(tail) + GRAB);
        let over = field.columns.span(lane_index, width).contains(pos.x) && reach.contains(pos.y);
        let note = NoteAt {
            tick: object.tick,
            lane: lane_index,
        };
        over.then_some((note, (pos.y - head).abs()))
    });
    under
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(note, _)| note)
}

/// Says whose timing the chart uses and numbers its lanes.
fn header(
    painter: &Canvas,
    columns: &Columns,
    lanes: u16,
    own_timing: bool,
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
    caption(
        columns.timing.center(),
        if own_timing {
            "chart timing"
        } else {
            "song timing"
        },
    );
    if columns.wave.span() > 0.0 {
        caption(columns.wave.center(), "audio");
    }
    for lane in 0..lanes {
        let span = columns.span(LaneIndex::new(lane), LaneCount::ONE);
        caption(span.center(), &lane.saturating_add(1).to_string());
    }
}

/// Fills the timeline columns and draws the borders between lanes and gutters.
fn backdrop(painter: &Canvas, columns: &Columns, lanes: u16, rows: Rangef) {
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
    if columns.wave.span() > 0.0 {
        painter.rect_filled(
            Rect::from_x_y_ranges(columns.wave, rows),
            0.0,
            style::WAVE_BG,
        );
    }
    for x in [
        columns.ruler.min,
        columns.timing.min,
        columns.wave.min,
        columns.lanes.min,
        columns.lanes.max,
    ] {
        painter.vline(x, rows, Stroke::new(1.0, style::DIVIDER));
    }
}

/// How the notefield is shown: how close, which way it runs, and the waveform of the song's
/// audio when there is one to draw.
#[derive(Clone, Copy, Debug)]
pub struct Shown<'a> {
    /// Vertical scale in screen points per audio second.
    pub zoom: Zoom,
    /// Whether later notes appear above or below the now line.
    pub direction: Direction,
    /// What the strip beside the lanes shows of the song's audio, if anything.
    pub strip: Option<Strip<'a>>,
    /// The note value of the grid's finest lines: 4 draws beats alone, 16 sixteenths.
    pub grid: NonZeroU16,
    /// Tint whole notes by layer instead of marking their leading edge.
    pub colour_notes_by_layer: bool,
}

/// What the audio strip beside the lanes draws.
#[derive(Clone, Copy, Debug)]
pub enum Strip<'a> {
    /// Peak amplitude for each visible audio interval.
    Waveform(&'a Peaks),
    /// How loud each band of pitch is over time, low on the strip's left and high on its right.
    Frequencies(&'a Tiles),
}

/// Spectrum frames each tile image holds a row of, at its level of detail.
const TILE: usize = 1024;
/// The coarsest level of detail: each of its rows holds the loudest of 2^8 frames, 1.28 s.
const COARSEST: u32 = 8;

/// A song's frequencies as images the GPU stretches over the strip, a band a column and a spectrum
/// frame a row, in tiles of [`TILE`] rows. Each level of detail holds, in each row, the loudest of
/// twice as many frames as the one before, so zoomed out a short sound still shows; the GPU's
/// own shrinking would skip it. Tiles are made the first time they show.
pub struct Tiles {
    /// Frequency frames shared by tiles at every level of detail.
    spectrum: std::sync::Arc<Spectrum>,
    /// Each level's tiles, by index, once made.
    made: std::cell::RefCell<Vec<Vec<Option<egui::TextureHandle>>>>,
}

impl Tiles {
    /// Unrendered spectrum tiles, generated on demand as they come into view.
    #[must_use]
    pub const fn new(spectrum: std::sync::Arc<Spectrum>) -> Self {
        Self {
            spectrum,
            made: std::cell::RefCell::new(Vec::new()),
        }
    }

    /// Tile `index` of `level`, made now unless it was before, or none past the song's end.
    fn tile(&self, ctx: &egui::Context, level: u32, index: usize) -> Option<egui::TextureId> {
        let mut made = self.made.borrow_mut();
        let level_at = usize::try_from(level).ok()?;
        if made.len() <= level_at {
            made.resize_with(level_at.saturating_add(1), Vec::new);
        }
        let tiles = made.get_mut(level_at)?;
        if tiles.len() <= index {
            tiles.resize_with(index.saturating_add(1), || None);
        }
        let slot = tiles.get_mut(index)?;
        if slot.is_none() {
            let image = tile_image(self.spectrum.frames(), level, index)?;
            let name = format!("spectrum {level} {index}");
            *slot = Some(ctx.load_texture(name, image, egui::TextureOptions::LINEAR));
        }
        slot.as_ref().map(egui::TextureHandle::id)
    }
}

impl std::fmt::Debug for Tiles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tiles")
            .field("frames", &self.spectrum.frames().len())
            .finish_non_exhaustive()
    }
}

/// The image of tile `index` at `level`: a row for each run of 2^`level` frames, holding each
/// band's loudest over the run, or none past the last frame.
fn tile_image(
    frames: &[[u8; Spectrum::BANDS]],
    level: u32,
    index: usize,
) -> Option<egui::ColorImage> {
    let run = 1usize.checked_shl(level)?;
    let first = index.checked_mul(TILE)?.checked_mul(run)?;
    let covered = frames.get(first..)?;
    if covered.is_empty() {
        return None;
    }
    let rows: Vec<[u8; Spectrum::BANDS]> = covered
        .chunks(run)
        .take(TILE)
        .map(|chunk| {
            let mut loudest = [0; Spectrum::BANDS];
            for frame in chunk {
                for (most, amplitude) in loudest.iter_mut().zip(frame) {
                    *most = (*most).max(*amplitude);
                }
            }
            loudest
        })
        .collect();
    let pixels = rows
        .iter()
        .flatten()
        .map(|amplitude| heat(*amplitude))
        .collect();
    Some(egui::ColorImage::new([Spectrum::BANDS, rows.len()], pixels))
}

/// The seconds tile `index` of `level` spans, `rows` rows tall: from the start of its first
/// frame, each frame centred on its time, to the end of its last.
fn tile_span(level: u32, index: usize, rows: usize) -> (f64, f64) {
    let run = f64::from(1u32 << level.min(COARSEST));
    let (first, rows) = (
        crate::cast::usize_f64(index) * crate::cast::usize_f64(TILE) * run,
        crate::cast::usize_f64(rows),
    );
    let from = (first - 0.5) / Spectrum::PER_SECOND;
    (from, from + rows * run / Spectrum::PER_SECOND)
}

/// The level of detail for `pixels_per_second` of screen: the finest whose rows are no closer
/// than a pixel apart.
fn level_for(pixels_per_second: f32) -> u32 {
    let mut level = 0;
    while level < COARSEST
        && Spectrum::PER_SECOND / f64::from(1u32 << level) > f64::from(pixels_per_second)
    {
        level = level.saturating_add(1);
    }
    level
}

/// The song's frequencies in the strip: the tiles the rows show, each band a column, lowest on
/// the left.
fn frequencies(
    painter: &Canvas,
    ctx: &egui::Context,
    (columns, view, rows): (&Columns, View, Rangef),
    tiles: &Tiles,
) {
    let strip = columns.wave.shrink(2.0);
    let pixels = view.zoom.points_per_second() * ctx.pixels_per_point();
    let level = level_for(pixels);
    let (top, bottom) = (view.seconds_at(rows.min), view.seconds_at(rows.max));
    let (from, to) = (top.min(bottom), top.max(bottom));
    let seconds = crate::cast::usize_f64(TILE) * f64::from(1u32 << level) / Spectrum::PER_SECOND;
    let (first, last) = (
        crate::cast::f64_usize((from / seconds).floor().max(0.0)),
        crate::cast::f64_usize((to / seconds).floor().max(0.0)),
    );
    for index in first..=last {
        let Some(texture) = tiles.tile(ctx, level, index) else {
            break;
        };
        let rows_held = ctx
            .tex_manager()
            .read()
            .meta(texture)
            .map_or(TILE, |meta| meta.size[1]);
        let (start, end) = tile_span(level, index, rows_held);
        let rect = Rect::from_x_y_ranges(strip, view.y_of(start)..=view.y_of(end));
        painter.image(texture, rect);
    }
}

/// The colour of a band `level` loud, from silent at 0 to the track's loudest at 255: nothing
/// until it is heard, fading in through the waveform's blue, and rising to the near-white of a
/// selection at the loudest.
fn heat(level: u8) -> egui::Color32 {
    let t = f32::from(level) / 255.0;
    let (low, high) = (style::WAVE.gamma_multiply(0.35), style::SELECTED);
    let alpha = ((t - 0.06) / 0.19).clamp(0.0, 1.0);
    egui::Color32::from_rgba_unmultiplied(
        mix(low.r(), high.r(), t),
        mix(low.g(), high.g(), t),
        mix(low.b(), high.b(), t),
        mix(0, 255, alpha),
    )
}

/// A channel value between `low` and `high` at the clamped fraction `t`.
fn mix(low: u8, high: u8, t: f32) -> u8 {
    let value = f32::from(low) + (f32::from(high) - f32::from(low)) * t.clamp(0.0, 1.0);
    let value = crate::cast::f32_u8(value.round());
    value
}

/// The audio under each row of `rows`, a point apart: the row, and the lowest and highest
/// sample in the time it covers.
fn wave_rows(peaks: &Peaks, view: View, rows: Rangef) -> Vec<(f32, f32, f32)> {
    let mut drawn = Vec::new();
    let mut y = rows.min.floor();
    while y < rows.max {
        let (from, to) = (view.seconds_at(y), view.seconds_at(y + 1.0));
        let extent = peaks.between(from.min(to), from.max(to));
        drawn.push((y, extent.low, extent.high));
        y += 1.0;
    }
    drawn
}

/// The song's audio in its strip, one line a row across the extent of the audio it covers, so
/// it lines up with the notes whatever the tempo.
fn waveform(painter: &Canvas, columns: &Columns, view: View, rows: Rangef, peaks: &Peaks) {
    let (middle, half) = (columns.wave.center(), columns.wave.span() / 2.0 - 3.0);
    for (y, low, high) in wave_rows(peaks, view, rows) {
        if high - low < 0.004 {
            continue;
        }
        let across = Rangef::new(
            middle + low.clamp(-1.0, 1.0) * half,
            middle + high.clamp(-1.0, 1.0) * half,
        );
        painter.hline(across, y + 0.5, Stroke::new(1.0, style::WAVE));
    }
}

/// The grids the notefield draws, by the note value of their finest lines: beats alone, then
/// finer lines between the beats.
pub const GRIDS: [u16; 7] = [4, 8, 12, 16, 24, 32, 48];

/// A line of the grid inside a measure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Line {
    /// A line coinciding with a quarter-note beat.
    Beat,
    /// A line of a finer grid between two beats.
    Between,
}

/// The lines inside `measure`, after its start: one on every `grid`th note, each on a beat
/// marked as one, so a measure of 4/4 on a grid of 16 has sixteen places with every fourth a
/// beat. A grid the resolution does not hold in whole ticks draws the beats alone.
fn lines_in(measure: &Measure, resolution: TicksPerBeat, grid: NonZeroU16) -> Vec<(Tick, Line)> {
    let beat = resolution.note_length(QUARTER);
    let step = resolution.note_length(grid).or(beat);
    let (Some(beat), Some(step), Some(end)) =
        (beat, step, measure.start.checked_add(measure.length))
    else {
        return Vec::new();
    };
    let (beat, step) = (i64::from(u32::from(beat)), u32::from(step));
    let per_beat = usize::try_from(beat.checked_div(i64::from(step)).unwrap_or(1))
        .unwrap_or(1)
        .max(1);
    let Some(step) = Ticks::new(step) else {
        return Vec::new();
    };
    std::iter::successors(measure.start.checked_add(step), |tick| {
        tick.checked_add(step)
    })
    .take_while(|tick| *tick < end)
    .take(MOST_BEATS.saturating_mul(per_beat))
    .map(|tick| {
        let line = if i64::from(tick)
            .saturating_sub(i64::from(measure.start))
            .rem_euclid(beat)
            == 0
        {
            Line::Beat
        } else {
            Line::Between
        };
        (tick, line)
    })
    .collect()
}

/// Measure, beat and finer grid lines over the visible time, with measure numbers. On a grid
/// finer than the beats, each beat's line draws brighter so the beats still read.
fn grid(
    painter: &Canvas,
    columns: &Columns,
    (song, finest): (&Song, NonZeroU16),
    view: View,
    rows: Rangef,
) {
    let timing = song.timing();
    let resolution = song.document().document().ticks_per_beat;
    let ruler = painter.clipped(Rect::from_x_y_ranges(columns.ruler, rows));
    let first = timing.latest_tick_by(view.seconds_at(rows.min));
    let finer = finest > QUARTER;
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
        for (tick, line) in lines_in(&measure, resolution, finest) {
            let line_y = view.y(timing.time_at(tick));
            let color = match line {
                Line::Beat if finer => style::BEAT_LINE_STRONG,
                Line::Beat => style::BEAT_LINE,
                Line::Between => style::SIXTEENTH_LINE,
            };
            painter.hline(columns.timeline(), line_y, Stroke::new(1.0, color));
        }
    }
}

/// Note spacing of one quarter note, used to distinguish beat lines from finer grid lines.
const QUARTER: NonZeroU16 = NonZeroU16::new(4).unwrap();

/// Sections, bookmarks and plugin markers as lines across the time axis, named in the ruler.
fn markers(
    painter: &Canvas,
    columns: &Columns,
    song: &Song,
    view: View,
    rows: Rangef,
    (first, last): (Tick, Tick),
) {
    let timing = song.timing();
    let ruler = painter.clipped(Rect::from_x_y_ranges(columns.ruler, rows));
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
    painter: &Canvas,
    columns: &Columns,
    song: &Song,
    view: View,
    rows: Rangef,
    (first, last): (Tick, Tick),
) {
    let timing = song.timing();
    let gutter = painter.clipped(Rect::from_x_y_ranges(columns.timing, rows));
    let shown = between(&timing.events, |event| event.tick, first, last);
    // Every event at a tick is one timing point, as osu! has them: one line, its values
    // beneath it.
    for point in shown.chunk_by(|a, b| a.tick == b.tick) {
        let Some(event) = point.first() else {
            continue;
        };
        let y = view.y(timing.time_at(event.tick));
        painter.hline(columns.timing, y, Stroke::new(2.0, style::TIMING_MARK));
        for (row, detail) in (0u8..).zip(point) {
            let text = match &detail.kind {
                TimingKind::Stop(pause) => format!("stop {pause} s"),
                TimingKind::Delay(pause) => format!("delay {pause} s"),
                TimingKind::Warp(ticks) => format!("warp {ticks} ticks"),
                TimingKind::TimeSignature(meter) => meter.to_string(),
                TimingKind::Other { name, value } => format!("{name} {value}"),
            };
            let at = pos2(columns.timing.min + 6.0, y + 3.0 + 13.0 * f32::from(row));
            gutter.text(
                at,
                Align2::LEFT_TOP,
                text,
                FontId::monospace(11.0),
                style::TEXT,
            );
        }
    }
}

/// One object on lanes, in the shared vocabulary's glyphs: the ruleset draws its own kinds
/// once plugins can draw.
/// The colours a note of `kind` draws in: its head, and the body of a sustain.
pub(crate) fn kind_colours(kind: &Kind) -> (egui::Color32, egui::Color32) {
    match kind {
        Kind::Tap | Kind::Lift | Kind::Hold => (style::TAP, style::HOLD),
        Kind::Roll => (style::ROLL, style::ROLL.gamma_multiply(0.5)),
        Kind::Mine => (style::MINE, style::MINE.gamma_multiply(0.5)),
        Kind::Fake => (style::FAKE, style::FAKE.gamma_multiply(0.5)),
        Kind::Other(_) => (style::OTHER, style::OTHER.gamma_multiply(0.5)),
    }
}

/// Note-kind colours, optionally tinted by layer and dimmed for reference notes.
fn object_colours(
    kind: &Kind,
    view: charter_core::LayerView,
    colour: Option<charter_core::LayerColour>,
    tinted: bool,
) -> (egui::Color32, egui::Color32) {
    let (head, body) = kind_colours(kind);
    let (head, body) = if tinted {
        colour.map_or((head, body), |colour| {
            let colour = style::layer_colour(colour);
            (colour, colour.gamma_multiply(0.65))
        })
    } else {
        (head, body)
    };
    if view == charter_core::LayerView::Ghosted {
        (head.gamma_multiply(0.35), body.gamma_multiply(0.35))
    } else {
        (head, body)
    }
}

/// Draws a lane object's head and any sustained body, highlighting selected heads.
fn draw_object(
    painter: &Canvas,
    columns: &Columns,
    timing: Valid<'_, Timing>,
    view: View,
    object: Valid<'_, Object>,
    selected: bool,
    (layer_view, colour, tinted): (
        charter_core::LayerView,
        Option<charter_core::LayerColour>,
        bool,
    ),
) {
    let Position::Lane { index, width } = object.position else {
        return;
    };
    let start = view.y(timing.time_at(object.tick));
    let end = view.y(timing.time_at(object.end()));
    let across = columns
        .span(index, width)
        .shrink((columns.lane_w * 0.12).max(2.0));
    let (head, body) = object_colours(&object.kind, layer_view, colour, tinted);
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
                if let Position::Lane {
                    index: point_lane,
                    width: point_width,
                } = point.position
                {
                    line.push(pos2(
                        columns.span(point_lane, point_width).center(),
                        view.y(timing.time_at(point.tick)),
                    ));
                }
            }
            painter.line(line, Stroke::new(4.0, body));
        }
    }
    let rect = Rect::from_x_y_ranges(across, start - 5.0..=start + 5.0);
    if selected {
        painter.rect_stroke(
            rect.expand(3.0),
            5.0,
            Stroke::new(1.5, style::SELECTED),
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
        Kind::Tap | Kind::Hold | Kind::Roll | Kind::Fake | Kind::Other(_) => {
            painter.rect_filled(rect, 3.0, head);
        }
    }
    if let Some(colour) = colour
        && !tinted
    {
        let colour = style::layer_colour(colour);
        painter.left_stripe(
            rect,
            if layer_view == charter_core::LayerView::Ghosted {
                colour.gamma_multiply(0.35)
            } else {
                colour
            },
        );
    }
    for child in object.children() {
        if let Position::Lane {
            index: child_lane,
            width: child_width,
        } = child.position
        {
            let centre = pos2(
                columns.span(child_lane, child_width).center(),
                view.y(timing.time_at(child.tick)),
            );
            painter.circle_filled(centre, 3.0, style::TEXT);
        }
    }
}

/// The body a hold would have if released on `to`, without covering its existing head.
fn hold_preview_rect(
    columns: &Columns,
    timing: Valid<'_, Timing>,
    view: View,
    lane: LaneIndex,
    from: Tick,
    to: Tick,
) -> Option<Rect> {
    if from == to {
        return None;
    }
    let across = columns
        .span(lane, LaneCount::ONE)
        .shrink((columns.lane_w * 0.12).max(2.0));
    Some(Rect::from_two_pos(
        pos2(
            across.min + across.span() * 0.25,
            view.y(timing.time_at(from)),
        ),
        pos2(
            across.max - across.span() * 0.25,
            view.y(timing.time_at(to)),
        ),
    ))
}

/// The snap line nearest `pos` on screen.
fn snap_line(song: &Song, view: View, pos: Pos2) -> Option<Tick> {
    let step = song
        .document()
        .document()
        .ticks_per_beat
        .note_length(song.snap())?;
    Some(nearest_line(song.timing(), view.seconds_at(pos.y), step))
}

/// Where a dragged note will land, `by` ticks later and lanes across: its head outlined, and
/// its tail when it lasts, when that shows between the ticks `first` and `last`.
fn ghost(
    painter: &Canvas,
    (columns, timing, view): (&Columns, Valid<'_, Timing>, View),
    object: &Object,
    (ticks, lanes): (i32, i32),
    (first, last): (Tick, Tick),
) {
    let Position::Lane { index, width } = object.position else {
        return;
    };
    let Ok(index) = u16::try_from(i32::from(u16::from(index)).saturating_add(lanes)) else {
        return;
    };
    let shift = |tick: Tick| {
        i32::try_from(i64::from(tick).saturating_add(i64::from(ticks)))
            .ok()
            .map(Tick::new)
    };
    let end = object.shape.end(object.tick).and_then(shift);
    let (Some(start), Some(end)) = (shift(object.tick), end) else {
        return;
    };
    if start > last || end < first {
        return;
    }
    let across = columns
        .span(LaneIndex::new(index), width)
        .shrink((columns.lane_w * 0.12).max(2.0));
    let (head, tail) = (view.y(timing.time_at(start)), view.y(timing.time_at(end)));
    if end > start {
        let body = Rect::from_two_pos(
            pos2(across.min + across.span() * 0.25, head),
            pos2(across.max - across.span() * 0.25, tail),
        );
        painter.rect_filled(body, 2.0, style::SELECTED.gamma_multiply(0.35));
    }
    let rect = Rect::from_x_y_ranges(across, head - 5.0..=head + 5.0);
    painter.rect_filled(rect, 3.0, style::SELECTED.gamma_multiply(0.35));
    painter.rect_stroke(
        rect,
        3.0,
        Stroke::new(1.5, style::SELECTED),
        StrokeKind::Inside,
    );
}

/// Where a dragged handle will land, the snap line nearest `y`: a line across the time axis in
/// the accent, with its position in the ruler.
fn drop_guide(
    painter: &Canvas,
    columns: &Columns,
    (song, free): (&Song, bool),
    view: View,
    y: f32,
) {
    let timing = song.timing();
    let (at, label) = if free {
        (y, style::clock(view.seconds_at(y)))
    } else {
        let Some(step) = song
            .document()
            .document()
            .ticks_per_beat
            .note_length(song.snap())
        else {
            return;
        };
        let tick = nearest_line(timing, view.seconds_at(y), step);
        (
            view.y(timing.time_at(tick)),
            timing.grid_position(tick).to_string(),
        )
    };
    painter.hline(columns.timeline(), at, Stroke::new(1.5, style::ACCENT));
    painter.text(
        pos2(columns.ruler.min + 6.0, at - 3.0),
        Align2::LEFT_BOTTOM,
        label,
        FontId::monospace(11.0),
        style::ACCENT,
    );
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
#[expect(
    clippy::arithmetic_side_effects,
    reason = "egui positions are offset by vectors"
)]
fn now_line(painter: &Canvas, columns: &Columns, now_y: f32) {
    painter.hline(columns.timeline(), now_y, Stroke::new(2.0, style::NOW_LINE));
    let tip = pos2(columns.ruler.max, now_y);
    let handle = vec![tip + vec2(-11.0, -6.0), tip, tip + vec2(-11.0, 6.0)];
    painter.convex_polygon(handle, style::NOW_LINE, Stroke::NONE);
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
        // Ctrl zooms, and Shift moves by a measure.
        if modifiers.command || modifiers.ctrl || modifiers.shift {
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
    let steps = crate::cast::f32_i32(steps);
    steps
}

/// How many measures the wheel turned under Shift this frame, later positive: a notch a measure,
/// with what a touchpad turned short of a whole one kept in `remainder` for the next frame.
/// Shift turns some wheels sideways, so either way counts.
fn measure_steps(events: &[egui::Event], remainder: &mut f32) -> i32 {
    let notches: f32 = events
        .iter()
        .filter_map(|event| match event {
            egui::Event::MouseWheel {
                unit,
                delta,
                modifiers,
                ..
            } if modifiers.shift && !modifiers.command && !modifiers.ctrl => {
                let along = if delta.y == 0.0 { delta.x } else { delta.y };
                Some(match unit {
                    egui::MouseWheelUnit::Point => -along / POINTS_PER_STEP,
                    egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page => -along,
                })
            }
            egui::Event::Copy
            | egui::Event::Cut
            | egui::Event::Paste(_)
            | egui::Event::Text(_)
            | egui::Event::Key { .. }
            | egui::Event::ModifiersChanged(_)
            | egui::Event::PointerMoved(_)
            | egui::Event::MouseMoved(_)
            | egui::Event::PointerButton { .. }
            | egui::Event::PointerGone
            | egui::Event::Zoom(_)
            | egui::Event::Rotate(_)
            | egui::Event::Ime(_)
            | egui::Event::Touch { .. }
            | egui::Event::MouseWheel { .. }
            | egui::Event::WindowFocused(_)
            | egui::Event::AccessKitActionRequest(_)
            | egui::Event::Screenshot { .. } => None,
        })
        .sum();
    *remainder += notches;
    let whole = remainder.trunc();
    *remainder -= whole;
    let whole = crate::cast::f32_i32(whole);
    whole
}

/// Points of touchpad scrolling that make one cursor step.
const POINTS_PER_STEP: f32 = 40.0;

#[cfg(test)]
mod tests {
    use charter_core::RulesetPlugin;
    use egui::{Modifiers, MouseWheelUnit, TouchPhase, Vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};

    use super::*;
    use charter_audio::Track;
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

    fn common_measure() -> Measure {
        Measure {
            number: 0,
            start: Tick::new(0),
            length: Ticks::new(3840).unwrap(),
            meter: charter_core::TimeSignature::default(),
        }
    }

    fn grid_of(n: u16) -> NonZeroU16 {
        NonZeroU16::new(n).unwrap()
    }

    #[test]
    fn a_measure_of_four_four_has_three_beat_lines_inside_it() {
        let resolution = charter_core::TicksPerBeat::new(960).unwrap();
        let lines = lines_in(&common_measure(), resolution, grid_of(4));
        let beats: Vec<_> = (1..4)
            .map(|beat| (Tick::new(beat * 960), Line::Beat))
            .collect();
        assert_eq!(lines, beats);
    }

    /// The lines of a 4/4 measure at 960 ticks a beat on a grid of `grid`: each `step` ticks
    /// apart, every `per_beat`th a beat.
    fn assert_fills(grid: u16, step: i32, per_beat: i32) {
        let resolution = charter_core::TicksPerBeat::new(960).unwrap();
        let lines = lines_in(&common_measure(), resolution, grid_of(grid));
        assert_eq!(
            lines.len(),
            usize::try_from(4_i32.saturating_mul(per_beat).saturating_sub(1)).unwrap()
        );
        for (place, (tick, line)) in (1_i32..).zip(&lines) {
            assert_eq!(*tick, Tick::new(place.saturating_mul(step)));
            let expected = if place.rem_euclid(per_beat) == 0 {
                Line::Beat
            } else {
                Line::Between
            };
            assert_eq!(*line, expected, "grid {grid}, place {place}");
        }
    }

    #[test]
    fn sixteenth_lines_fill_a_four_four_measure_to_sixteen_places_every_fourth_a_beat() {
        assert_fills(16, 240, 4);
    }

    #[test]
    fn finer_grids_put_their_lines_between_the_beats_each_beat_still_marked() {
        assert_fills(8, 480, 2);
        assert_fills(12, 320, 3);
        assert_fills(24, 160, 6);
        assert_fills(32, 120, 8);
        assert_fills(48, 80, 12);
    }

    #[test]
    fn a_grid_finer_than_the_resolution_holds_draws_the_beats_alone() {
        let resolution = charter_core::TicksPerBeat::new(4).unwrap();
        let lines = lines_in(&common_measure(), resolution, grid_of(48));
        assert!(lines.iter().all(|(_, line)| *line == Line::Beat));
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
    fn a_hidden_layer_is_not_hit_even_where_its_note_was_drawn() {
        let mut song = Song::new(demo::song(), &[&charter_stepmania::Plugin]);
        song.set_layer_view(0, charter_core::LayerView::Hidden)
            .unwrap();
        let field = field(&song);
        let chart = song.chart().unwrap();
        let note = chart
            .objects()
            .find(|o| matches!(o.position, Position::Lane { .. }))
            .unwrap();
        let Position::Lane { index, width } = note.position else {
            panic!("the chosen note has a lane")
        };
        let at = pos2(
            field.columns.span(index, width).center(),
            field.view.y(chart.timing().time_at(note.tick)),
        );
        assert_eq!(
            hit_on_layers(
                &song,
                &field,
                chart.timing(),
                at,
                (Tick::new(0), Tick::new(3840))
            ),
            None
        );
    }

    #[test]
    fn ghosted_notes_dim_but_shown_notes_keep_their_kind_colour() {
        let (shown, _) = object_colours(&Kind::Tap, charter_core::LayerView::Shown, None, false);
        let (ghost, _) = object_colours(&Kind::Tap, charter_core::LayerView::Ghosted, None, false);
        assert_eq!(shown, style::TAP);
        assert!(ghost.r() < shown.r());
        assert!(ghost.g() < shown.g());
        let (tinted, _) = object_colours(
            &Kind::Tap,
            charter_core::LayerView::Shown,
            Some(charter_core::LayerColour::Pink),
            true,
        );
        assert_eq!(tinted, style::layer_colour(charter_core::LayerColour::Pink));
    }

    #[test]
    fn layer_stripe_fills_the_left_edge_of_the_head_in_every_scroll_direction() {
        let mut song = Song::new(demo::song(), &[&charter_stepmania::Plugin]);
        song.add_layer().unwrap();
        let field = field(&song);
        let chart = song.chart().unwrap();
        let object = chart
            .objects()
            .find(|note| matches!(note.position, Position::Lane { .. }))
            .unwrap();
        for direction in Direction::ALL {
            let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 800.0));
            let ctx = egui::Context::default();
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                let canvas = Canvas::new(ui.painter_at(area), Flip { area, direction });
                draw_object(
                    &canvas,
                    &field.columns,
                    chart.timing(),
                    field.view,
                    object,
                    false,
                    (
                        charter_core::LayerView::Shown,
                        Some(charter_core::LayerColour::Blue),
                        false,
                    ),
                );
            });
            let stripes: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|shape| {
                    if let egui::Shape::Rect(rect) = &shape.shape
                        && rect.fill == style::layer_colour(charter_core::LayerColour::Blue)
                    {
                        Some(rect.rect)
                    } else {
                        None
                    }
                })
                .collect();
            output.textures_delta.clear();
            assert!(
                stripes.iter().any(|stripe| {
                    (stripe.width() - 4.0).abs() < 0.01
                        && stripes.iter().any(|head| {
                            (head.height() - stripe.height()).abs() < 0.01
                                && (head.left() - stripe.left()).abs() < 0.01
                                && head.width() > stripe.width()
                        })
                }),
                "{direction:?}: {stripes:?}"
            );
        }
    }

    #[test]
    fn hidden_layer_notes_disappear_from_the_rendered_notefield() {
        let mut song = Song::new(demo::song(), &[&charter_stepmania::Plugin]);
        let painted_taps = |drawing_song: &Song| {
            let ctx = egui::Context::default();
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                show(
                    ui,
                    drawing_song,
                    Shown {
                        zoom: Zoom::default(),
                        direction: Direction::Up,
                        strip: None,
                        grid: grid_of(16),
                        colour_notes_by_layer: false,
                    },
                    &mut [0.0; 2],
                    &mut None,
                    &mut Vec::new(),
                );
            });
            let count = output.shapes.iter().filter(|shape| {
                matches!(&shape.shape, egui::Shape::Rect(rect) if rect.fill == style::TAP)
            }).count();
            output.textures_delta.clear();
            count
        };
        assert!(painted_taps(&song) > 0);
        song.set_layer_view(0, charter_core::LayerView::Hidden)
            .unwrap();
        assert_eq!(painted_taps(&song), 0);
    }

    #[test]
    fn note_menu_moves_selection_to_another_layer() {
        let mut song = Song::new(demo::song(), &[&charter_stepmania::Plugin]);
        song.add_layer().unwrap();
        let first = song
            .chart()
            .unwrap()
            .objects()
            .find(|o| matches!(o.position, Position::Lane { .. }))
            .unwrap();
        let Position::Lane { index, .. } = first.position else {
            panic!("the chosen note has a lane")
        };
        song.select(
            [NoteAt {
                tick: first.tick,
                lane: index,
            }],
            Selecting::Replace,
        );
        let mut harness = Harness::builder().build_ui_state(
            |ui, (song, actions): &mut (Song, Vec<Action>)| {
                menu_items(ui, song, (LaneIndex::new(0), Tick::new(0)), actions);
            },
            (song, Vec::new()),
        );
        harness.run();
        harness.get_by_label("Move to layer").click();
        harness.run();
        harness.get_by_label("Layer 1").click();
        harness.run();
        assert!(harness.state().1.contains(&Action::MoveSelectionToLayer(0)));
    }

    #[test]
    fn lower_layers_are_painted_before_higher_ones() {
        let mut song = Song::new(demo::song(), &[&charter_stepmania::Plugin]);
        song.add_layer().unwrap();
        song.set_active_layer(0);
        song.toggle(Tick::new(1_000_000), LaneIndex::new(0), Kind::Tap)
            .unwrap();
        let order = ordered_objects(&song, (Tick::new(0), Tick::new(1_000_000)));
        let chart = song.chart().unwrap();
        let layers: Vec<_> = order
            .iter()
            .map(|object| chart.layer_index(object).unwrap())
            .collect();
        assert_eq!(layers.first(), Some(&1));
        assert_eq!(layers.last(), Some(&0));
    }

    #[test]
    fn shift_scrolling_a_touchpad_adds_up_to_whole_measures() {
        let nudge = egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -10.0),
            modifiers: egui::Modifiers::SHIFT,
            phase: egui::TouchPhase::Move,
        };
        let mut remainder = 0.0;
        let steps: Vec<i32> =
            std::iter::repeat_with(|| measure_steps(std::slice::from_ref(&nudge), &mut remainder))
                .take(4)
                .collect();
        assert_eq!(steps, [0, 0, 0, 1]);
    }

    #[test]
    fn zoom_grows_and_shrinks_by_a_quarter_within_bounds() {
        let start = Zoom::default();
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
        let zoom = Zoom::default();
        assert!((zoom.percent() - 100.0).abs() < 1e-3);
        assert!((zoom.by(2.0).percent() - 200.0).abs() < 1e-3);
        assert!((zoom.by(2.0).by(1e6).points_per_second() - 3000.0).abs() < 1e-3);
    }

    #[test]
    fn later_audio_sits_lower_by_the_zoom_per_second() {
        let song = song();
        let timing = song.timing();
        let view = View::new(100.0, timing.time_at(Tick::new(0)), Zoom::default());
        let beat_later = view.y(timing.time_at(Tick::new(960)));
        let per_second = Zoom::default().points_per_second();
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
        let view = View::new(100.0, timing.time_at(Tick::new(960)), Zoom::default());
        let y = view.y(timing.time_at(Tick::new(1920)));
        assert!((view.seconds_at(y) - 0.8).abs() < 1e-6);
    }

    #[test]
    fn a_hold_preview_spans_the_beats_it_will_cover_in_either_direction() {
        let song = song();
        let timing = song.timing();
        let view = View::new(100.0, timing.time_at(Tick::new(0)), Zoom::default());
        let columns = Columns::laid_out(Rangef::new(0.0, 600.0), 4, false);
        let later = hold_preview_rect(
            &columns,
            timing,
            view,
            LaneIndex::new(0),
            Tick::new(0),
            Tick::new(960),
        )
        .unwrap();
        assert!((later.top() - 100.0).abs() < 0.01);
        assert!((later.bottom() - 196.0).abs() < 0.01);
        assert!(later.left() >= columns.lanes.min);
        let earlier = hold_preview_rect(
            &columns,
            timing,
            view,
            LaneIndex::new(0),
            Tick::new(960),
            Tick::new(0),
        )
        .unwrap();
        assert_eq!(later, earlier);
        assert!(
            hold_preview_rect(
                &columns,
                timing,
                view,
                LaneIndex::new(0),
                Tick::new(0),
                Tick::new(0),
            )
            .is_none()
        );
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
    fn the_waveform_gets_a_strip_between_the_timing_and_the_lanes() {
        let across = Rangef::new(0.0, 1000.0);
        let with = Columns::laid_out(across, 4, true);
        assert!((with.wave.min - with.timing.max).abs() < f32::EPSILON);
        assert!((with.lanes.min - with.wave.max).abs() < f32::EPSILON);
        assert!(with.wave.span() > 40.0);
        let without = Columns::laid_out(across, 4, false);
        assert!(without.wave.span() < f32::EPSILON);
        assert!((without.lanes.min - without.timing.max).abs() < f32::EPSILON);
    }

    #[test]
    fn a_tile_holds_a_row_a_frame_and_coarser_levels_keep_each_run_s_loudest() {
        let mut frames = vec![[0u8; Spectrum::BANDS]; TILE + 10];
        // A short sound in one frame and one band.
        frames[3][5] = 255;
        let image = tile_image(&frames, 0, 0).unwrap();
        assert_eq!(image.size, [Spectrum::BANDS, TILE]);
        assert_eq!(image.pixels[3 * Spectrum::BANDS + 5], heat(255));
        assert_eq!(
            tile_image(&frames, 0, 1).unwrap().size,
            [Spectrum::BANDS, 10]
        );
        assert!(tile_image(&frames, 0, 2).is_none());
        let coarse = tile_image(&frames, 2, 0).unwrap();
        assert_eq!(coarse.size[1], (TILE + 10).div_ceil(4));
        assert_eq!(coarse.pixels[5], heat(255), "the loudest of frames 0 to 3");
    }

    #[test]
    fn tiles_line_up_with_the_frames_times_and_the_zoom_picks_their_detail() {
        let (from, to) = tile_span(0, 1, TILE);
        let expected = (crate::cast::usize_f64(TILE) - 0.5) / Spectrum::PER_SECOND;
        assert!((from - expected).abs() < 1e-12);
        let long = crate::cast::usize_f64(TILE) / Spectrum::PER_SECOND;
        assert!((to - from - long).abs() < 1e-12);
        assert_eq!(level_for(400.0), 0);
        assert_eq!(level_for(200.0), 0);
        assert_eq!(level_for(150.0), 1);
        assert_eq!(level_for(30.0), 3);
        assert_eq!(level_for(0.1), COARSEST);
    }

    #[test]
    fn silence_is_clear_and_the_loudest_is_bright_and_solid() {
        assert_eq!(heat(0).a(), 0);
        assert_eq!(heat(255).a(), 255);
        assert!(heat(255).r() > heat(128).r());
    }

    #[test]
    fn each_row_of_the_waveform_shows_the_audio_it_covers() {
        // A second of silence, then a second at half volume, at 1000 frames a second.
        let mut frames = vec![[0.0, 0.0]; 1000];
        frames.extend((0..1000).map(|frame| {
            if frame % 2 == 0 {
                [0.5, 0.5]
            } else {
                [-0.5, -0.5]
            }
        }));
        let peaks = Peaks::of(&Track::from_frames(1000, frames));
        let now = song().timing().time_at(Tick::new(0));
        let view = View::new(0.0, now, Zoom::default());
        let rows = wave_rows(&peaks, view, Rangef::new(0.0, view.y_of(2.0)));
        let at = |seconds: f64| {
            let y = view.y_of(seconds);
            rows.iter()
                .find(|row| (row.0 - y).abs() < 1.0)
                .map(|row| (row.1, row.2))
                .unwrap()
        };
        assert_eq!(at(0.5), (0.0, 0.0));
        let loud = at(1.5);
        assert!(
            (loud.0 + 0.5).abs() < 1e-6 && (loud.1 - 0.5).abs() < 1e-6,
            "{loud:?}"
        );
    }

    #[test]
    fn a_box_over_a_chart_without_lanes_reaches_no_lane() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
        let now = song().timing().time_at(Tick::new(0));
        let field = Field::new(area, 0, now, Zoom::default(), false);
        assert_eq!(field.lanes_between(0.0, 800.0), None);
    }

    #[test]
    fn a_box_past_either_side_of_the_lanes_reaches_the_lane_on_that_side() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
        let now = song().timing().time_at(Tick::new(0));
        let field = Field::new(area, 4, now, Zoom::default(), false);
        let lanes = field.columns.lanes;
        let (first, last) = (LaneIndex::new(0), LaneIndex::new(3));
        assert_eq!(field.lanes_between(0.0, 800.0), Some((first, last)));
        assert_eq!(
            field.lanes_between(lanes.max, lanes.max + 5.0),
            Some((last, last))
        );
        assert_eq!(field.lanes_between(0.0, lanes.min - 1.0), None);
        assert_eq!(field.lanes_between(lanes.max + 1.0, 800.0), None);
    }

    #[test]
    fn the_latest_tick_by_a_time_waits_out_a_stop() {
        let song = song();
        let timing = song.timing();
        assert_eq!(timing.latest_tick_by(0.5), Tick::new(1200));
        let stop = Tick::new(30720);
        let paused = timing.time_at(stop).seconds() + 0.25;
        assert_eq!(timing.latest_tick_by(paused), stop);
    }

    fn field(song: &Song) -> Field {
        Field::new(
            Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0)),
            4,
            song.timing().time_at(song.cursor()),
            Zoom::default(),
            false,
        )
    }

    #[test]
    fn events_at_one_tick_draw_one_timing_point_with_both_details() {
        let mut document = serde_json::to_value(demo::song().document()).unwrap();
        let timing_events = document["timing"]["events"].as_array_mut().unwrap();
        timing_events.insert(0, serde_json::json!({"tick": 0, "kind": {"stop": 0.25}}));
        timing_events.insert(1, serde_json::json!({"tick": 0, "kind": {"delay": 0.125}}));
        let document =
            charter_core::ValidDocument::from_json(&serde_json::to_vec(&document).unwrap())
                .unwrap();
        let song = Song::new(document, &[]);
        let field = field(&song);
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
            let canvas = Canvas::new(
                ui.painter_at(area),
                Flip {
                    area,
                    direction: Direction::Up,
                },
            );
            events(
                &canvas,
                &field.columns,
                &song,
                field.view,
                Rangef::new(0.0, 600.0),
                (Tick::new(0), Tick::new(960)),
            );
        });
        let labels: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| {
                let egui::Shape::Text(text) = &shape.shape else {
                    return None;
                };
                Some(text.galley.text().to_owned())
            })
            .collect();
        output.textures_delta.clear();
        assert!(
            labels.iter().any(|label| label == "stop 0.25 s"),
            "{labels:?}"
        );
        assert!(
            labels.iter().any(|label| label == "delay 0.125 s"),
            "{labels:?}"
        );
    }

    #[test]
    fn a_read_only_song_allows_scrubbing_but_not_dragging_timing_or_markers() {
        let song = song();
        let field = field(&song);
        let now = field.view.now_y;
        let handle = pos2(field.columns.handle_x(), now);
        assert_eq!(
            handle_at(&song, &field, handle),
            Some(Gesture::Scrub {
                seconds: 0.0,
                y: now
            })
        );
        assert_eq!(
            handle_at(&song, &field, pos2(field.columns.timing_x(), now)),
            None
        );
        assert_eq!(
            handle_at(&song, &field, pos2(field.columns.ruler_x(), now)),
            None
        );
        let mut actions = Vec::new();
        let origin = pos2(field.columns.lanes.center(), field.rows.min - 2.0);
        assert_eq!(
            started(
                &song,
                &field,
                (origin, Modifiers::COMMAND),
                (Tick::new(0), Tick::new(3840)),
                &mut actions,
            ),
            None
        );
        assert!(actions.is_empty());
    }

    #[test]
    fn a_drag_above_the_rows_cannot_place_a_hold_but_can_box_the_lanes() {
        let song = Song::new(demo::song(), &[&charter_stepmania::Plugin]);
        let field = field(&song);
        let origin = pos2(field.columns.lanes.center(), field.rows.min - 3.0);
        let mut actions = Vec::new();
        assert_eq!(
            started(
                &song,
                &field,
                (origin, Modifiers::COMMAND),
                (Tick::new(0), Tick::new(3840)),
                &mut actions,
            ),
            None
        );
        assert!(actions.is_empty());
        let box_drag = started(
            &song,
            &field,
            (origin, Modifiers::NONE),
            (Tick::new(0), Tick::new(3840)),
            &mut actions,
        );
        assert!(matches!(box_drag, Some(Gesture::Marquee { .. })));
        let line = |at: Pos2| snap_line(&song, field.view, at);
        let dropped = let_go(
            box_drag,
            (
                Some(pos2(field.columns.lanes.max, field.view.now_y + 96.0)),
                false,
            ),
            &field,
            song.timing(),
            &line,
        );
        assert!(matches!(dropped, Some(Action::SelectBetween { .. })));
    }

    #[test]
    fn panning_without_a_snap_uses_the_tick_at_the_audio_time() {
        let song = song();
        let field = field(&song);
        let y = field.view.now_y;
        let pos = pos2(field.columns.lanes.center(), y - 48.0);
        assert_eq!(
            panned(&field, song.timing(), None, (0.0, y), pos),
            Tick::new(480)
        );
        assert_eq!(
            panned(
                &field,
                song.timing(),
                Ticks::new(240),
                (0.0, y),
                pos2(pos.x, y - 51.0)
            ),
            Tick::new(480)
        );
    }

    #[test]
    fn moved_notes_keep_their_time_under_shift_and_stop_at_the_last_lane() {
        let song = song();
        let field = field(&song);
        let moving = Gesture::MoveNotes {
            tick: Tick::new(0),
            lane: LaneIndex::new(1),
            room: (-1, 1),
        };
        let far_right = pos2(field.columns.lanes.max + 50.0, field.view.now_y + 100.0);
        assert_eq!(
            moving.moved_by(&field, Tick::new(960), far_right, true),
            Some((0, 1))
        );
        assert_eq!(
            moving.moved_by(&field, Tick::new(960), far_right, false),
            Some((960, 1))
        );
        assert_eq!(
            moving.moved_by(
                &field,
                Tick::new(0),
                pos2(field.columns.lanes.min - 50.0, far_right.y),
                false
            ),
            Some((0, -1))
        );
    }

    #[test]
    fn a_hold_is_hit_through_its_body_but_not_beyond_its_tail() {
        let song = song();
        let field = field(&song);
        let chart = song.chart().unwrap();
        let timing = chart.timing();
        let x = field
            .columns
            .span(LaneIndex::new(3), LaneCount::ONE)
            .center();
        let at = |tick| pos2(x, field.view.y(timing.time_at(Tick::new(tick))));
        assert_eq!(
            hit(
                &chart.objects,
                &field,
                timing,
                at(960),
                (Tick::new(0), Tick::new(3840)),
            ),
            Some(NoteAt {
                tick: Tick::new(0),
                lane: LaneIndex::new(3),
            })
        );
        assert_eq!(
            hit(
                &chart.objects,
                &field,
                timing,
                at(2400),
                (Tick::new(0), Tick::new(3840)),
            ),
            None
        );
    }

    #[test]
    fn an_anchor_drops_at_audio_time_but_a_meter_and_marker_drop_on_grid_lines() {
        let song = song();
        let field = field(&song);
        let pos = pos2(field.columns.timing_x(), field.view.now_y + 98.0);
        let line = |at: Pos2| snap_line(&song, field.view, at);
        let anchor = let_go(
            Some(Gesture::MoveTiming { from: Tick::new(0) }),
            (Some(pos), false),
            &field,
            song.timing(),
            &line,
        );
        assert!(matches!(
            anchor,
            Some(Action::MovePointTo(from, seconds))
                if from == Tick::new(0) && (seconds - 98.0 / 240.0).abs() < 1e-6
        ));
        let meter = let_go(
            Some(Gesture::MoveTiming {
                from: Tick::new(92160),
            }),
            (Some(pos), false),
            &field,
            song.timing(),
            &line,
        );
        assert!(
            matches!(
                meter,
                Some(Action::MoveTiming { from, to })
                    if from == Tick::new(92160) && to == Tick::new(960)
            ),
            "{meter:?}"
        );
        assert!(matches!(
            let_go(
                Some(Gesture::MoveMarker { index: 1 }),
                (Some(pos), false),
                &field,
                song.timing(),
                &line
            ),
            Some(Action::MoveMarker { index: 1, to }) if to == Tick::new(960)
        ));
    }

    #[test]
    fn sideways_shift_wheel_scrolls_measures_without_consuming_plain_steps() {
        let sideways = egui::Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: vec2(-20.0, 0.0),
            modifiers: Modifiers::SHIFT,
            phase: TouchPhase::Move,
        };
        let mut measures = 0.0;
        let mut steps = 0.0;
        assert_eq!(
            measure_steps(std::slice::from_ref(&sideways), &mut measures),
            0
        );
        assert_eq!(
            measure_steps(std::slice::from_ref(&sideways), &mut measures),
            1
        );
        assert_eq!(wheel_steps(&[sideways], &mut steps), 0);
        assert_eq!(
            wheel_steps(
                &[wheel(MouseWheelUnit::Line, -1.0, Modifiers::NONE)],
                &mut steps
            ),
            -1
        );
        assert_eq!(
            measure_steps(
                &[wheel(MouseWheelUnit::Line, 1.0, Modifiers::COMMAND)],
                &mut measures
            ),
            0
        );
    }

    #[test]
    fn a_moved_note_ghost_is_drawn_at_its_destination_only_when_visible() {
        let song = song();
        let field = field(&song);
        let chart = song.chart().unwrap();
        let object = chart
            .objects
            .iter()
            .find(|object| object.tick == Tick::new(0)
                && matches!(object.position, Position::Lane { index, .. } if index == LaneIndex::new(3)))
            .unwrap();
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let canvas = Canvas::new(
                ui.painter_at(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))),
                Flip {
                    area: Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0)),
                    direction: Direction::Up,
                },
            );
            ghost(
                &canvas,
                (&field.columns, chart.timing(), field.view),
                object,
                (960, -1),
                (Tick::new(0), Tick::new(3840)),
            );
            ghost(
                &canvas,
                (&field.columns, chart.timing(), field.view),
                object,
                (960, -1),
                (Tick::new(10000), Tick::new(11000)),
            );
        });
        let painted: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Rect(rect) if rect.fill == style::SELECTED.gamma_multiply(0.35) => {
                    Some(rect.rect)
                }
                egui::Shape::Noop
                | egui::Shape::Vec(_)
                | egui::Shape::Circle(_)
                | egui::Shape::Ellipse(_)
                | egui::Shape::LineSegment { .. }
                | egui::Shape::Path(_)
                | egui::Shape::Rect(_)
                | egui::Shape::Text(_)
                | egui::Shape::Mesh(_)
                | egui::Shape::QuadraticBezier(_)
                | egui::Shape::CubicBezier(_)
                | egui::Shape::Callback(_) => None,
            })
            .collect();
        output.textures_delta.clear();
        assert_eq!(painted.len(), 2, "one visible hold has a head and a body");
        let head = painted.iter().find(|rect| rect.height() <= 10.0).unwrap();
        let lane = field.columns.span(LaneIndex::new(2), LaneCount::ONE);
        assert!((head.center().x - lane.center()).abs() < 0.01);
        assert!(
            (head.center().y - field.view.y(chart.timing().time_at(Tick::new(960)))).abs() < 0.01
        );
    }

    #[test]
    fn a_drop_guide_snaps_markers_but_tracks_an_anchor_at_its_audio_moment() {
        let song = song();
        let field = field(&song);
        let desired = field.view.now_y + 98.0;
        let guide_y = |free| {
            let ctx = egui::Context::default();
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                let canvas = Canvas::new(
                    ui.painter_at(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))),
                    Flip {
                        area: Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0)),
                        direction: Direction::Up,
                    },
                );
                drop_guide(&canvas, &field.columns, (&song, free), field.view, desired);
            });
            let y = output.shapes.iter().find_map(|shape| match &shape.shape {
                egui::Shape::Path(path)
                    if path.stroke.color == egui::epaint::ColorMode::Solid(style::ACCENT) =>
                {
                    path.points.first().map(|point| point.y)
                }
                egui::Shape::Noop
                | egui::Shape::Vec(_)
                | egui::Shape::Circle(_)
                | egui::Shape::Ellipse(_)
                | egui::Shape::LineSegment { .. }
                | egui::Shape::Path(_)
                | egui::Shape::Rect(_)
                | egui::Shape::Text(_)
                | egui::Shape::Mesh(_)
                | egui::Shape::QuadraticBezier(_)
                | egui::Shape::CubicBezier(_)
                | egui::Shape::Callback(_) => None,
            });
            output.textures_delta.clear();
            y
        };
        assert_eq!(guide_y(true), Some(desired));
        assert_eq!(
            guide_y(false),
            Some(field.view.y(song.timing().time_at(Tick::new(960))))
        );
    }

    #[test]
    fn every_spectrum_level_caches_a_tile_and_refuses_tiles_past_the_track() {
        let track = Track::from_frames(1000, vec![[0.5, 0.5]; 60]);
        let tiles = Tiles::new(std::sync::Arc::new(Spectrum::of(&track)));
        let ctx = egui::Context::default();
        for level in 0..=COARSEST {
            let first = tiles.tile(&ctx, level, 0).unwrap();
            assert_eq!(tiles.tile(&ctx, level, 0), Some(first));
            assert_eq!(tiles.tile(&ctx, level, 1), None);
        }
        assert_eq!(
            tiles.made.borrow().len(),
            usize::try_from(COARSEST).unwrap() + 1
        );
    }

    #[test]
    fn horizontal_fields_keep_the_timeline_and_lane_hits_in_their_turned_positions() {
        let song = song();
        let area = Rect::from_min_size(pos2(10.0, 20.0), vec2(700.0, 400.0));
        for direction in [Direction::Left, Direction::Right] {
            let flip = Flip { area, direction };
            let field = Field::new(
                flip.field(),
                4,
                song.timing().time_at(Tick::new(0)),
                Zoom::default(),
                false,
            );
            let lane = field
                .columns
                .span(LaneIndex::new(2), LaneCount::ONE)
                .center();
            let on_screen = flip.pos(pos2(lane, field.view.now_y));
            let back = flip.back(on_screen);
            assert_eq!(
                field.lane_at(back.x),
                Some(LaneIndex::new(2)),
                "{direction:?}"
            );
            assert!((back.y - field.view.now_y).abs() < 0.01, "{direction:?}");
            assert_eq!(
                handle_at(&song, &field, pos2(field.columns.handle_x(), back.y)),
                Some(Gesture::Scrub {
                    seconds: 0.0,
                    y: back.y
                })
            );
        }
    }

    #[test]
    fn read_only_menu_disables_edits_but_keeps_selection_commands() {
        let mut song = song();
        song.select_all();
        let mut harness = Harness::builder().build_ui(|ui| {
            let mut actions = Vec::new();
            menu_items(ui, &song, (LaneIndex::new(1), Tick::new(480)), &mut actions);
            assert!(actions.is_empty());
        });
        harness.run();
        for label in [
            "Paste here",
            "Insert pattern here",
            "Add a timing point here",
            "Bookmark here",
            "Cut",
            "Delete",
        ] {
            assert!(
                harness
                    .get_by_label_contains(label)
                    .accesskit_node()
                    .is_disabled(),
                "{label}"
            );
        }
        assert!(
            !harness
                .get_by_label("Select lane 2")
                .accesskit_node()
                .is_disabled()
        );
        assert!(
            !harness
                .get_by_label_contains("Select all")
                .accesskit_node()
                .is_disabled()
        );
        assert!(
            !harness
                .get_by_label_contains("Copy")
                .accesskit_node()
                .is_disabled()
        );
    }

    #[test]
    fn changing_a_selected_ttr_note_uses_the_rulesets_requested_change() {
        let game = &charter_ttr::Plugin;
        let mut document: serde_json::Value = serde_json::from_slice(
            &game
                .new_song(&charter_core::NewSong {
                    metadata: demo::song().document().metadata.clone(),
                    layout: 0,
                    charts: vec![charter_core::NewChart::named("Hard")],
                })
                .unwrap()
                .to_json()
                .unwrap(),
        )
        .unwrap();
        document["charts"][0]["objects"] = serde_json::json!([{
            "tick": 0,
            "position": {"lane": {"index": 0}},
            "kind": "tap",
            "shape": "instant"
        }]);
        let document =
            charter_core::ValidDocument::from_json(&serde_json::to_vec(&document).unwrap())
                .unwrap();
        let mut song = Song::new(document, &[game]);
        song.select_all();
        let mut harness = Harness::builder().build_ui_state(
            move |ui, actions: &mut Vec<Action>| {
                menu_items(ui, &song, (LaneIndex::new(0), Tick::new(0)), actions);
            },
            Vec::new(),
        );
        harness.run();
        harness.get_by_label("Change to").click();
        harness.run();
        harness.get_by_label("Arrow").click();
        harness.run();
        assert!(matches!(
            harness.state().as_slice(),
            [Action::ChangeNotes(id)] if id == "arrow"
        ));
    }

    #[test]
    fn the_pattern_menu_uses_its_clicked_lane_tick_steps_and_spacing() {
        let mut harness = Harness::builder().build_ui_state(
            |ui, actions: &mut Vec<Action>| {
                patterns(ui, (LaneIndex::new(2), Tick::new(960)), actions);
            },
            Vec::new(),
        );
        harness.run();
        harness.get_by_label("Insert pattern here").click();
        harness.run();
        harness.get_by_label_contains("1-2-3-1-2-3").click();
        harness.run();
        harness.get_by_label("Every 1/4 beat").click();
        harness.run();
        let [Action::Here(at, pattern)] = harness.state().as_slice() else {
            panic!("the menu should request exactly one pattern at its click");
        };
        assert_eq!(*at, Tick::new(960));
        assert!(matches!(
            pattern.as_ref(),
            Action::Pattern {
                steps,
                per_beat,
                lane,
            } if *steps == [0, 1, 2, 0, 1, 2]
                && *per_beat == 4
                && *lane == LaneIndex::new(2)
        ));
    }

    #[test]
    fn escape_discards_an_active_marquee_before_the_pointer_is_released() {
        struct Drag {
            song: Song,
            gesture: Option<Gesture>,
            actions: Vec<Action>,
        }
        let mut harness = Harness::builder()
            .with_size(vec2(800.0, 600.0))
            .build_ui_state(
                |ui, state: &mut Drag| {
                    show(
                        ui,
                        &state.song,
                        Shown {
                            zoom: Zoom::default(),
                            direction: Direction::Up,
                            strip: None,
                            grid: grid_of(16),
                            colour_notes_by_layer: false,
                        },
                        &mut [0.0; 2],
                        &mut state.gesture,
                        &mut state.actions,
                    );
                },
                Drag {
                    song: song(),
                    gesture: None,
                    actions: Vec::new(),
                },
            );
        harness.run();
        let field = Field::new(
            harness.get_by_label("Notefield").rect(),
            4,
            harness.state().song.timing().time_at(Tick::new(0)),
            Zoom::default(),
            false,
        );
        let from = pos2(field.columns.lanes.max - 10.0, field.view.now_y + 20.0);
        let to = pos2(field.columns.lanes.min + 10.0, field.view.now_y + 95.0);
        harness.hover_at(from);
        harness.event(egui::Event::PointerButton {
            pos: from,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        harness.run();
        harness.hover_at(to);
        harness.run();
        assert!(matches!(
            harness.state().gesture,
            Some(Gesture::Marquee { .. })
        ));
        harness.key_press(egui::Key::Escape);
        harness.run();
        assert_eq!(harness.state().gesture, None);
        harness.event(egui::Event::PointerButton {
            pos: to,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        });
        harness.run();
        assert!(
            !harness
                .state()
                .actions
                .iter()
                .any(|action| matches!(action, Action::SelectBetween { .. }))
        );
    }
}
