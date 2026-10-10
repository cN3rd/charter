//! The timing window measures how well the map fits the audio.
//!
//! What the timing window measures: how far each onset near a grid line is from it, how well a
//! stretch of the map fits the audio, a least-squares fit that makes it fit better, and the
//! places a map most likely went wrong. All of it works on the song as it stands and the onsets
//! the host found, and draws nothing.

use std::ops::RangeInclusive;

use charter_audio::Onset;
use charter_core::{Bpm, ResolvedTime, Tick, Timing, TimingKind, Valid};
use charter_editor::{Pin, TimedSpan};

use crate::grid::Grid;

/// How far from where it was asked, in seconds, a moment snaps to an onset: roughly where two
/// hits stop sounding like one.
pub const SNAP: f64 = 0.025;
/// How close to a grid line, as a share of the step there, an onset has to be to be measured
/// against it; farther, it is off the grid on purpose, such as a syncopation.
pub const ON_GRID: f64 = 0.30;
/// The residual at which the strip is as strong as it gets, in milliseconds.
pub const SATURATION_MS: f64 = 20.0;
/// The median residual at which a bar is worth a look, in milliseconds.
pub const BAR_WARNING_MS: f64 = 5.0;
/// How much two neighbouring tempos may differ, as a share, before it looks like a mistake.
pub const TEMPO_JUMP: f64 = 0.03;
/// How far from its onset an anchor may be before it is flagged, in milliseconds.
pub const ANCHOR_WARNING_MS: f64 = 2.0;
/// The residual under which an anchor counts as on its onset, in milliseconds.
pub const ON_ONSET_MS: f64 = 1.0;

/// An onset near a grid line, and how far from it: positive when the audio comes late.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Residual {
    /// Audio position of the onset.
    pub seconds: f64,
    /// Detected onset strength.
    pub strength: f32,
    /// Nearest grid tick to the onset.
    pub line: Tick,
    /// Signed onset offset from that tick, in milliseconds.
    pub ms: f64,
}

/// The onsets at least `sensitivity` strong, for the grid `grid` over `timing`, with how far each
/// is from its line; onsets far from every line are left out.
#[must_use]
pub fn residuals(
    timing: Valid<'_, Timing>,
    grid: Grid,
    onsets: &[Onset],
    sensitivity: f32,
) -> Vec<Residual> {
    onsets
        .iter()
        .filter(|onset| onset.strength >= sensitivity)
        .filter_map(|onset| {
            let near = grid.nearest_in_time(timing, onset.seconds)?;
            let off = onset.seconds - near.seconds;
            (off.abs() <= ON_GRID * near.step_seconds).then_some(Residual {
                seconds: onset.seconds,
                strength: onset.strength,
                line: near.line,
                ms: off * 1000.0,
            })
        })
        .collect()
}

/// The strongest onset at least `sensitivity` strong within [`SNAP`] of `seconds`, where a
/// moment asked for snaps to.
#[must_use]
pub fn snap(onsets: &[Onset], sensitivity: f32, seconds: f64) -> Option<f64> {
    within(onsets, sensitivity, seconds)
        .max_by(|a, b| a.strength.total_cmp(&b.strength))
        .map(|onset| onset.seconds)
}

/// The onset at least `sensitivity` strong nearest `seconds`, within [`SNAP`] of it.
#[must_use]
pub fn nearest_onset(onsets: &[Onset], sensitivity: f32, seconds: f64) -> Option<f64> {
    within(onsets, sensitivity, seconds)
        .min_by(|a, b| {
            (a.seconds - seconds)
                .abs()
                .total_cmp(&(b.seconds - seconds).abs())
        })
        .map(|onset| onset.seconds)
}

/// Onsets within [`SNAP`] seconds of `seconds` that meet the strength threshold.
fn within(onsets: &[Onset], sensitivity: f32, seconds: f64) -> impl Iterator<Item = &Onset> {
    let from = onsets.partition_point(|onset| onset.seconds < seconds - SNAP);
    onsets
        .get(from..)
        .unwrap_or_default()
        .iter()
        .take_while(move |onset| onset.seconds <= seconds + SNAP)
        .filter(move |onset| onset.strength >= sensitivity)
}

/// The median of how far `values` are from nothing, if there are any.
#[must_use]
pub fn median_abs(values: impl IntoIterator<Item = f64>) -> Option<f64> {
    let mut sizes: Vec<f64> = values.into_iter().map(f64::abs).collect();
    sizes.sort_by(f64::total_cmp);
    let middle = sizes.len().div_euclid(2);
    match sizes.len() {
        0 => None,
        count if count.is_multiple_of(2) => {
            Some((sizes.get(middle.saturating_sub(1))? + sizes.get(middle)?) / 2.0)
        }
        _ => sizes.get(middle).copied(),
    }
}

