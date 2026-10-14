//! The rows of the timing window: the song laid out either by bar position or by audio time, a
//! little of the previous row shown as lead-in, and everything drawn over a row.

use charter_audio::{Onset, Peaks};
use charter_core::{Measure, ResolvedTime, Tick, Timing, Valid};
use charter_style as style;
use egui::{Align2, Color32, FontId, Pos2, Rect, Shape, Stroke, pos2, vec2};

use crate::accuracy::{Residual, SATURATION_MS};
use crate::grid::{Grid, Line};

/// How tall a row is, its residual strip included.
pub(crate) const ROW: f32 = 76.0;
/// How tall the strip under a row that shows the residuals is.
pub(crate) const STRIP: f32 = 6.0;
/// How wide the row headers down the left are.
pub(crate) const GUTTER: f32 = 56.0;
/// How far from an anchor's line, in points, a press still takes it.
pub(crate) const GRAB: f32 = 6.0;
/// How much of the bar before each row starts shows ahead of its downbeat, so a pickup is seen.
pub const LEAD_IN: f64 = 0.10;

/// How many bars a row holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum BarsPerRow {
    /// Half a bar, for close inspection.
    Half,
    /// One bar across the row.
    #[default]
    One,
    /// Two bars across the row.
    Two,
    /// Four bars across the row.
    Four,
}

impl BarsPerRow {
    /// Choices in toolbar order.
    pub const ALL: [Self; 4] = [Self::Half, Self::One, Self::Two, Self::Four];

    /// Bar count represented by this choice.
    #[must_use]
    pub const fn bars(self) -> f64 {
        match self {
            Self::Half => 0.5,
            Self::One => 1.0,
            Self::Two => 2.0,
            Self::Four => 4.0,
        }
    }

    /// Short label for the toolbar.
    #[must_use]
    #[expect(
        clippy::non_ascii_literal,
        reason = "the half-bar row option displays the ½ glyph"
    )]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Half => "½ bar",
            Self::One => "1 bar",
            Self::Two => "2 bars",
            Self::Four => "4 bars",
        }
    }
}

/// How long a row is in the time layout.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SecondsPerRow {
    /// One second across the row.
    One,
    /// Two seconds across the row.
    #[default]
    Two,
    /// Four seconds across the row.
    Four,
    /// Eight seconds across the row.
    Eight,
}

impl SecondsPerRow {
    /// Choices in toolbar order.
    pub const ALL: [Self; 4] = [Self::One, Self::Two, Self::Four, Self::Eight];

    /// Audio duration represented by this choice.
    #[must_use]
    pub const fn seconds(self) -> f64 {
        match self {
            Self::One => 1.0,
            Self::Two => 2.0,
            Self::Four => 4.0,
            Self::Eight => 8.0,
        }
    }

    /// Short label for the toolbar.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::One => "1 s",
            Self::Two => "2 s",
            Self::Four => "4 s",
            Self::Eight => "8 s",
        }
    }
}

/// Which way the rows lay the song out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Layout {
    /// By bar position: the grid stays put and the audio stretches to it, so a correct map lines
    /// its beats up down the rows, and an edit slides the waveform under the grid.
    Bars,
    /// By audio time: each row a fixed span of seconds, the waveform never moves, and an edit
    /// moves the grid lines and anchors over it. Timing points are moments of the audio, so the
    /// window opens on this.
    #[default]
    Time,
}

/// Bars maps ticks to positions along the song.
///
/// The measures of a stretch of the song, by number, for turning ticks into bar positions and
/// back: a bar position counts the bar and how far through it, so a correct map lines beats up
/// down the rows whatever the tempo. The rows lay ticks out by a position that is the bar
/// position in the bar layout and the audio time in the time layout.
#[derive(Clone, Debug)]
pub(crate) struct Bars<'a> {
    /// Measures used to translate between ticks and bar positions.
    measures: Vec<Measure>,
    /// Validated timing map for audio positions.
    timing: Valid<'a, Timing>,
    /// Whether positions are bars or seconds.
    layout: Layout,
}

impl<'a> Bars<'a> {
    /// The measures from the one holding `from` to the one holding `to`, and `tail` more.
    #[must_use]
    pub fn new(timing: Valid<'a, Timing>, from: Tick, to: Tick, tail: usize) -> Self {
        let mut measures: Vec<Measure> = timing
            .measures_from(from)
            .take_while(|measure| measure.start <= to)
            .collect();
        let after = measures
            .last()
            .and_then(|last| last.start.checked_add(last.length));
        if let Some(after) = after {
            measures.extend(timing.measures_from(after).take(tail));
        }
        Self {
            measures,
            timing,
            layout: Layout::Bars,
        }
    }

    /// These bars, laid out by `layout`.
    #[must_use]
    pub fn laid_out(self, layout: Layout) -> Self {
        Self { layout, ..self }
    }

    /// Whether positions in these rows count bars or seconds.
    #[must_use]
    pub const fn layout(&self) -> Layout {
        self.layout
    }

