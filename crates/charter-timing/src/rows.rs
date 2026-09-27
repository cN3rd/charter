//! The rows of the timing window: the song laid out by bar position, a few bars a row with a
//! little of the bar before shown ahead of each downbeat, and everything drawn over a row.

use charter_audio::{Onset, Peaks};
use charter_core::{Measure, Tick, Timing, Valid};
use charter_style as style;
use egui::{Align2, Color32, FontId, Pos2, Rect, Shape, Stroke, pos2, vec2};

use crate::accuracy::{Residual, SATURATION_MS};
use crate::grid::{Grid, Line};

/// How tall a row is, its residual strip included.
pub const ROW: f32 = 76.0;
/// How tall the strip under a row that shows the residuals is.
pub const STRIP: f32 = 6.0;
/// How wide the row headers down the left are.
pub const GUTTER: f32 = 56.0;
/// How far from an anchor's line, in points, a press still takes it.
pub const GRAB: f32 = 6.0;
/// How much of the bar before each row starts shows ahead of its downbeat, so a pickup is seen.
pub const LEAD_IN: f64 = 0.10;

/// How many bars a row holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BarsPerRow {
    Half,
    #[default]
    One,
    Two,
    Four,
}

impl BarsPerRow {
    pub const ALL: [Self; 4] = [Self::Half, Self::One, Self::Two, Self::Four];

    #[must_use]
    pub fn bars(self) -> f64 {
        match self {
            Self::Half => 0.5,
            Self::One => 1.0,
            Self::Two => 2.0,
            Self::Four => 4.0,
        }
    }

    #[must_use]
    pub fn name(self) -> &'static str {
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
pub enum SecondsPerRow {
    One,
    #[default]
    Two,
    Four,
    Eight,
}

impl SecondsPerRow {
    pub const ALL: [Self; 4] = [Self::One, Self::Two, Self::Four, Self::Eight];

    #[must_use]
    pub fn seconds(self) -> f64 {
        match self {
            Self::One => 1.0,
            Self::Two => 2.0,
            Self::Four => 4.0,
            Self::Eight => 8.0,
        }
    }

    #[must_use]
    pub fn name(self) -> &'static str {
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

/// The measures of a stretch of the song, by number, for turning ticks into bar positions and
/// back: a bar position counts the bar and how far through it, so a correct map lines beats up
/// down the rows whatever the tempo. The rows lay ticks out by a position that is the bar
/// position in the bar layout and the audio time in the time layout.
#[derive(Clone, Debug)]
pub struct Bars<'a> {
    measures: Vec<Measure>,
    timing: Valid<'a, Timing>,
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

    #[must_use]
    pub fn layout(&self) -> Layout {
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
        #[allow(clippy::cast_precision_loss)] // bar numbers of a song
        let number = measure.number as f64;
        number + (tick - start(measure)) / length(measure)
    }

    /// The tick, maybe between ticks, at bar position `bar`.
    #[must_use]
    pub fn tick_at(&self, bar: f64) -> f64 {
        let (Some(first), Some(last)) = (self.measures.first(), self.measures.last()) else {
            return 0.0;
        };
        #[allow(clippy::cast_possible_truncation)] // a bar number of a song
        let number = bar.floor() as i64;
        let measure = usize::try_from(number - first.number)
            .ok()
            .and_then(|at| self.measures.get(at))
            .unwrap_or(if number < first.number { first } else { last });
        #[allow(clippy::cast_precision_loss)]
        let into = bar - measure.number as f64;
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
        let (from, to, per) = match self.layout {
            #[allow(clippy::cast_precision_loss)] // bar numbers of a song
            Layout::Bars => (first.number as f64, last.number as f64 + 1.0, per.bars()),
            Layout::Time => {
                let per = seconds.seconds();
                let from = (seconds_at_tick(self.timing, start(first)) / per).floor() * per;
                (from, seconds_at_tick(self.timing, end), per)
            }
        };
        let mut rows = Vec::new();
        let mut at = from;
        while at < to - 1e-9 {
            rows.push(Span {
                from: at - LEAD_IN * per,
                start: at,
                to: at + per,
            });
            at += per;
        }
        rows
    }
}

/// The stretch a row spans, in positions: from where it starts showing, through where its own
/// part starts, to where it ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub from: f64,
    pub start: f64,
    pub to: f64,
}

/// The tick, maybe between ticks, at `seconds` into the audio: the earliest of a paused stretch.
#[must_use]
pub fn tick_at_seconds(timing: Valid<'_, Timing>, seconds: f64) -> f64 {
    let tick = timing.latest_tick_by(seconds);
    let after = Tick::new(i32::try_from(i64::from(tick) + 1).unwrap_or(i32::MAX));
    let (at, next) = (
        timing.time_at(tick).seconds(),
        timing.time_at(after).seconds(),
    );
    let within = if next > at {
        (seconds - at) / (next - at)
    } else {
        0.0
    };
    #[allow(clippy::cast_precision_loss)] // ticks of a song
    let whole = i64::from(tick) as f64;
    whole + within.clamp(0.0, 1.0)
}

fn start(measure: &Measure) -> f64 {
    #[allow(clippy::cast_precision_loss)] // ticks of a song
    let at = i64::from(measure.start) as f64;
    at
}

fn length(measure: &Measure) -> f64 {
    f64::from(u32::from(measure.length))
}

/// A row on screen: the positions it spans, and where.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Row {
    pub from: f64,
    /// Where the row's own part starts, after the lead-in from before it.
    pub start: f64,
    pub to: f64,
    /// Where the audio is drawn, the residual strip under it.
    pub rect: Rect,
}