/// How well a stretch of the map fits the audio.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    /// The root mean square of the residuals in it, in milliseconds.
    pub error_ms: Option<f64>,
    /// The share of its grid lines that have an onset: low where the song is quiet or rubato,
    /// and a fit there is weak.
    pub coverage: Option<f64>,
    /// How many residuals it holds.
    pub onsets: usize,
}

/// How well the ticks from `from` up to `to`, or to the last residual when there is no `to`, fit
/// the audio.
#[must_use]
pub fn metrics(
    timing: Valid<'_, Timing>,
    grid: Grid,
    residuals: &[Residual],
    (from, to): (Tick, Option<Tick>),
) -> Metrics {
    let inside: Vec<&Residual> = residuals
        .iter()
        .filter(|residual| residual.line >= from && to.is_none_or(|to| residual.line < to))
        .collect();
    let to = to.or_else(|| inside.iter().map(|residual| residual.line).max());
    let error_ms = rms(inside.iter().map(|residual| residual.ms));
    let mut matched: Vec<Tick> = inside.iter().map(|residual| residual.line).collect();
    matched.sort_unstable();
    matched.dedup();
    let lines = to.map_or(0, |to| lines_between(timing, grid, from, to));
    let coverage = (lines > 0).then(|| {
        crate::rows::double_from_usize(matched.len()) / crate::rows::double_from_usize(lines)
    });
    Metrics {
        error_ms,
        coverage: coverage.map(|share| share.min(1.0)),
        onsets: inside.len(),
    }
}

/// How many lines of `grid` lie from `from` up to `to`.
fn lines_between(timing: Valid<'_, Timing>, grid: Grid, from: Tick, to: Tick) -> usize {
    let resolution = timing.ticks_per_beat();
    timing
        .measures_from(from)
        .take_while(|measure| measure.start < to)
        .map(|measure| {
            grid.lines(&measure, resolution)
                .iter()
                .filter(|(line, _)| *line >= from && *line < to)
                .count()
        })
        .sum()
}

/// Root mean square of `values`, or `None` for an empty iterator.
fn rms(values: impl Iterator<Item = f64>) -> Option<f64> {
    let (count, sum) = values.fold((0_u32, 0.0), |(count, sum), value| {
        (count.saturating_add(1), sum + value * value)
    });
    (count > 0).then(|| (sum / f64::from(count)).sqrt())
}

/// A fit of anchors to the audio, to look at before taking.
#[derive(Clone, Debug, PartialEq)]
pub struct Fitted {
    /// Fitted anchors in tick order.
    pub pins: Vec<Pin>,
    /// Fitted tempo following the last anchor.
    pub tail: Bpm,
    /// The error of the stretch fitted before the fit and after, in milliseconds.
    pub before_ms: f64,
    /// Residual error after fitting, in milliseconds.
    pub after_ms: f64,
}