    /// Where the rows lay `tick` out, which may be between ticks: its bar position, or its time.
    #[must_use]
    pub fn position(&self, tick: f64) -> f64 {
        match self.layout {
            Layout::Bars => self.bar_of(tick),
            Layout::Time => seconds_at_tick(self.timing, tick),
        }
    }

    /// The tick, maybe between ticks, the rows lay out at `position`.
    #[must_use]
    pub fn tick_at_position(&self, position: f64) -> f64 {
        match self.layout {
            Layout::Bars => self.tick_at(position),
            Layout::Time => tick_at_seconds(self.timing, position),
        }
    }

    /// The measure holding `tick`, among these.
    #[must_use]
    pub fn measure_at(&self, tick: f64) -> Option<&Measure> {
        let at = self
            .measures
            .partition_point(|measure| start(measure) <= tick)
            .checked_sub(1)?;
        self.measures.get(at)
    }

    /// Measures used for translating ticks into bar positions.
    #[must_use]
    pub fn measures(&self) -> &[Measure] {
        &self.measures
    }

    /// The bar position of `tick`, which may be between ticks.
    #[must_use]
    pub fn bar_of(&self, tick: f64) -> f64 {
        let at = self
            .measures
            .partition_point(|measure| start(measure) <= tick)
            .saturating_sub(1);
        let Some(measure) = self.measures.get(at) else {
            return 0.0;
        };
        let number = float_from_i64(measure.number);
        number + (tick - start(measure)) / length(measure)
    }

    /// The tick, maybe between ticks, at bar position `bar`.
    #[must_use]
    pub fn tick_at(&self, bar: f64) -> f64 {
        let (Some(first), Some(last)) = (self.measures.first(), self.measures.last()) else {
            return 0.0;
        };
        let number = float_to_i64(bar.floor());
        let measure = usize::try_from(number.saturating_sub(first.number))
            .ok()
            .and_then(|at| self.measures.get(at))
            .unwrap_or(if number < first.number { first } else { last });
        let into = bar - float_from_i64(measure.number);
        start(measure) + into * length(measure)
    }

    /// The rows the bars make, `per` bars a row in the bar layout or `seconds` a row in the time
    /// layout, each showing a tenth of its length from before it.
    #[must_use]
    pub fn rows(&self, per: BarsPerRow, seconds: SecondsPerRow) -> Vec<Span> {
        let (Some(first), Some(last)) = (self.measures.first(), self.measures.last()) else {
            return Vec::new();
        };
        let end = start(last) + length(last);
        let (from, to, step_per_row) = match self.layout {
            Layout::Bars => (
                float_from_i64(first.number),
                float_from_i64(last.number) + 1.0,
                per.bars(),
            ),
            Layout::Time => {
                let duration = seconds.seconds();
                let from =
                    (seconds_at_tick(self.timing, start(first)) / duration).floor() * duration;
                (from, seconds_at_tick(self.timing, end), duration)
            }
        };
        let mut rows = Vec::new();
        let mut at = from;
        while at < to - 1e-9 {
            rows.push(Span {
                from: at - LEAD_IN * step_per_row,
                start: at,
                to: at + step_per_row,
            });
            at += step_per_row;
        }
        rows
    }
}

/// The stretch a row spans, in positions: from where it starts showing, through where its own
/// part starts, to where it ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Span {
    /// Beginning of the lead-in, in bars or seconds.
    pub from: f64,
    /// Beginning of the row's own span.
    pub start: f64,
    /// End of the row's own span.
    pub to: f64,
}

/// The tick, maybe between ticks, at `seconds` into the audio: the earliest of a paused stretch.
#[must_use]
pub(crate) fn tick_at_seconds(timing: Valid<'_, Timing>, seconds: f64) -> f64 {
    let Some(moment) = ResolvedTime::from_seconds(seconds) else {
        return seconds;
    };
    let tick = timing.latest_tick_by(moment);
    let after = Tick::new(i32::try_from(i64::from(tick).saturating_add(1)).unwrap_or(i32::MAX));
    let (at, next) = (
        timing.time_at(tick).seconds(),
        timing.time_at(after).seconds(),
    );
    let within = if next > at {
        (seconds - at) / (next - at)
    } else {
        0.0
    };
    let whole = float_from_i64(i64::from(tick));
    whole + within.clamp(0.0, 1.0)
}

/// Start tick of `measure` as a floating-point value.
fn start(measure: &Measure) -> f64 {
    float_from_i64(i64::from(measure.start))
}

/// Length of `measure` in ticks as a floating-point value.
fn length(measure: &Measure) -> f64 {
    f64::from(u32::from(measure.length))
}

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "song positions fit visual precision"
)]
/// Converts a tick count to floating-point visual coordinates.
pub(crate) const fn float_from_i64(value: i64) -> f64 {
    value as f64
}

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "row counts fit visual precision"
)]
/// Converts a row count to single-precision drawing coordinates.
pub(crate) const fn float_from_usize(value: usize) -> f32 {
    value as f32
}

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "collection counts fit visual precision"
)]
/// Converts a collection count to double precision for visual calculations.
pub(crate) const fn double_from_usize(value: usize) -> f64 {
    value as f64
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "rendering needs f32 coordinates"
)]
/// Converts a visual coordinate to the painter's single-precision coordinates.
pub(crate) const fn float_to_f32(value: f64) -> f32 {
    value as f32
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "input is clamped to chart ticks"
)]
/// Clamps a floating-point chart tick to the range of `i32` and truncates it.
pub(crate) fn float_to_i32(value: f64) -> i32 {
    value.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "bar numbers outside i64 saturate"
)]
/// Truncates a floating-point bar number to an integer, saturating outside the `i64` range.
pub(crate) const fn float_to_i64(value: f64) -> i64 {
    value as i64
}

