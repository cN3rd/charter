use super::{
    Canvas, Columns, FontId, Kind, LaneCount, LaneIndex, MOST_BEATS, MarkerKind, Measure,
    NonZeroU16, Object, Peaks, Position, Rangef, Rect, Shape, Song, Stroke, StrokeKind, Tick,
    Ticks, TicksPerBeat, Timing, TimingKind, Valid, View, Visuals, egui, nearest_line, pos2, style,
    vec2,
};
use egui::Align2;

/// Cached order of the visible note objects, lower rows first.
#[derive(Debug, Default)]
pub(crate) struct ObjectOrder {
    /// The song and viewport this order describes.
    key: Option<ObjectOrderKey>,
    /// Object indices in paint order.
    indices: Vec<usize>,
}

/// The song and viewport facts that make an object order reusable.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ObjectOrderKey {
    /// Song revision: changes when notes change.
    revision: u64,
    /// History position: catches layer edits and chart switches; cheap enough for now.
    history_step: usize,
    /// Open chart name.
    chart: String,
    /// First visible tick.
    first: Tick,
    /// Last visible tick.
    last: Tick,
    /// Current object count.
    objects: usize,
    /// Current layer count.
    layers: usize,
}

impl ObjectOrder {
    /// Draw lower rows first, preserving note order within each row.
    pub(crate) fn indices(&mut self, song: &Song, (first, last): (Tick, Tick)) -> &[usize] {
        let Some(chart) = song.chart() else {
            self.key = None;
            self.indices.clear();
            return &self.indices;
        };
        let matches = self.key.as_ref().is_some_and(|key| {
            key.revision == song.revision()
                && key.history_step == song.history_step()
                && key.chart == chart.name
                && key.first == first
                && key.last == last
                && key.objects == chart.objects.len()
                && key.layers == chart.layers.len()
        });
        if matches {
            return &self.indices;
        }
        self.indices.clear();
        self.indices.extend(reaching(&chart.objects, first, last));
        self.indices.sort_by_key(|index| {
            std::cmp::Reverse(
                chart
                    .objects
                    .get(*index)
                    .and_then(|object| chart.layer_index(object))
                    .unwrap_or(0),
            )
        });
        self.key = Some(ObjectOrderKey {
            revision: song.revision(),
            history_step: song.history_step(),
            chart: chart.name.clone(),
            first,
            last,
            objects: chart.objects.len(),
            layers: chart.layers.len(),
        });
        &self.indices
    }
}

/// Says whose timing the chart uses and numbers its lanes.
pub(super) fn header(
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
pub(super) fn backdrop(painter: &Canvas, columns: &Columns, lanes: u16, rows: Rangef) {
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

/// The audio under each row of `rows`, a point apart: the row, and the lowest and highest
/// sample in the time it covers.
pub(super) fn wave_rows(peaks: &Peaks, view: View, rows: Rangef) -> Vec<(f32, f32, f32)> {
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
pub(super) fn waveform(
    painter: &Canvas,
    columns: &Columns,
    view: View,
    rows: Rangef,
    peaks: &Peaks,
) {
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
pub(crate) const GRIDS: [u16; 7] = [4, 8, 12, 16, 24, 32, 48];

/// A line of the grid inside a measure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Line {
    /// A line coinciding with a quarter-note beat.
    Beat,
    /// A line of a finer grid between two beats.
    Between,
}

/// The lines inside `measure`, after its start: one on every `grid`th note, each on a beat
/// marked as one, so a measure of 4/4 on a grid of 16 has sixteen places with every fourth a
/// beat. A grid the resolution does not hold in whole ticks draws the beats alone.
pub(super) fn lines_in(
    measure: &Measure,
    resolution: TicksPerBeat,
    grid: NonZeroU16,
) -> Vec<(Tick, Line)> {
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
pub(super) fn grid(
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
pub(super) fn markers(
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
pub(super) fn events(
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
        for (row, detail) in (0_u8..).zip(point) {
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
pub(super) fn object_colours(
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
pub(super) fn draw_object(
    painter: &Canvas,
    columns: &Columns,
    timing: Valid<'_, Timing>,
    view: View,
    object: &Object,
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
    let end = view.y(timing.time_at(object.shape.end(object.tick).unwrap_or(object.tick)));
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
    for child in &object.children {
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
pub(super) fn hold_preview_rect(
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

/// Where a dragged note will land, `by` ticks later and lanes across: its head outlined, and
/// its tail when it lasts, when that shows between the ticks `first` and `last`.
pub(super) fn ghost(
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
pub(super) fn drop_guide(
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
pub(super) fn reaching(
    objects: &[Object],
    first: Tick,
    last: Tick,
) -> impl Iterator<Item = usize> + '_ {
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
pub(super) fn now_line(painter: &Canvas, columns: &Columns, now_y: f32) {
    painter.hline(columns.timeline(), now_y, Stroke::new(2.0, style::NOW_LINE));
    let tip = pos2(columns.ruler.max, now_y);
    let handle = vec![tip + vec2(-11.0, -6.0), tip, tip + vec2(-11.0, 6.0)];
    painter.convex_polygon(handle, style::NOW_LINE, Stroke::NONE);
}

#[cfg(test)]
mod tests {
    use charter_core::RulesetPlugin;
    use egui::{Modifiers, MouseWheelUnit, TouchPhase, Vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};

    use super::super::*;
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
                    &object,
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
                    &mut ObjectOrder::default(),
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
        let mut order = ObjectOrder::default();
        let indices = order.indices(&song, (Tick::new(0), Tick::new(1_000_000)));
        let chart = song.chart().unwrap();
        let layers: Vec<_> = indices
            .iter()
            .map(|index| chart.layer_index(&chart.objects[*index]).unwrap())
            .collect();
        assert_eq!(layers.first(), Some(&1));
        assert_eq!(layers.last(), Some(&0));
    }

    #[test]
    fn cached_object_order_key_includes_layer_history() {
        let mut song = Song::new(demo::song(), &[&charter_stepmania::Plugin]);
        let mut order = ObjectOrder::default();
        order.indices(&song, (Tick::new(0), Tick::new(1_000_000)));
        let first = order.key.clone();
        song.add_layer().unwrap();
        order.indices(&song, (Tick::new(0), Tick::new(1_000_000)));
        assert_ne!(order.key, first);
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
        let mut frames = vec![[0_u8; Spectrum::BANDS]; TILE + 10];
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
                        &mut ObjectOrder::default(),
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