/// Fits anchors to audio onsets by least squares.
///
/// A least-squares fit of the anchors `range` indexes among `pins` to the `residuals`, the
/// tempo after the last anchor as well when the range reaches it: each anchor's moment moves so
/// the grid lines of the segments it bounds pass as near their onsets as they can. With
/// `half_bpm`, the tempos inside the range then round to the nearest half BPM, from its first
/// anchor on. None when there is nothing in the range to fit to, or the fit would put anchors
/// out of order.
#[must_use]
#[expect(
    clippy::indexing_slicing,
    reason = "indices run over the vectors' own lengths"
)]
pub fn fit(
    timing: Valid<'_, Timing>,
    (pins, tail): (&[Pin], Bpm),
    range: RangeInclusive<usize>,
    residuals: &[Residual],
    half_bpm: bool,
) -> Option<Fitted> {
    let (first, last) = (*range.start(), *range.end());
    let last_pin = pins.len().checked_sub(1)?;
    if first > last || last > last_pin {
        return None;
    }
    let ticks_per_beat = timing.ticks_per_beat();
    let with_tail = last == last_pin;
    let mut points: Vec<_> = pins
        .iter()
        .map(|pin| (pin.tick, pin.seconds.seconds()))
        .collect();
    // The tempo after the last anchor fits as the moment of a point past it, where its onsets
    // end, so it is a moment like the others.
    if with_tail {
        let pin = pins.get(last_pin)?;
        let far = residuals
            .iter()
            .filter(|residual| residual.line > pin.tick)
            .map(|residual| i64::from(residual.line))
            .fold(
                i64::from(pin.tick)
                    .saturating_add(4_i64.saturating_mul(i64::from(u16::from(ticks_per_beat)))),
                i64::max,
            )
            .min(i64::from(i32::MAX));
        let far = Tick::new(i32::try_from(far).ok()?);
        let span = TimedSpan::between(timing, pin.tick, far)?;
        points.push((
            far,
            pin.seconds.seconds() + span.at_bpm(tail, ticks_per_beat),
        ));
    }
    // Every point by index: the anchors, then one past the last when fitting the tail.
    let free: Vec<usize> = (first..=last)
        .chain(with_tail.then(|| points.len().saturating_sub(1)))
        .collect();
    let variable = |point: usize| free.iter().position(|free| *free == point);
    // The observations: each residual in a segment one of whose ends moves.
    let touched = first.saturating_sub(1)..=last;
    let mut rows: Vec<(Vec<(usize, f64)>, f64)> = Vec::new();
    for residual in residuals {
        let Some(segment) = (0..points.len().saturating_sub(1)).find(|&at| {
            let end = points[at.saturating_add(1)].0;
            points[at].0 <= residual.line
                && (residual.line < end || (with_tail && at == last_pin && residual.line == end))
        }) else {
            continue;
        };
        if !touched.contains(&segment) {
            continue;
        }
        let (start, end) = (points[segment], points[segment.saturating_add(1)]);
        let whole = TimedSpan::between(timing, start.0, end.0)?;
        let part = TimedSpan::between(timing, start.0, residual.line)?;
        let share = part.fraction_of(whole)?;
        let mut terms = Vec::new();
        let mut known = residual.seconds - part.paused_seconds() + share * whole.paused_seconds();
        for (point, weight) in [(segment, 1.0 - share), (segment.saturating_add(1), share)] {
            match variable(point) {
                Some(at) => terms.push((at, weight)),
                None => known -= weight * points[point].1,
            }
        }
        if !terms.is_empty() {
            rows.push((terms, known));
        }
    }
    if rows.is_empty() {
        return None;
    }
    let current: Vec<f64> = free.iter().map(|&point| points[point].1).collect();
    let before = rows_error(&rows, &current);
    let mut solved = least_squares(&rows, &current)?;
    if half_bpm {
        solved = to_half_bpm(timing, &points, &free, &solved)?;
    }
    let after = rows_error(&rows, &solved);
    let mut fitted = points;
    for (point, seconds) in free.iter().zip(&solved) {
        fitted[*point].1 = *seconds;
    }
    let ordered = fitted.windows(2).all(|pair| pair[1].1 > pair[0].1);
    if !ordered {
        return None;
    }
    let tail = if with_tail {
        let (end_tick, end_seconds) = fitted.pop()?;
        let (last_tick, last_seconds) = *fitted.last()?;
        let span = TimedSpan::between(timing, last_tick, end_tick)?;
        let active_seconds = end_seconds - last_seconds - span.paused_seconds();
        let tempo = 60.0 * span.timed_beats(ticks_per_beat) / active_seconds;
        Bpm::nearest(tempo).ok()?
    } else {
        tail
    };
    Some(Fitted {
        pins: resolved_pins(pins, fitted)?,
        tail,
        before_ms: before * 1000.0,
        after_ms: after * 1000.0,
    })
}

/// Keeps fitted moments only when they can be carried as audio times.
fn resolved_pins(pins: &[Pin], fitted: Vec<(Tick, f64)>) -> Option<Vec<Pin>> {
    pins.iter()
        .zip(fitted)
        .map(|(pin, (_, seconds))| {
            Some(Pin {
                tick: pin.tick,
                seconds: ResolvedTime::from_seconds(seconds)?,
            })
        })
        .collect()
}

/// The root mean square of how far `rows` miss with the free moments at `values`, in seconds.
#[expect(
    clippy::indexing_slicing,
    reason = "indices run over the vectors' own lengths"
)]
fn rows_error(rows: &[(Vec<(usize, f64)>, f64)], values: &[f64]) -> f64 {
    rms(rows.iter().map(|(terms, known)| {
        let predicted: f64 = terms.iter().map(|(at, weight)| weight * values[*at]).sum();
        predicted - known
    }))
    .unwrap_or(0.0)
}