impl Row {
    /// Where the position `position` lies across the row.
    #[must_use]
    pub fn x_of(&self, position: f64) -> f32 {
        #[allow(clippy::cast_possible_truncation)] // a fraction of a row
        let across = ((position - self.from) / (self.to - self.from)) as f32;
        self.rect.left() + across * self.rect.width()
    }

    /// The position at `x`.
    #[must_use]
    pub fn position_at(&self, x: f32) -> f64 {
        let across = f64::from((x - self.rect.left()) / self.rect.width());
        self.from + across * (self.to - self.from)
    }

    #[must_use]
    pub fn x(&self, bars: &Bars, tick: Tick) -> f32 {
        #[allow(clippy::cast_precision_loss)]
        let at = i64::from(tick) as f64;
        self.x_of(bars.position(at))
    }

    /// The tick nearest `x`.
    #[must_use]
    pub fn tick_at(&self, bars: &Bars, x: f32) -> Tick {
        let tick = bars.tick_at_position(self.position_at(x)).round();
        #[allow(clippy::cast_possible_truncation)] // clamped to the ticks a document holds
        let tick = tick.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32;
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
        #[allow(clippy::cast_precision_loss)]
        let position = bars.position(i64::from(tick) as f64);
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
pub fn seconds_at_tick(timing: Valid<'_, Timing>, tick: f64) -> f64 {
    let below = tick.floor();
    #[allow(clippy::cast_possible_truncation)] // clamped to the ticks a document holds
    let whole = below.clamp(f64::from(i32::MIN), f64::from(i32::MAX - 1)) as i32;
    let (at, next) = (
        timing.time_at(Tick::new(whole)).seconds(),
        timing.time_at(Tick::new(whole + 1)).seconds(),
    );
    at + (next - at) * (tick - below)
}

/// How an anchor is drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct Anchor {
    pub tick: Tick,
    /// The tempo from it on.
    pub bpm: String,
    pub focused: bool,
    pub selected: bool,
    /// How strongly it pulses, from 0 to 1, when an edit ran into it.
    pub pulse: f32,
}

/// Everything a row draws.
#[derive(Debug)]
pub struct Scene<'a> {
    pub timing: Valid<'a, Timing>,
    pub bars: &'a Bars<'a>,
    pub grid: Grid,
    pub peaks: Option<&'a Peaks>,
    pub onsets: &'a [Onset],
    pub sensitivity: f32,
    pub residuals: &'a [Residual],
    pub anchors: &'a [Anchor],
    pub cursor: Tick,
    /// Where a drag would drop, and whether it is on a snap target.
    pub ghost: Option<(f32, bool)>,
    /// The bar positions a dragged anchor may reach; the rest is dimmed.
    pub reach: Option<(f64, f64)>,
}

/// Draws `row` of `scene` with `painter`.
pub fn draw(painter: &egui::Painter, row: &Row, scene: &Scene<'_>) {
    let rect = row.rect;
    let audio = Rect::from_min_max(rect.min, pos2(rect.right(), rect.bottom() - STRIP));
    painter.rect_filled(audio, 2.0, style::WAVE_BG);
    if let Some(peaks) = scene.peaks {
        waveform(painter, row, audio, scene, peaks);
    }
    grid(painter, row, audio, scene);
    onsets(painter, row, audio, scene);
    strip(painter, row, scene);
    if let Some((from, to)) = scene.reach {
        dim(painter, row, audio, from, to);
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
    if let Some((x, snapped)) = scene.ghost
        && audio.x_range().contains(x)
    {
        painter.vline(x, audio.y_range(), Stroke::new(1.0, style::ACCENT));
        let centre = pos2(x, audio.top() + 7.0);
        let stroke = Stroke::new(1.5, style::ACCENT);
        if snapped {
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
pub fn header(
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
                Stroke::new(1.0, WAVE),
            );
        }
        x += 1.0;
    }
}

/// The waveform a touch brighter than the notefield's, since it is all this window shows.
const WAVE: Color32 = Color32::from_rgb(118, 158, 210);

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
        let mut beat = 0;
        for (tick, line) in scene.grid.lines(measure, resolution) {
            // Beats count from the bar's downbeat, whether or not this row shows it.
            if matches!(line, Line::Downbeat | Line::Beat) {
                beat += 1;
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
pub fn x_at_seconds(row: &Row, scene: &Scene<'_>, seconds: f64) -> f32 {
    match scene.bars.layout() {
        Layout::Time => row.x_of(seconds),
        Layout::Bars => row.x_of(scene.bars.bar_of(tick_at_seconds(scene.timing, seconds))),
    }
}

/// The residual strip: each onset on the grid as a bar in the strip under the row, blue when the
/// audio comes early and orange when late, fuller the farther off, full at the saturation.
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
        #[allow(clippy::cast_possible_truncation)] // a share from 0 to 1
        let size = (residual.ms.abs() / SATURATION_MS).min(1.0) as f32;
        let colour = if residual.ms < 0.0 {
            style::RESIDUAL_EARLY
        } else {
            style::RESIDUAL_LATE
        };
        let bar = Rect::from_center_size(pos2(x, strip.center().y), vec2(3.0, STRIP));
        painter.rect_filled(bar, 0.0, colour.gamma_multiply(0.2 + 0.8 * size));
    }
}

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
/// on. Focused, it draws brighter and thicker; selected, its flag is filled.
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
        colour.lerp_to_gamma(style::WARN, anchor.pulse)
    } else {
        colour
    };
    let width = if anchor.focused { 3.0 } else { 2.0 } + 2.0 * anchor.pulse;
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
pub fn row_at(rows: &[Row], pos: Pos2) -> Option<&Row> {
    rows.iter()
        .find(|row| row.whole().y_range().contains(pos.y))
}

#[cfg(test)]
mod tests {
    use charter_core::ValidDocument;

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
}