/// A row on screen: the positions it spans, and where.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Row {
    /// Position where the lead-in to this row begins, in bars or seconds.
    pub from: f64,
    /// Where the row's own part starts, after the lead-in from before it.
    pub start: f64,
    /// Position where the row ends, in bars or seconds.
    pub to: f64,
    /// Where the audio is drawn, the residual strip under it.
    pub rect: Rect,
}

impl Row {
    /// Where the position `position` lies across the row.
    #[must_use]
    pub fn x_of(&self, position: f64) -> f32 {
        let across = float_to_f32((position - self.from) / (self.to - self.from));
        self.rect.left() + across * self.rect.width()
    }

    /// The position at `x`.
    #[must_use]
    pub fn position_at(&self, x: f32) -> f64 {
        let across = f64::from((x - self.rect.left()) / self.rect.width());
        self.from + across * (self.to - self.from)
    }

    /// Screen x-coordinate of `tick` in the row's layout.
    #[must_use]
    pub fn x(&self, bars: &Bars, tick: Tick) -> f32 {
        let at = float_from_i64(i64::from(tick));
        self.x_of(bars.position(at))
    }

    /// The tick nearest `x`.
    #[must_use]
    pub fn tick_at(&self, bars: &Bars, x: f32) -> Tick {
        let tick = bars.tick_at_position(self.position_at(x)).round();
        let tick = float_to_i32(tick);
        Tick::new(tick)
    }

    /// The moment of the audio at `x`, between ticks as finely as the row draws.
    #[must_use]
    pub fn seconds_at(&self, bars: &Bars, timing: Valid<'_, Timing>, x: f32) -> f64 {
        match bars.layout {
            Layout::Bars => seconds_at_tick(timing, bars.tick_at(self.position_at(x))),
            Layout::Time => self.position_at(x),
        }
    }

    /// Whether `tick` shows in this row.
    #[must_use]
    pub fn holds(&self, bars: &Bars, tick: Tick) -> bool {
        let position = bars.position(float_from_i64(i64::from(tick)));
        position >= self.from && position < self.to
    }

    /// The whole row on screen, header and strip included.
    #[must_use]
    pub fn whole(&self) -> Rect {
        Rect::from_min_max(
            pos2(self.rect.left() - GUTTER, self.rect.top()),
            self.rect.max,
        )
    }
}

/// The moment of the audio at `tick`, which may be between ticks.
#[must_use]
pub(crate) fn seconds_at_tick(timing: Valid<'_, Timing>, tick: f64) -> f64 {
    let below = tick.floor();
    let whole = float_to_i32(below.clamp(f64::from(i32::MIN), f64::from(i32::MAX - 1)));
    let (at, next) = (
        timing.time_at(Tick::new(whole)).seconds(),
        timing.time_at(Tick::new(whole.saturating_add(1))).seconds(),
    );
    at + (next - at) * (tick - below)
}

/// How an anchor is drawn.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Anchor {
    /// Tick held by the anchor.
    pub tick: Tick,
    /// The tempo from it on.
    pub bpm: String,
    /// Whether this anchor has keyboard focus.
    pub focused: bool,
    /// Whether this anchor is selected.
    pub selected: bool,
    /// How strongly it pulses, from 0 to 1, when an edit ran into it.
    pub pulse: f32,
}

/// Where a pending anchor edit would land on a row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Ghost {
    /// Screen x-coordinate on the row.
    pub x: f32,
    /// Whether the target was snapped to the beat grid or an onset.
    pub snapped: bool,
}

/// Row-position interval a dragged anchor may occupy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Reach {
    /// First allowed position.
    pub from: f64,
    /// First position beyond the allowed interval.
    pub to: f64,
}

/// Everything a row draws.
#[derive(Debug)]
pub(crate) struct Scene<'a> {
    /// Validated timing of the song being drawn.
    pub timing: Valid<'a, Timing>,
    /// Measures and layout used to locate ticks.
    pub bars: &'a Bars<'a>,
    /// Beat lines shown over the waveform.
    pub grid: Grid,
    /// Decoded audio envelope, when available.
    pub peaks: Option<&'a Peaks>,
    /// Detected transients, in time order.
    pub onsets: &'a [Onset],
    /// Minimum strength for a transient to appear.
    pub sensitivity: f32,
    /// Distances from detected transients to grid lines.
    pub residuals: &'a [Residual],
    /// Visible anchor labels and highlights.
    pub anchors: &'a [Anchor],
    /// Playhead tick.
    pub cursor: Tick,
    /// Where a drag would drop, and whether it is on a snap target.
    pub ghost: Option<Ghost>,
    /// The row positions a dragged anchor may reach; the rest is dimmed.
    pub reach: Option<Reach>,
}