/// The moments that make `rows` miss by as little as can be, pulled a little toward `current`
/// so a moment no onset constrains stays where it is.
#[expect(
    clippy::indexing_slicing,
    reason = "indices run over the vectors' own lengths"
)]
fn least_squares(rows: &[(Vec<(usize, f64)>, f64)], current: &[f64]) -> Option<Vec<f64>> {
    const PULL: f64 = 1e-3;
    let size = current.len();
    let mut matrix = vec![vec![0.0; size.saturating_add(1)]; size];
    for (terms, known) in rows {
        for &(row, a) in terms {
            for &(column, b) in terms {
                matrix[row][column] += a * b;
            }
            matrix[row][size] += a * known;
        }
    }
    for (at, (row, value)) in matrix.iter_mut().zip(current).enumerate() {
        row[at] += PULL * PULL;
        row[size] += PULL * PULL * value;
    }
    // Gaussian elimination with the largest pivot in each column.
    for column in 0..size {
        let pivot = (column..size).max_by(|a, b| {
            matrix[*a][column]
                .abs()
                .total_cmp(&matrix[*b][column].abs())
        })?;
        matrix.swap(column, pivot);
        let lead = matrix[column][column];
        if lead.abs() < 1e-18 {
            return None;
        }
        let (before, from_pivot) = matrix.split_at_mut(column);
        let (pivot_row, after) = from_pivot.split_first_mut()?;
        for row in before.iter_mut().chain(after.iter_mut()) {
            let factor = row[column] / lead;
            for (value, above) in row.iter_mut().zip(pivot_row.iter()).skip(column) {
                *value -= factor * above;
            }
        }
    }
    Some(
        (0..size)
            .map(|at| matrix[at][size] / matrix[at][at])
            .collect(),
    )
}

/// `solved` with the tempo of each segment between two free points rounded to the nearest half
/// BPM, the moments after the first following from the rounded tempos.
#[expect(
    clippy::indexing_slicing,
    reason = "indices run over the vectors' own lengths"
)]
fn to_half_bpm(
    timing: Valid<'_, Timing>,
    points: &[(Tick, f64)],
    free: &[usize],
    solved: &[f64],
) -> Option<Vec<f64>> {
    let mut seconds = solved.to_vec();
    for at in 1..free.len() {
        let (before, point) = (free[at.saturating_sub(1)], free[at]);
        if point != before.saturating_add(1) {
            continue;
        }
        let span = TimedSpan::between(timing, points[before].0, points[point].0)?;
        let beats = span.timed_beats(timing.ticks_per_beat());
        let tempo =
            60.0 * beats / (seconds[at] - seconds[at.saturating_sub(1)] - span.paused_seconds());
        let rounded = (tempo * 2.0).round() / 2.0;
        if rounded > 0.0 {
            seconds[at] =
                seconds[at.saturating_sub(1)] + span.paused_seconds() + 60.0 * beats / rounded;
        }
    }
    Some(seconds)
}

/// Somewhere the map most likely went wrong.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Issue {
    /// The bar starting on `tick` misses its onsets by `median_ms` in the middle.
    Bar {
        /// First tick of the bar.
        tick: Tick,
        /// Bar number to show in the issue list.
        number: i64,
        /// Median absolute offset from its onsets, in milliseconds.
        median_ms: f64,
    },
    /// The segment from the anchor on `tick` is shorter than a beat.
    Short {
        /// Tick of the anchor beginning the short segment.
        tick: Tick,
    },
    /// The tempo from the anchor on `tick` differs from the one before by `percent`.
    Jump {
        /// Tick where the tempo changes.
        tick: Tick,
        /// Magnitude of the tempo change, as a percentage.
        percent: f64,
    },
    /// The anchor on `tick` is `ms` from the onset nearest it.
    Anchor {
        /// Tick pinned by the anchor.
        tick: Tick,
        /// Signed distance to the nearest onset, in milliseconds.
        ms: f64,
    },
    /// A time signature on `tick` without an anchor, so its downbeat is not pinned.
    Metre {
        /// Tick where the metre changes without an anchor.
        tick: Tick,
    },
}

impl Issue {
    /// Where in the song the issue is.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        match *self {
            Self::Bar { tick, .. }
            | Self::Short { tick }
            | Self::Jump { tick, .. }
            | Self::Anchor { tick, .. }
            | Self::Metre { tick } => tick,
        }
    }

    /// The issue in words.
    #[must_use]
    pub fn describe(&self, timing: Valid<'_, Timing>) -> String {
        let at = |tick: Tick| charter_style::clock(timing.time_at(tick).seconds());
        match *self {
            Self::Bar {
                number, median_ms, ..
            } => format!("Bar {number} is {median_ms:.1} ms off its onsets"),
            Self::Short { tick } => format!("The segment from {} is shorter than a beat", at(tick)),
            Self::Jump { tick, percent } => {
                format!("The tempo jumps {percent:.1}% at {}", at(tick))
            }
            Self::Anchor { tick, ms } => {
                format!(
                    "The anchor at {} is {:.1} ms from its onset",
                    at(tick),
                    ms.abs()
                )
            }
            Self::Metre { tick } => format!("The metre change at {} has no anchor", at(tick)),
        }
    }
}

