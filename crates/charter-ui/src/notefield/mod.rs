#![expect(clippy::mod_module_files, reason = "requested directory layout")]
//! The notefield view and interaction model.
//!
//! The module keeps the public surface (`show`, [`Zoom`], [`View`] and related helper types) at
//! `crate::notefield`, while drawing, gestures, context menus, spectrum tiles and wheel math live
//! in focused submodules.

use std::num::NonZeroU16;

use charter_audio::{Peaks, Spectrum};
use charter_core::{
    Chart, Kind, LaneCount, LaneIndex, MarkerKind, Measure, Object, Position, ResolvedTime, Shape,
    Tick, Ticks, TicksPerBeat, Timing, TimingKind, Valid,
};
use charter_editor::{NoteAt, Selecting, Song, Step};
use egui::{self, FontId, Pos2, Rangef, Rect, Sense, Stroke, StrokeKind, Ui, Visuals, pos2, vec2};

use crate::flip::{Canvas, Direction, Flip};
use crate::style;
use crate::{Action, Note};

/// Grid, audio, marker and object drawing.
mod draw;
/// Pointer gestures, handle hit-testing and drag actions.
mod gesture;
/// Right-click note editing menu.
mod menu;
/// Spectrogram tile cache and heat colours.
mod spectrum;
/// Mouse wheel step accumulation.
mod wheel;

use draw::{
    backdrop, draw_object, drop_guide, events, ghost, grid, header, hold_preview_rect, markers,
    now_line, waveform,
};
use gesture::{anchored, grab, handle_at, pointer, snap_line};
use menu::menu;
use spectrum::frequencies;
use wheel::measure_steps;

pub(crate) use draw::GRIDS;
pub(crate) use draw::{ObjectOrder, kind_colours};
pub(crate) use gesture::Gesture;
pub(crate) use spectrum::Tiles;
pub(crate) use wheel::wheel_steps;

#[cfg(test)]
use gesture::{hit, hit_on_layers, let_go, panned, started};
#[cfg(test)]
use menu::{menu_items, patterns};
#[cfg(test)]
use spectrum::{COARSEST, TILE, heat, level_for, tile_image, tile_span};

/// How tall a second of audio is on screen, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Zoom(f32);

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
    pub(crate) const STEP: f32 = 1.25;

    /// The vertical scale in screen points per audio second.
    pub(crate) const fn points_per_second(self) -> f32 {
        self.0
    }

    /// How close the zoom is, against where a song opens.
    pub(crate) fn percent(self) -> f32 {
        self.0 / Self::default().0 * 100.0
    }

    /// This zoom scaled by `factor`, as far as it goes either way.
    #[must_use]
    pub(crate) fn by(self, factor: f32) -> Self {
        Self((self.0 * factor).clamp(Self::FARTHEST, Self::CLOSEST))
    }
}

/// Where audio times fall on screen in one frame: later times lower, the cursor on the now line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct View {
    /// Screen height of the cursor's now line, in points.
    pub(crate) now_y: f32,
    /// Cursor position in audio seconds.
    cursor: f64,
    /// Vertical scale of the audio timeline.
    zoom: Zoom,
}

impl View {
    /// A timeline with `cursor` aligned to `now_y`.
    pub(crate) const fn new(now_y: f32, cursor: ResolvedTime, zoom: Zoom) -> Self {
        Self {
            now_y,
            cursor: cursor.seconds(),
            zoom,
        }
    }

    /// Where `at` is drawn.
    pub(crate) fn y(self, at: ResolvedTime) -> f32 {
        self.y_of(at.seconds())
    }

    /// Where audio `seconds` into the song is drawn.
    pub(crate) fn y_of(self, seconds: f64) -> f32 {
        let below = (seconds - self.cursor) * f64::from(self.zoom.points_per_second());
        let below = crate::cast::f64_f32(below);
        self.now_y + below
    }

    /// The audio time drawn at height `y`.
    pub(crate) fn seconds_at(self, y: f32) -> f64 {
        self.cursor + f64::from(y - self.now_y) / f64::from(self.zoom.points_per_second())
    }
}

/// The line of a grid `step` ticks apart from tick 0 that is drawn nearest to `seconds`.
pub(crate) fn nearest_line(timing: Valid<'_, Timing>, seconds: f64, step: Ticks) -> Tick {
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
pub(crate) fn earliest_tick_from(timing: Valid<'_, Timing>, seconds: f64) -> Tick {
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
pub(crate) fn show(
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
    object_order: &mut ObjectOrder,
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
    // ponytail: cached by viewport ticks, so smooth scrolling still invalidates each tick range;
    // move to a wider time bucket if profiling says this remains hot.
    for &index in object_order.indices(song, (first, last)) {
        let Some(object) = chart.objects.get(index) else {
            continue;
        };
        let (layer_view, colour) = song.layer_look(object);
        if layer_view == charter_core::LayerView::Hidden {
            continue;
        }
        let selected = song.is_selected(object);
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

/// How the notefield is shown: how close, which way it runs, and the waveform of the song's
/// audio when there is one to draw.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Shown<'a> {
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