/// Draws `row` of `scene` with `painter`.
pub(crate) fn draw(painter: &egui::Painter, row: &Row, scene: &Scene<'_>) {
    let rect = row.rect;
    let audio = Rect::from_min_max(rect.min, pos2(rect.right(), rect.bottom() - STRIP));
    painter.rect_filled(audio, 2.0, style::WAVE_BG);
    if let Some(peaks) = scene.peaks {
        waveform(painter, row, audio, scene, peaks);
    }
    grid(painter, row, audio, scene);
    onsets(painter, row, audio, scene);
    strip(painter, row, scene);
    if let Some(reach) = scene.reach {
        dim(painter, row, audio, reach.from, reach.to);
    }
    for anchor in scene
        .anchors
        .iter()
        .filter(|anchor| row.holds(scene.bars, anchor.tick))
    {
        draw_anchor(painter, row, audio, scene.bars, anchor);
    }
    if row.holds(scene.bars, scene.cursor) {
        let x = row.x(scene.bars, scene.cursor);
        painter.vline(x, audio.y_range(), Stroke::new(1.5, style::NOW_LINE));
    }
    if let Some(ghost) = scene.ghost
        && audio.x_range().contains(ghost.x)
    {
        painter.vline(ghost.x, audio.y_range(), Stroke::new(1.0, style::ACCENT));
        let centre = pos2(ghost.x, audio.top() + 7.0);
        let stroke = Stroke::new(1.5, style::ACCENT);
        if ghost.snapped {
            painter.circle_stroke(centre, 4.0, stroke);
        } else {
            painter.rect_stroke(
                Rect::from_center_size(centre, vec2(7.0, 7.0)),
                0.0,
                stroke,
                egui::StrokeKind::Middle,
            );
        }
    }
}

/// The row's header: `title`, its bar number or its time, the metre, and a badge when its bar
/// misses its onsets.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "egui screen-coordinate vector addition cannot overflow integers"
)]
pub(crate) fn header(
    painter: &egui::Painter,
    row: &Row,
    (title, measure): (&str, Option<&Measure>),
    off_ms: Option<f64>,
) {
    let at = pos2(row.rect.left() - 8.0, row.rect.top() + 4.0);
    let colour = if off_ms.is_some() {
        style::WARN
    } else {
        style::TEXT
    };
    painter.text(
        at,
        Align2::RIGHT_TOP,
        title,
        FontId::monospace(12.0),
        colour,
    );
    if let Some(measure) = measure {
        let below = at + vec2(0.0, 15.0);
        let metre = measure.meter.to_string();
        painter.text(
            below,
            Align2::RIGHT_TOP,
            metre,
            FontId::monospace(10.0),
            style::WEAK_TEXT,
        );
    }
    if let Some(off) = off_ms {
        let below = at + vec2(0.0, 30.0);
        let badge = format!("⚠{off:.0}ms");
        painter.text(
            below,
            Align2::RIGHT_TOP,
            badge,
            FontId::proportional(10.0),
            style::WARN,
        );
    }
}

/// Draws the audio envelope one pixel column at a time across `row`.
fn waveform(painter: &egui::Painter, row: &Row, audio: Rect, scene: &Scene<'_>, peaks: &Peaks) {
    let (middle, half) = (audio.center().y, audio.height() / 2.0 - 4.0);
    let mut x = audio.left().floor();
    let mut from = row.seconds_at(scene.bars, scene.timing, x);
    while x < audio.right() {
        let to = row.seconds_at(scene.bars, scene.timing, x + 1.0);
        let extent = peaks.between(from.min(to), from.max(to));
        from = to;
        if extent.high - extent.low >= 0.004 {
            let top = middle - extent.high.clamp(-1.0, 1.0) * half;
            let bottom = middle - extent.low.clamp(-1.0, 1.0) * half;
            painter.vline(
                x + 0.5,
                egui::Rangef::new(top, bottom),
                Stroke::new(1.0, style::TIMING_WAVE),
            );
        }
        x += 1.0;
    }
}