/// Every issue of the map in `timing`, whose anchors are `pins`, against `residuals` and the
/// `onsets` at least `sensitivity` strong, in the order they come in the song.
#[must_use]
pub fn issues(
    timing: Valid<'_, Timing>,
    pins: &[Pin],
    residuals: &[Residual],
    (onsets, sensitivity): (&[Onset], f32),
) -> Vec<Issue> {
    let mut found = Vec::new();
    let mut by_bar: Vec<(i64, Tick, Vec<f64>)> = Vec::new();
    for residual in residuals {
        let Some(measure) = timing.measures_from(residual.line).next() else {
            continue;
        };
        match by_bar
            .iter_mut()
            .find(|(number, ..)| *number == measure.number)
        {
            Some((_, _, sizes)) => sizes.push(residual.ms),
            None => by_bar.push((measure.number, measure.start, vec![residual.ms])),
        }
    }
    for (number, tick, sizes) in by_bar {
        if let Some(median_ms) = median_abs(sizes).filter(|median| *median > BAR_WARNING_MS) {
            found.push(Issue::Bar {
                tick,
                number,
                median_ms,
            });
        }
    }
    let beat = i64::from(u16::from(timing.ticks_per_beat()));
    for pair in pins.windows(2) {
        if let [from, to] = pair
            && i64::from(to.tick).saturating_sub(i64::from(from.tick)) < beat
        {
            found.push(Issue::Short { tick: from.tick });
        }
    }
    let tempos = timing.tempos();
    for pair in tempos.windows(2) {
        if let [(_, before), (tick, after)] = pair {
            let change = (after / before - 1.0).abs();
            if change > TEMPO_JUMP {
                found.push(Issue::Jump {
                    tick: *tick,
                    percent: change * 100.0,
                });
            }
        }
    }
    for pin in pins {
        if let Some(onset) = nearest_onset(onsets, sensitivity, pin.seconds.seconds()) {
            let ms = (pin.seconds.seconds() - onset) * 1000.0;
            if ms.abs() > ANCHOR_WARNING_MS {
                found.push(Issue::Anchor { tick: pin.tick, ms });
            }
        }
    }
    for event in &timing.events {
        if matches!(event.kind, TimingKind::TimeSignature(_))
            && !pins.iter().any(|pin| pin.tick == event.tick)
        {
            found.push(Issue::Metre { tick: event.tick });
        }
    }
    found.sort_by_key(Issue::tick);
    found
}

#[cfg(test)]
mod tests {
    use charter_core::ValidDocument;
    use proptest::prelude::*;
    use serde_json::json;

    use super::*;

    fn song(offset: f64, events: &serde_json::Value) -> ValidDocument {
        let value = json!({
            "format_version": 0,
            "ruleset": { "id": "t", "version": "1" },
            "metadata": { "title": "t", "artist": "a", "audio": "a.ogg" },
            "timing": { "offset_seconds": offset, "events": events },
            "charts": []
        });
        ValidDocument::from_json(&serde_json::to_vec(&value).unwrap()).unwrap()
    }

    fn bpm(tick: i32, bpm: f64) -> serde_json::Value {
        json!({ "tick": tick, "kind": { "bpm": bpm } })
    }

    fn onset(seconds: f64) -> Onset {
        Onset {
            seconds,
            strength: 1.0,
        }
    }

    fn pins_of(document: &ValidDocument) -> Vec<Pin> {
        let timing = document.timing();
        timing
            .anchors
            .iter()
            .map(|anchor| Pin {
                tick: anchor.tick,
                seconds: timing.time_at(anchor.tick),
            })
            .collect()
    }

    #[test]
    fn a_non_finite_fit_cannot_become_an_anchor() {
        let document = song(0.0, &json!([bpm(0, 120.0)]));
        let pins = pins_of(&document);
        assert_eq!(
            resolved_pins(&pins, vec![(Tick::new(0), f64::INFINITY)]),
            None
        );
    }

    #[test]
    fn onsets_near_lines_are_measured_and_those_between_are_left_out() {
        let document = song(0.0, &json!([bpm(0, 120.0)]));
        let onsets = [onset(0.503), onset(0.75), onset(0.998)];
        let found = residuals(document.timing(), Grid::default(), &onsets, 0.0);
        assert_eq!(found.len(), 2);
        assert!((found[0].ms - 3.0).abs() < 1e-6);
        assert!((found[1].ms + 2.0).abs() < 1e-6);
        assert_eq!(found[1].line, Tick::new(1920));
    }

    #[test]
    fn a_moment_snaps_to_the_strongest_onset_within_25_ms_and_no_farther() {
        let onsets = [
            Onset {
                seconds: 0.99,
                strength: 0.4,
            },
            Onset {
                seconds: 1.01,
                strength: 0.9,
            },
            Onset {
                seconds: 1.5,
                strength: 1.0,
            },
        ];
        assert_eq!(snap(&onsets, 0.0, 1.0), Some(1.01));
        assert_eq!(snap(&onsets, 0.0, 1.46), None);
        assert_eq!(nearest_onset(&onsets, 0.0, 0.995), Some(0.99));
        assert_eq!(snap(&onsets, 0.95, 1.0), None);
    }

    /// Onsets on every beat of a steady tempo, each off by up to a millisecond either way.
    fn steady(bpm: f64, from: f64, beats: u32) -> Vec<Onset> {
        (0..beats)
            .map(|beat| {
                let jitter = f64::from(beat.saturating_mul(7919).rem_euclid(21)) / 10.0 - 1.0;
                onset(from + f64::from(beat) * 60.0 / bpm + jitter / 1000.0)
            })
            .collect()
    }

    #[test]
    fn a_fit_finds_a_steady_tempo_through_jittered_onsets() {
        let document = song(0.5, &json!([bpm(0, 138.0)]));
        let timing = document.timing();
        let onsets = steady(140.0, 0.5, 32);
        let found = residuals(timing, Grid::default(), &onsets, 0.0);
        let pins = pins_of(&document);
        let tail = Bpm::try_from(138.0).unwrap();
        let fitted = fit(timing, (&pins, tail), 0..=0, &found, false).unwrap();
        assert!(
            (f64::from(fitted.tail) - 140.0).abs() < 0.02,
            "{}",
            fitted.tail
        );
        assert!(fitted.after_ms <= 1.0, "{}", fitted.after_ms);
        assert!(fitted.before_ms > fitted.after_ms);
        let rounded = fit(timing, (&pins, tail), 0..=0, &found, true).unwrap();
        assert!((f64::from(rounded.tail) - 140.0).abs() < 0.02);
    }

    #[test]
    fn a_fit_between_two_anchors_moves_the_one_between_onto_its_onsets() {
        let document = song(
            0.0,
            &json!([bpm(0, 120.0), bpm(3840, 120.0), bpm(7680, 120.0)]),
        );
        let timing = document.timing();
        // The audio's first bar runs slow, so its second starts 100 ms late, and that bar runs
        // fast to catch up by the third.
        let onsets: Vec<Onset> = (0..8)
            .map(|beat| {
                let at = if beat < 4 {
                    f64::from(beat) * 0.525
                } else {
                    2.1 + f64::from(beat - 4) * 0.475
                };
                onset(at)
            })
            .collect();
        let found = residuals(timing, Grid::default(), &onsets, 0.0);
        let pins = pins_of(&document);
        let tail = Bpm::try_from(120.0).unwrap();
        let fitted = fit(timing, (&pins, tail), 1..=1, &found, false).unwrap();
        assert!(
            (fitted.pins[1].seconds.seconds() - 2.1).abs() < 0.01,
            "{:?}",
            fitted.pins
        );
        assert_eq!(fitted.pins[0], pins[0]);
        assert_eq!(fitted.pins[2], pins[2]);
    }

    fn anchored(seconds: f64, events: &serde_json::Value) -> ValidDocument {
        let value = json!({
            "format_version": 1,
            "ruleset": { "id": "t", "version": "1" },
            "metadata": { "title": "t", "artist": "a", "audio": "a.ogg" },
            "timing": {
                "anchors": [
                    { "tick": 0, "seconds": 0.0 },
                    { "tick": 3840, "seconds": seconds }
                ],
                "tempo": 120.0,
                "events": events
            }
        });
        ValidDocument::from_json(&serde_json::to_vec(&value).unwrap()).unwrap()
    }

    #[test]
    fn fitting_a_perfect_map_with_a_stop_leaves_its_anchors_in_place() {
        let document = anchored(2.5, &json!([{ "tick": 1920, "kind": { "stop": 0.5 } }]));
        let timing = document.timing();
        let onsets = [onset(0.5), onset(1.0), onset(2.0)];
        let found = residuals(timing, Grid::default(), &onsets, 0.0);
        assert_eq!(found.len(), 3);
        let fitted = fit(
            timing,
            (&pins_of(&document), Bpm::try_from(120.0).unwrap()),
            0..=0,
            &found,
            false,
        )
        .unwrap();
        assert!(fitted.before_ms.abs() < 1e-6, "{}", fitted.before_ms);
        assert!(
            fitted.pins[0].seconds.seconds().abs() < 1e-6,
            "{:?}",
            fitted.pins
        );
    }

    #[test]
    fn fitting_a_perfect_map_with_a_warp_leaves_its_anchors_in_place() {
        let document = anchored(1.5, &json!([{ "tick": 1920, "kind": { "warp": 960 } }]));
        let timing = document.timing();
        let found = [
            Residual {
                seconds: 0.5,
                strength: 1.0,
                line: Tick::new(960),
                ms: 0.0,
            },
            Residual {
                seconds: 1.0,
                strength: 1.0,
                line: Tick::new(2880),
                ms: 0.0,
            },
        ];
        let fitted = fit(
            timing,
            (&pins_of(&document), Bpm::try_from(120.0).unwrap()),
            0..=0,
            &found,
            false,
        )
        .unwrap();
        assert!(fitted.before_ms.abs() < 1e-6, "{}", fitted.before_ms);
        assert!(
            fitted.pins[0].seconds.seconds().abs() < 1e-6,
            "{:?}",
            fitted.pins
        );
    }