/// The grid lines in the row, told apart by shape as well as colour: downbeats thick, beats
/// solid, lines between dashed, or dotted for triplets, and the beats numbered along the top.
fn grid(painter: &egui::Painter, row: &Row, audio: Rect, scene: &Scene<'_>) {
    let resolution = scene.timing.ticks_per_beat();
    for measure in scene.bars.measures() {
        let (from, to) = (
            scene.bars.position(start(measure)),
            scene.bars.position(start(measure) + length(measure)),
        );
        if to < row.from || from > row.to {
            continue;
        }
        let mut beat = 0_u32;
        for (tick, line) in scene.grid.lines(measure, resolution) {
            // Beats count from the bar's downbeat, whether or not this row shows it.
            if matches!(line, Line::Downbeat | Line::Beat) {
                beat = beat.saturating_add(1);
            }
            if !row.holds(scene.bars, tick) {
                continue;
            }
            let x = row.x(scene.bars, tick);
            let (top, bottom) = (pos2(x, audio.top()), pos2(x, audio.bottom()));
            match line {
                Line::Downbeat => {
                    painter.line_segment([top, bottom], Stroke::new(2.0, style::MEASURE_LINE));
                }
                Line::Beat => {
                    painter.line_segment([top, bottom], Stroke::new(1.2, style::DIVIDER));
                }
                Line::Sub { triplet: false } => {
                    painter.extend(Shape::dashed_line(
                        &[top, bottom],
                        Stroke::new(1.0, style::DIVIDER),
                        4.0,
                        4.0,
                    ));
                }
                Line::Sub { triplet: true } => {
                    painter.extend(Shape::dotted_line(&[top, bottom], style::DIVIDER, 5.0, 1.0));
                }
            }
            if matches!(line, Line::Downbeat | Line::Beat) {
                let label = pos2(x + 3.0, audio.top() + 2.0);
                let text = beat.to_string();
                painter.text(
                    label,
                    Align2::LEFT_TOP,
                    text,
                    FontId::monospace(9.0),
                    style::WEAK_TEXT,
                );
            }
        }
    }
    // The part of the bar before, shown ahead of the downbeat, is shaded as not this row's.
    let downbeat = row.x_of(row.start);
    let lead = Rect::from_min_max(audio.min, pos2(downbeat, audio.bottom()));
    painter.rect_filled(lead, 0.0, Color32::from_black_alpha(70));
}

/// A tick along the bottom of the audio for each onset, taller the stronger it is.
fn onsets(painter: &egui::Painter, row: &Row, audio: Rect, scene: &Scene<'_>) {
    let (from, to) = (
        row.seconds_at(scene.bars, scene.timing, audio.left()),
        row.seconds_at(scene.bars, scene.timing, audio.right()),
    );
    let first = scene.onsets.partition_point(|onset| onset.seconds < from);
    let shown = scene.onsets.get(first..).unwrap_or_default();
    for onset in shown.iter().take_while(|onset| onset.seconds <= to) {
        if onset.strength < scene.sensitivity {
            continue;
        }
        let x = x_at_seconds(row, scene, onset.seconds);
        let height = 3.0 + 9.0 * onset.strength;
        let range = egui::Rangef::new(audio.bottom() - height, audio.bottom());
        painter.vline(x, range, Stroke::new(1.5, style::ONSET));
    }
}

/// Where the moment `seconds` lies across the row.
#[must_use]
pub(crate) fn x_at_seconds(row: &Row, scene: &Scene<'_>, seconds: f64) -> f32 {
    match scene.bars.layout() {
        Layout::Time => row.x_of(seconds),
        Layout::Bars => row.x_of(scene.bars.bar_of(tick_at_seconds(scene.timing, seconds))),
    }
}

/// The residual strip: each onset on the grid as a bar in the strip under the row, in the top half
/// when the audio comes early and the bottom half when late, so the half says which as well as
/// the colour, and fuller the farther off, full at the saturation.
fn strip(painter: &egui::Painter, row: &Row, scene: &Scene<'_>) {
    let strip = Rect::from_min_max(
        pos2(row.rect.left(), row.rect.bottom() - STRIP),
        row.rect.max,
    );
    painter.rect_filled(strip, 0.0, style::FIELD);
    for residual in scene.residuals {
        if !row.holds(scene.bars, residual.line) {
            continue;
        }
        let x = x_at_seconds(row, scene, residual.seconds);
        let size = float_to_f32((residual.ms.abs() / SATURATION_MS).min(1.0));
        let (colour, half) = if residual.ms < 0.0 {
            (style::RESIDUAL_EARLY, strip.top()..=strip.center().y)
        } else {
            (style::RESIDUAL_LATE, strip.center().y..=strip.bottom())
        };
        let bar = Rect::from_x_y_ranges(x - 1.5..=x + 1.5, half);
        painter.rect_filled(bar, 0.0, colour.gamma_multiply(0.2 + 0.8 * size));
    }
}

/// Shades the audio outside the reachable span from `from` to `to`.
fn dim(painter: &egui::Painter, row: &Row, audio: Rect, from: f64, to: f64) {
    let (left, right) = (
        row.x_of(from).max(audio.left()),
        row.x_of(to).min(audio.right()),
    );
    if left > audio.left() {
        painter.rect_filled(
            Rect::from_min_max(audio.min, pos2(left, audio.bottom())),
            0.0,
            style::REACH_DIM,
        );
    }
    if right < audio.right() {
        painter.rect_filled(
            Rect::from_min_max(pos2(right, audio.top()), audio.max),
            0.0,
            style::REACH_DIM,
        );
    }
}