    #[test]
    fn fitting_the_farthest_tail_onset_includes_it() {
        let document = anchored(2.0, &json!([]));
        let residual = Residual {
            seconds: 5.85,
            strength: 1.0,
            line: Tick::new(10_000),
            ms: 641.666_666,
        };
        let fitted = fit(
            document.timing(),
            (&pins_of(&document), Bpm::try_from(120.0).unwrap()),
            1..=1,
            &[residual],
            false,
        )
        .expect("tail onset should be fitted");
        assert!(fitted.before_ms > 600.0, "{}", fitted.before_ms);
        assert!(fitted.after_ms < 1.0, "{}", fitted.after_ms);
    }

    #[test]
    fn fitting_before_the_last_tick_does_not_need_a_tail_point_past_it() {
        let value = json!({
            "format_version": 1,
            "ruleset": { "id": "t", "version": "1" },
            "metadata": { "title": "t", "artist": "a", "audio": "a.ogg" },
            "timing": {
                "anchors": [
                    { "tick": 0, "seconds": 0.0 },
                    { "tick": i32::MAX, "seconds": 1_118_481.066_145 }
                ],
                "tempo": 120.0,
                "events": []
            }
        });
        let document = ValidDocument::from_json(&serde_json::to_vec(&value).unwrap()).unwrap();
        let residual = Residual {
            seconds: 0.5,
            strength: 1.0,
            line: Tick::new(960),
            ms: 0.0,
        };
        assert!(
            fit(
                document.timing(),
                (&pins_of(&document), Bpm::try_from(120.0).unwrap()),
                0..=0,
                &[residual],
                false,
            )
            .is_some()
        );
    }