/// An anchor: a line the height of the audio with a flag at its head naming the tempo from it
/// on. Focused or selected, it draws at twice the playhead's width with its flag filled, and
/// focused, brighter.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "egui screen-coordinate vector addition cannot overflow integers"
)]
fn draw_anchor(painter: &egui::Painter, row: &Row, audio: Rect, bars: &Bars, anchor: &Anchor) {
    let x = row.x(bars, anchor.tick);
    let colour = if anchor.focused {
        style::ANCHOR_FOCUS
    } else if anchor.selected {
        style::ANCHOR_SELECTED
    } else {
        style::ANCHOR
    };
    let colour = if anchor.pulse > 0.0 {
        colour.lerp_to_gamma(style::REFUSED, anchor.pulse)
    } else {
        colour
    };
    // Selected, it is near-white like the playhead, so its weight sets it apart.
    let held = anchor.focused || anchor.selected;
    let width = if held { 3.0 } else { 2.0 } + 2.0 * anchor.pulse;
    painter.vline(x, audio.y_range(), Stroke::new(width, colour));
    let galley = painter.layout_no_wrap(anchor.bpm.clone(), FontId::monospace(11.0), colour);
    let flag = Rect::from_min_size(pos2(x, audio.top() + 12.0), galley.size() + vec2(8.0, 2.0));
    if anchor.selected || anchor.focused {
        painter.rect_filled(flag, 2.0, colour.gamma_multiply(0.35));
    } else {
        painter.rect_filled(flag, 2.0, style::WAVE_BG);
    }
    painter.rect_stroke(
        flag,
        2.0,
        Stroke::new(1.0, colour),
        egui::StrokeKind::Inside,
    );
    painter.galley(flag.min + vec2(4.0, 1.0), galley, colour);
}

/// Where `pos` is, among the rows laid out.
#[must_use]
pub(crate) fn row_at(rows: &[Row], pos: Pos2) -> Option<&Row> {
    rows.iter()
        .find(|row| row.whole().y_range().contains(pos.y))
}

#[cfg(test)]
mod tests {
    use charter_audio::Track;
    use charter_core::ValidDocument;
    use egui_kittest::Harness;
    use proptest::prelude::*;

    use super::*;

    fn song(events: &serde_json::Value) -> ValidDocument {
        let value = serde_json::json!({
            "format_version": 0,
            "ruleset": { "id": "t", "version": "1" },
            "metadata": { "title": "t", "artist": "a", "audio": "a.ogg" },
            "timing": { "offset_seconds": 0.0, "events": events },
            "charts": []
        });
        ValidDocument::from_json(&serde_json::to_vec(&value).unwrap()).unwrap()
    }

    #[test]
    fn a_non_silent_envelope_draws_visible_waveform_strokes() {
        let document = song(&serde_json::json!([{ "tick": 0, "kind": { "bpm": 120.0 } }]));
        let peaks = Peaks::of(&Track::from_frames(64, vec![[0.8, -0.8]; 128]));
        let mut harness = Harness::builder()
            .with_size(vec2(130.0, 100.0))
            .build_ui_state(
                |ui, (document, peaks): &mut (ValidDocument, Peaks)| {
                    let envelope = &*peaks;
                    let timing = document.timing();
                    let bars = Bars::new(timing, Tick::new(0), Tick::new(3840), 0);
                    let row = Row {
                        from: -0.1,
                        start: 0.0,
                        to: 1.0,
                        rect: Rect::from_min_size(pos2(0.0, 0.0), vec2(110.0, ROW)),
                    };
                    let scene = Scene {
                        timing,
                        bars: &bars,
                        grid: Grid::default(),
                        peaks: Some(envelope),
                        onsets: &[],
                        sensitivity: 0.1,
                        residuals: &[],
                        anchors: &[],
                        cursor: Tick::new(0),
                        ghost: None,
                        reach: None,
                    };
                    waveform(ui.painter(), &row, row.rect, &scene, envelope);
                },
                (document, peaks),
            );
        harness.run();
        assert!(harness.output().shapes.iter().any(|shape| {
            matches!(
                &shape.shape,
                egui::Shape::LineSegment { stroke, .. } if stroke.color == style::TIMING_WAVE
            )
        }));
    }

    /// What `paint` draws over a row holding the first bar of a 120 BPM song, with `residuals`,
    /// `anchors` and the playhead at `cursor`.
    fn painted(
        residuals: &[Residual],
        anchors: &[Anchor],
        cursor: Tick,
        paint: fn(&egui::Painter, &Row, &Scene<'_>),
    ) -> Vec<Shape> {
        let document = song(&serde_json::json!([{ "tick": 0, "kind": { "bpm": 120.0 } }]));
        let timing = document.timing();
        let bars = Bars::new(timing, Tick::new(0), Tick::new(3840), 0);
        let row = Row {
            from: -0.1,
            start: 0.0,
            to: 1.0,
            rect: Rect::from_min_size(pos2(0.0, 0.0), vec2(110.0, ROW)),
        };
        let scene = Scene {
            timing,
            bars: &bars,
            grid: Grid::default(),
            peaks: None,
            onsets: &[],
            sensitivity: 0.1,
            residuals,
            anchors,
            cursor,
            ghost: None,
            reach: None,
        };
        let mut output = egui::Context::default().run_ui(egui::RawInput::default(), |ui| {
            paint(ui.painter(), &row, &scene);
        });
        output.textures_delta.clear();
        output
            .shapes
            .into_iter()
            .map(|clipped| clipped.shape)
            .collect()
    }

    #[test]
    fn an_early_residual_fills_the_top_of_the_strip_and_a_late_one_the_bottom() {
        let residuals = [
            Residual {
                seconds: 0.48,
                strength: 1.0,
                line: Tick::new(960),
                ms: -SATURATION_MS,
            },
            Residual {
                seconds: 1.02,
                strength: 1.0,
                line: Tick::new(1920),
                ms: SATURATION_MS,
            },
        ];
        let shapes = painted(&residuals, &[], Tick::new(0), strip);
        let bar = |colour: Color32| {
            shapes
                .iter()
                .find_map(|shape| {
                    if let Shape::Rect(rect) = shape
                        && rect.fill == colour
                    {
                        Some(rect.rect)
                    } else {
                        None
                    }
                })
                .unwrap()
        };
        // The half says early or late, so the two read apart without their colours.
        let middle = ROW - STRIP / 2.0;
        assert!(bar(style::RESIDUAL_EARLY).bottom() <= middle + 0.01);
        assert!(bar(style::RESIDUAL_LATE).top() >= middle - 0.01);
    }

    #[test]
    fn a_selected_anchor_is_drawn_at_least_twice_as_wide_as_the_playhead() {
        let anchors = [Anchor {
            tick: Tick::new(0),
            bpm: "120".to_owned(),
            focused: false,
            selected: true,
            pulse: 0.0,
        }];
        let shapes = painted(&[], &anchors, Tick::new(1920), draw);
        let width = |colour: Color32| {
            shapes
                .iter()
                .find_map(|shape| {
                    if let Shape::LineSegment { stroke, .. } = shape
                        && stroke.color == colour
                    {
                        Some(stroke.width)
                    } else {
                        None
                    }
                })
                .unwrap()
        };
        // Both are near-white lines down the audio, so their weight is what tells them apart.
        assert!(width(style::ANCHOR_SELECTED) >= 2.0 * width(style::NOW_LINE));
    }

    #[test]
    fn bar_positions_count_bars_and_how_far_through_whatever_the_metre() {
        let document = song(&serde_json::json!([
            { "tick": 0, "kind": { "bpm": 120.0 } },
            { "tick": 3840, "kind": { "time_signature": [3, 4] } }
        ]));
        let bars = Bars::new(document.timing(), Tick::new(0), Tick::new(6000), 1);
        assert!((bars.bar_of(1920.0) - 0.5).abs() < 1e-9);
        assert!((bars.bar_of(3840.0 + 1440.0) - 1.5).abs() < 1e-9);
        assert!((bars.tick_at(1.5) - 5280.0).abs() < 1e-9);
        assert!((bars.tick_at(-0.25) + 960.0).abs() < 1e-9);
    }

    #[test]
    fn rows_hold_their_bars_and_a_tenth_of_the_bar_before() {
        let document = song(&serde_json::json!([{ "tick": 0, "kind": { "bpm": 120.0 } }]));
        let bars = Bars::new(document.timing(), Tick::new(0), Tick::new(7000), 0);
        let seconds = SecondsPerRow::default();
        let rows = bars.rows(BarsPerRow::One, seconds);
        assert_eq!(rows.len(), 2);
        assert!((rows[1].from - 0.9).abs() < 1e-9 && (rows[1].to - 2.0).abs() < 1e-9);
        assert_eq!(bars.rows(BarsPerRow::Half, seconds).len(), 4);
        assert_eq!(bars.rows(BarsPerRow::Two, seconds).len(), 1);
        let row = Row {
            from: 0.9,
            start: 1.0,
            to: 2.0,
            rect: Rect::from_min_size(pos2(0.0, 0.0), vec2(110.0, ROW)),
        };
        assert!((row.x(&bars, Tick::new(3840)) - 10.0).abs() < 1e-3);
        assert_eq!(row.tick_at(&bars, 60.0), Tick::new(5760));
        assert!(row.holds(&bars, Tick::new(3456)));
        assert!(!row.holds(&bars, Tick::new(3400)));
        assert!((row.seconds_at(&bars, document.timing(), 60.0) - 3.0).abs() < 1e-6);
    }

    #[test]
    fn in_the_time_layout_a_row_is_seconds_and_only_the_grid_follows_the_tempo() {
        let rows_of = |bpm: f64| {
            let document = song(&serde_json::json!([{ "tick": 0, "kind": { "bpm": bpm } }]));
            let timing = document.timing();
            let bars = Bars::new(timing, Tick::new(0), Tick::new(7000), 0).laid_out(Layout::Time);
            let spans = bars.rows(BarsPerRow::One, SecondsPerRow::Two);
            let row = Row {
                from: spans[0].from,
                start: spans[0].start,
                to: spans[0].to,
                rect: Rect::from_min_size(pos2(0.0, 0.0), vec2(110.0, ROW)),
            };
            (
                spans.len(),
                row.seconds_at(&bars, timing, 60.0),
                row.x(&bars, Tick::new(1920)),
            )
        };
        let (count, moment, beat) = rows_of(120.0);
        assert_eq!(count, 2, "two bars of 2 s at 120 BPM");
        // Screen points are f32, so a moment read off one is good to about 1e-7 s.
        assert!((moment - 1.0).abs() < 1e-6);
        assert!((beat - 60.0).abs() < 1e-3);
        let (_, same_moment, later_beat) = rows_of(240.0);
        assert!(
            (same_moment - 1.0).abs() < 1e-6,
            "the audio stays where it was"
        );
        assert!(
            (later_beat - 35.0).abs() < 1e-3,
            "beat 2 moves to 0.5 s: {later_beat}"
        );
    }

    #[test]
    fn a_moment_during_a_stop_falls_on_the_stopped_tick_and_reads_back() {
        let document = song(&serde_json::json!([
            { "tick": 0, "kind": { "bpm": 60.0 } },
            { "tick": 960, "kind": { "stop": 1.0 } }
        ]));
        let timing = document.timing();
        assert!((tick_at_seconds(timing, 0.5) - 480.0).abs() < 1e-6);
        let paused = tick_at_seconds(timing, 1.5);
        assert!((paused.floor() - 960.0).abs() < f64::EPSILON, "{paused}");
        assert!((seconds_at_tick(timing, paused) - 1.5).abs() < 1e-9);
        assert!((tick_at_seconds(timing, 3.0) - 1920.0).abs() < 1e-6);
    }

    #[test]
    fn an_empty_stretch_has_no_rows_and_no_measure_to_find() {
        let document = song(&serde_json::json!([{ "tick": 0, "kind": { "bpm": 120.0 } }]));
        let bars = Bars::new(document.timing(), Tick::new(3840), Tick::new(0), 0);
        assert!(bars.measures().is_empty());
        assert!(bars.rows(BarsPerRow::One, SecondsPerRow::One).is_empty());
        assert!(bars.measure_at(0.0).is_none());
        assert!(bars.bar_of(0.0).abs() < f64::EPSILON);
        assert!(bars.tick_at(0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_one_beat_bar_and_negative_ticks_keep_their_bar_positions() {
        let document = song(&serde_json::json!([
            { "tick": 0, "kind": { "bpm": 120.0 } },
            { "tick": 0, "kind": { "time_signature": [1, 4] } }
        ]));
        let bars = Bars::new(document.timing(), Tick::new(-960), Tick::new(1920), 0);
        assert!((bars.bar_of(-480.0) + 0.125).abs() < 1e-9);
        assert!((bars.tick_at(-0.125) + 480.0).abs() < 1e-9);
        assert!((bars.bar_of(1440.0) - 1.5).abs() < 1e-9);
        assert!((bars.tick_at(1.5) - 1440.0).abs() < 1e-9);
    }

    #[test]
    fn a_delay_and_a_stop_keep_audio_time_monotonic_across_their_ticks() {
        let document = song(&serde_json::json!([
            { "tick": 0, "kind": { "bpm": 60.0 } },
            { "tick": 960, "kind": { "delay": 0.5 } },
            { "tick": 1920, "kind": { "stop": 0.5 } }
        ]));
        let timing = document.timing();
        for seconds in [0.5, 1.25, 1.75, 2.25, 2.75, 3.25] {
            let tick = tick_at_seconds(timing, seconds);
            assert!(
                (seconds_at_tick(timing, tick) - seconds).abs() < 1e-9,
                "{seconds}: {tick}"
            );
        }
        assert!(tick_at_seconds(timing, 1.25) < tick_at_seconds(timing, 1.75));
    }

    proptest! {
        #![proptest_config(crate::proptest_config())]

        #[test]
        fn rows_cover_their_own_span_without_gaps_for_odd_meters_and_all_layouts(
            numerator in 1_u16..=11,
            start_bar in 0_i32..=6,
            count in 1_i32..=12,
            layout in prop_oneof![Just(Layout::Bars), Just(Layout::Time)],
            bars_per in prop::sample::select(BarsPerRow::ALL.to_vec()),
            seconds_per in prop::sample::select(SecondsPerRow::ALL.to_vec()),
        ) {
            let document = song(&serde_json::json!([
                { "tick": 0, "kind": { "bpm": 120.0 } },
                { "tick": 0, "kind": { "time_signature": [numerator, 8] } }
            ]));
            let bar_ticks = i32::from(numerator).saturating_mul(480);
            let from = Tick::new(start_bar.saturating_mul(bar_ticks));
            let to = Tick::new(start_bar.saturating_add(count).saturating_mul(bar_ticks).saturating_sub(1));
            let spans = Bars::new(document.timing(), from, to, 0)
                .laid_out(layout).rows(bars_per, seconds_per);
            prop_assert!(!spans.is_empty());
            for span in &spans {
                prop_assert!(span.from < span.start && span.start < span.to);
                prop_assert!((span.start - span.from - (span.to - span.start) * LEAD_IN).abs() < 1e-9);
            }
            for pair in spans.windows(2) {
                prop_assert!(pair.first().zip(pair.get(1)).is_some_and(|(first, second)| (first.to - second.start).abs() < 1e-9));
            }
        }
    }
}