    #[test]
    fn a_map_off_by_eight_ms_at_one_anchor_flags_the_bar_after_and_the_anchor() {
        let events = json!([bpm(0, 119.521_912)]);
        let document = song(0.0, &events);
        let timing = document.timing();
        let onsets = steady(120.0, 0.0, 16);
        let found = residuals(timing, Grid::default(), &onsets, 0.0);
        let pins = pins_of(&document);
        let issues = issues(timing, &pins, &found, (&onsets, 0.0));
        assert!(issues.iter().any(|issue| matches!(issue, Issue::Bar { number: 1, median_ms, .. } if (*median_ms - 10.9).abs() < 0.1)), "{issues:?}");
        assert!(
            !issues
                .iter()
                .any(|issue| matches!(issue, Issue::Bar { number: 0, .. }))
        );
        let bar = found
            .iter()
            .filter(|residual| residual.line >= Tick::new(3840));
        assert!(bar.clone().all(|residual| residual.ms < -7.0));
        let metrics = metrics(timing, Grid::default(), &found, (Tick::new(3840), None));
        assert!(metrics.error_ms.unwrap() > 7.0);
        assert!((metrics.coverage.unwrap() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn short_segments_tempo_jumps_and_unpinned_metres_are_issues() {
        let meter = json!({ "tick": 7680, "kind": { "time_signature": [3, 4] } });
        let document = song(0.0, &json!([bpm(0, 120.0), bpm(480, 130.0), meter]));
        let timing = document.timing();
        let pins = pins_of(&document);
        let issues = issues(timing, &pins, &[], (&[], 0.0));
        assert!(matches!(issues[0], Issue::Short { tick } if tick == Tick::new(0)));
        assert!(matches!(issues[1], Issue::Jump { tick, .. } if tick == Tick::new(480)));
        assert!(matches!(issues[2], Issue::Metre { tick } if tick == Tick::new(7680)));
        assert_eq!(
            issues[2].describe(timing),
            "The metre change at 00:03.712 has no anchor"
        );
    }

    #[test]
    fn the_median_is_of_sizes_and_nothing_has_none() {
        assert_eq!(median_abs([-3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median_abs([-4.0, 1.0, 2.0, 3.0]), Some(2.5));
        assert_eq!(median_abs([]), None);
    }

    #[test]
    fn empty_onsets_have_no_residuals_fit_or_coverage() {
        let document = song(0.0, &json!([bpm(0, 120.0)]));
        let timing = document.timing();
        let grid = Grid::default();
        assert!(residuals(timing, grid, &[], 0.0).is_empty());
        assert_eq!(snap(&[], 0.0, 1.0), None);
        assert_eq!(nearest_onset(&[], 0.0, 1.0), None);
        assert_eq!(
            metrics(timing, grid, &[], (Tick::new(0), None)),
            Metrics {
                error_ms: None,
                coverage: None,
                onsets: 0,
            }
        );
        assert!(
            fit(
                timing,
                (&pins_of(&document), Bpm::try_from(120.0).unwrap()),
                0..=0,
                &[],
                false
            )
            .is_none()
        );
    }

    #[test]
    fn fitting_refuses_missing_pins_and_ranges_outside_the_anchor_list() {
        let document = song(0.0, &json!([bpm(0, 120.0)]));
        let timing = document.timing();
        let pins = pins_of(&document);
        let tail = Bpm::try_from(120.0).unwrap();
        let found = [Residual {
            seconds: 0.5,
            strength: 1.0,
            line: Tick::new(960),
            ms: 0.0,
        }];
        for (candidate_pins, range) in [
            (&[][..], 0..=0),
            (pins.as_slice(), 1..=1),
            (pins.as_slice(), 1..=pins.len() - 1),
        ] {
            assert!(fit(timing, (candidate_pins, tail), range, &found, false).is_none());
        }
    }

    #[test]
    fn issue_descriptions_identify_each_kind_of_problem() {
        let document = song(0.0, &json!([bpm(0, 120.0)]));
        let timing = document.timing();
        assert_eq!(
            Issue::Bar {
                tick: Tick::new(0),
                number: 2,
                median_ms: 7.5
            }
            .describe(timing),
            "Bar 2 is 7.5 ms off its onsets"
        );
        assert_eq!(
            Issue::Short { tick: Tick::new(0) }.describe(timing),
            "The segment from 00:00.000 is shorter than a beat"
        );
        assert_eq!(
            Issue::Jump {
                tick: Tick::new(0),
                percent: 4.0
            }
            .describe(timing),
            "The tempo jumps 4.0% at 00:00.000"
        );
        assert_eq!(
            Issue::Anchor {
                tick: Tick::new(0),
                ms: -8.0
            }
            .describe(timing),
            "The anchor at 00:00.000 is 8.0 ms from its onset"
        );
    }

    #[test]
    fn coverage_counts_distinct_grid_lines_not_multiple_hits_on_one_line() {
        let document = song(0.0, &json!([bpm(0, 120.0)]));
        let timing = document.timing();
        let found = residuals(
            timing,
            Grid::default(),
            &[onset(0.01), onset(0.02), onset(1.01)],
            0.0,
        );
        let measured = metrics(
            timing,
            Grid::default(),
            &found,
            (Tick::new(0), Some(Tick::new(3840))),
        );
        assert_eq!(measured.onsets, 3);
        assert!((measured.error_ms.unwrap() - (200.0_f64).sqrt()).abs() < 1e-9);
        assert_eq!(measured.coverage, Some(0.5));
        let outside = metrics(
            timing,
            Grid::default(),
            &found,
            (Tick::new(3840), Some(Tick::new(7680))),
        );
        assert_eq!(outside.error_ms, None);
        assert_eq!(outside.coverage, Some(0.0));
    }

    #[test]
    fn pauses_and_negative_ticks_measure_offsets_against_the_correct_line() {
        let document = song(
            0.0,
            &json!([
                bpm(0, 120.0),
                { "tick": 960, "kind": { "stop": 0.5 } },
                { "tick": 1920, "kind": { "delay": 0.25 } }
            ]),
        );
        let found = residuals(
            document.timing(),
            Grid::default(),
            &[onset(-0.505), onset(0.49), onset(1.01), onset(1.76)],
            0.0,
        );
        assert_eq!(
            found
                .iter()
                .map(|residual| residual.line)
                .collect::<Vec<_>>(),
            [Tick::new(-960), Tick::new(960), Tick::new(1920)]
        );
        assert!((found[0].ms + 5.0).abs() < 1e-9);
        assert!((found[1].ms + 10.0).abs() < 1e-9);
        assert!((found[2].ms - 10.0).abs() < 1e-9);
    }

    proptest! {
        #![proptest_config(crate::proptest_config())]

        #[test]
        fn residuals_are_symmetric_around_an_unpaused_grid_line(
            bpm_value in 60_u16..=240,
            offset_ms in 1_u16..=40,
            beat in 2_i32..=20,
        ) {
            let document = song(0.0, &json!([bpm(0, f64::from(bpm_value))]));
            let timing = document.timing();
            let at = f64::from(beat) * 60.0 / f64::from(bpm_value);
            let delta = f64::from(offset_ms) / 1000.0;
            let found = residuals(timing, Grid::default(), &[onset(at - delta), onset(at + delta)], 0.0);
            prop_assert_eq!(found.len(), 2);
            prop_assert_eq!(found[0].line, Tick::new(beat.saturating_mul(960)));
            prop_assert_eq!(found[1].line, found[0].line);
            prop_assert!((found[0].ms + found[1].ms).abs() < 1e-6);
        }
    }
}
