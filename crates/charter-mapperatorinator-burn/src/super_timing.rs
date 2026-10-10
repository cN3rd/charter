//! Super timing: many passes of the model over a song, each shifted by its own offset, fused into
//! one beat grid.
//!
//! Ported line by line from OliBomby/Mapperatorinator (MIT),
//! `osuT5/osuT5/inference/super_timing_generator.py`, `SuperTimingGenerator.generate` after its
//! model calls. Every pass's beats, measures and timing points land in histograms at 1 ms
//! resolution, which are smoothed. The local tempo at each peak comes from the spacing of nearby
//! beats. Walking outwards from the strongest peaks at that tempo, snapping to peaks on the way,
//! builds the beat grid, whose beats are then labelled measures or timing points. Python's
//! indexing quirks the original leans on (negative slice starts, negative indices) are kept.

use anyhow::{Result, bail};

use crate::signal::{find_peaks, gaussian_filter1d};
use crate::tokenizer::{BEAT, Event, MEASURE, TIME_SHIFT, TIMING_POINT, TYPE_EVENTS, event_time};

/// What a beat of the grid is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BeatKind {
    /// A beat inside a bar.
    Beat,
    /// A beat that starts a bar.
    Measure,
    /// A beat that starts a tempo or a meter.
    TimingPoint,
}

impl BeatKind {
    /// The kind of beat an event of type `kind` is, if it is one.
    pub(crate) fn of(kind: &str) -> Option<Self> {
        match kind {
            BEAT => Some(Self::Beat),
            MEASURE => Some(Self::Measure),
            TIMING_POINT => Some(Self::TimingPoint),
            _ => None,
        }
    }

    /// The event type of this kind of beat.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Beat => BEAT,
            Self::Measure => MEASURE,
            Self::TimingPoint => TIMING_POINT,
        }
    }
}

/// `get_groups` with the beat kind after its time shift, or 0 without one.
fn groups(events: &[Event]) -> Vec<(BeatKind, i64)> {
    let mut out = Vec::new();
    let mut time = 0_i64;
    for e in events {
        if e.kind == TIME_SHIFT {
            time = i64::from(e.value);
        } else if TYPE_EVENTS.contains(&e.kind.as_str()) {
            if let Some(kind) = BeatKind::of(e.kind.as_str()) {
                out.push((kind, time));
            }
            time = 0;
        }
    }
    out
}

/// `sum(a[start:stop])` with Python's slice rules.
fn py_sum(a: &[f64], start: i64, stop: i64) -> f64 {
    let n = signed(a.len());
    let clamp = |i: i64| {
        if i < 0 {
            i.saturating_add(n).max(0)
        } else {
            i.min(n)
        }
    };
    let (s, e) = (clamp(start), clamp(stop));
    if s >= e {
        0.0
    } else {
        a.get(unsigned(s)..unsigned(e))
            .map_or(0.0, |slice| slice.iter().sum())
    }
}

/// `a[i]` with Python's negative indices.
fn py_index<T: Copy>(a: &[T], i: i64) -> Option<T> {
    let n = signed(a.len());
    let i = if i < 0 { i.saturating_add(n) } else { i };
    usize::try_from(i).ok().and_then(|i| a.get(i)).copied()
}

/// `np.median` of `values`, which it sorts.
#[expect(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "callers pass at least one value, so the middle ones are in bounds"
)]
#[expect(
    clippy::manual_midpoint,
    reason = "numpy's median averages the middle pair as (a + b) / 2"
)]
fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    let n = values.len();
    if n % 2 == 1 {
        values[n.div_euclid(2)]
    } else {
        (values[n.div_euclid(2) - 1] + values[n.div_euclid(2)]) / 2.0
    }
}

/// A peak of the smoothed histograms, and the tempo around it.
#[derive(Clone, Copy)]
struct Peak {
    /// Where the peak is, in milliseconds.
    time: usize,
    /// How far the peak stands above its surroundings.
    prominence: f64,
    /// The tempo at the peak, in beats a minute.
    bpm: f64,
    /// Whether the tempo comes from the beats near the peak rather than a neighbour.
    defined: bool,
}

/// The beat grid of a song `num_ms` milliseconds long from its passes, each the offset its audio
/// was shifted by and the events the model wrote for it, as time shift and type events.
#[expect(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    reason = "a whole port of upstream's SuperTimingGenerator.generate, kept as one to compare"
)]
#[expect(
    clippy::indexing_slicing,
    reason = "times index the histograms only within the song, and other indices their own lists"
)]
#[expect(
    clippy::arithmetic_side_effects,
    reason = "a song's milliseconds, beat lengths and bar sizes are far from i64's limits"
)]
pub(crate) fn fuse(
    num_ms: usize,
    passes: &[(i64, Vec<Event>)],
    bpm_threshold: f64,
) -> Result<Vec<Event>> {
    let iterations = count_float(passes.len());
    let n = signed(num_ms);
    let mut beats_hist = vec![0.0; num_ms];
    let mut measures_hist = vec![0.0; num_ms];
    let mut timing_points_hist = vec![0.0; num_ms];
    let mut tpbs: Vec<(i64, i64)> = Vec::new();
    let mut measure_counts: Vec<i64> = Vec::new();

    for (offset, events) in passes {
        let mut last_beat_time: Option<i64> = None;
        let mut last_group_type: Option<BeatKind> = None;
        let mut measure_counter: Option<i64> = None;
        for (kind, group_time) in groups(events) {
            let time = group_time - offset;
            if time < 0 || time >= n {
                continue;
            }
            match kind {
                BeatKind::Beat => {
                    beats_hist[unsigned(time)] += 1.0;
                    if let Some(c) = measure_counter.as_mut() {
                        *c += 1;
                    }
                }
                BeatKind::Measure => {
                    measures_hist[unsigned(time)] += 1.0;
                    if let Some(c) = measure_counter {
                        measure_counts.push(c);
                    }
                    measure_counter = Some(1);
                }
                BeatKind::TimingPoint => {
                    timing_points_hist[unsigned(time)] += 1.0;
                    measure_counter = Some(1);
                }
            }
            if let Some(last) = last_beat_time
                && last != time
                && !(kind == BeatKind::TimingPoint
                    && last_group_type != Some(BeatKind::TimingPoint))
            {
                let tpb = (time - last).div_euclid(10);
                if 20 < tpb && tpb < 100 {
                    tpbs.push((last, tpb));
                }
            }
            last_beat_time = Some(time);
            last_group_type = Some(kind);
        }
    }
    if tpbs.is_empty() {
        bail!("the model found no beats in the audio");
    }
    if measure_counts.is_empty() {
        bail!("the model found no measures in the audio");
    }

    let smooth = |h: &[f64]| -> Vec<f64> {
        gaussian_filter1d(h, 10.0)
            .iter()
            .map(|v| v / iterations * 50.0)
            .collect()
    };
    let (beats_hist, measures_hist, timing_points_hist) = (
        smooth(&beats_hist),
        smooth(&measures_hist),
        smooth(&timing_points_hist),
    );
    tpbs.sort_by_key(|&(time, _)| time);

    let signal: Vec<f64> = (0..num_ms)
        .map(|i| beats_hist[i] + measures_hist[i] + timing_points_hist[i] * 2.0)
        .collect();
    let (peakind, prominences) = find_peaks(&signal, 50.0, 0.1, 2.0, 1.0, 50.0);

    // The tempo at each peak: the most common beat length nearby, if it clearly is.
    let w = 200_i64;
    let mut peak_bpms: Vec<f64> = peakind
        .iter()
        .map(|&peak| {
            let peak = signed(peak);
            // np.histogram(bins=range(20, 100)): bins of one, the last also holding 99.
            let mut hist = [0_u32; 79];
            for &(time, tpb) in &tpbs {
                if peak - w < time && time < peak + w && (20..=99).contains(&tpb) {
                    hist[unsigned(tpb - 20).min(78)] += 1;
                }
            }
            let max = *hist.iter().max().unwrap_or(&0);
            let sum: u32 = hist.iter().sum();
            if f64::from(max) > bpm_threshold * f64::from(sum) {
                let argmax = hist.iter().position(|&h| h == max).unwrap_or(0);
                60_000.0 / (count_float(20 + argmax) * 10.0)
            } else {
                f64::NAN
            }
        })
        .collect();
    let defined: Vec<bool> = peak_bpms.iter().map(|b| !b.is_nan()).collect();

    // Fold every tempo into [median / 1.5, median * 1.5] by whole multiples.
    let median_bpm = 60_000.0
        / (median(
            &mut tpbs
                .iter()
                .map(|&(_, t)| whole_float(t))
                .collect::<Vec<_>>(),
        ) * 10.0);
    for bpm in &mut peak_bpms {
        *bpm /= (*bpm / (median_bpm * 1.5)).ceil();
        *bpm *= ((median_bpm / 1.5) / *bpm).ceil();
    }
    // A peak without a tempo takes its nearest neighbour's, the earlier on a tie.
    for i in 0..peak_bpms.len() {
        if !peak_bpms[i].is_nan() {
            continue;
        }
        let mut left = signed(i) - 1;
        while left >= 0 && peak_bpms[unsigned(left)].is_nan() {
            left -= 1;
        }
        let mut right = i + 1;
        while right < peak_bpms.len() && peak_bpms[right].is_nan() {
            right += 1;
        }
        let i64i = signed(i);
        peak_bpms[i] =
            if left >= 0 && (right >= peak_bpms.len() || i64i - left <= signed(right - i)) {
                peak_bpms[unsigned(left)]
            } else if right < peak_bpms.len() && (left < 0 || i64i - left > signed(right - i)) {
                peak_bpms[right]
            } else {
                median_bpm
            };
    }

    let peaks: Vec<Peak> = (0..peakind.len())
        .map(|i| Peak {
            time: peakind[i],
            prominence: prominences[i],
            bpm: peak_bpms[i],
            defined: defined[i],
        })
        .collect();
    let mut beat_times: Vec<i64> = Vec::new();
    let mut to_process = peaks.clone();
    to_process.sort_by(|a, b| b.prominence.total_cmp(&a.prominence));
    let mut processed: Vec<(f64, f64)> = Vec::new();

    #[expect(
        clippy::shadow_unrelated,
        reason = "the closure takes the list upstream's remove_range captures, under its name"
    )]
    let remove_range = |to_process: &mut Vec<Peak>, t1: f64, t2: f64| {
        let (t1, t2) = if t1 > t2 { (t2, t1) } else { (t1, t2) };
        to_process.retain(|p| !(t1 <= count_float(p.time) && count_float(p.time) <= t2));
    };
    let num_ms_f = count_float(num_ms);
    #[expect(
        clippy::shadow_unrelated,
        reason = "the closure takes the lists upstream's walk captures, under their names"
    )]
    let walk = |start: f64,
                mut period: f64,
                direction: f64,
                to_process: &mut Vec<Peak>,
                processed: &mut Vec<(f64, f64)>,
                beat_times: &mut Vec<i64>| {
        let loss = |p: &Peak, time: f64| (count_float(p.time) - time).abs() / p.prominence;
        let mut time = start;
        loop {
            let previous = time;
            time += direction * period;
            if !(0.0 <= time && time < num_ms_f) {
                remove_range(to_process, previous, time);
                break;
            }
            // Only peaks ahead, so a strong peak cannot be snapped to over and over.
            let nearest = peaks
                .iter()
                .filter(|p| direction * (count_float(p.time) - previous) > 0.0)
                .fold(None::<&Peak>, |best, p| match best {
                    Some(b) if loss(b, time) <= loss(p, time) => Some(b),
                    _ => Some(p),
                });
            if let Some(nearest) = nearest {
                let l = loss(nearest, time);
                if l < 60.0 {
                    time = count_float(nearest.time);
                    period = 60_000.0 / nearest.bpm;
                } else if l < 300.0 && nearest.defined {
                    // A beat nearby, likely of another tempo.
                    time -= direction * period;
                    break;
                }
            }
            if processed.iter().any(|&(t1, t2)| t1 <= time && time <= t2) {
                break;
            }
            beat_times.push(truncated(time.trunc()));
        }
        // Keep seams from making beats too near each other (over 300 BPM).
        let m = 200.0;
        let (t1, t2) = if direction > 0.0 {
            (start - m, time + m)
        } else {
            (time - m, start + m)
        };
        processed.push((t1, t2));
        remove_range(to_process, t1, t2);
    };
    while !to_process.is_empty() {
        let peak = to_process.remove(0);
        let period = 60_000.0 / peak.bpm;
        beat_times.push(signed(peak.time));
        walk(
            count_float(peak.time),
            period,
            1.0,
            &mut to_process,
            &mut processed,
            &mut beat_times,
        );
        walk(
            count_float(peak.time),
            period,
            -1.0,
            &mut to_process,
            &mut processed,
            &mut beat_times,
        );
    }
    beat_times.sort_unstable();

    let win = 10;
    let mut beat_types: Vec<BeatKind> = beat_times
        .iter()
        .map(|&t| {
            let beat = py_sum(&beats_hist, t - win, t + win);
            let measure = py_sum(&measures_hist, t - win, t + win);
            let timing_point = py_sum(&timing_points_hist, t - win, t + win);
            if timing_point > beat && timing_point > measure && beat + measure + timing_point > 1.0
            {
                BeatKind::TimingPoint
            } else {
                BeatKind::Beat
            }
        })
        .collect();

    // Label measures by the song's usual meter, choosing each bar's phase by the evidence.
    let signature = truncated(
        median(
            &mut measure_counts
                .iter()
                .map(|&c| whole_float(c))
                .collect::<Vec<_>>(),
        )
        .trunc(),
    );
    let snapshot = beat_types.clone();
    let count = signed(beat_times.len());
    let mut cooldown = 0_i64;
    for i in 0..beat_times.len() {
        let beat_type = snapshot[i];
        if cooldown > 0 {
            cooldown -= 1;
            continue;
        }
        if cooldown < 0 {
            cooldown += 1;
            if cooldown == 0 && beat_type != BeatKind::TimingPoint {
                beat_types[i] = BeatKind::Measure;
                cooldown = signature - 1;
            }
            continue;
        }
        if beat_type == BeatKind::TimingPoint {
            continue;
        }
        let mut offset_scores = Vec::new();
        for k in 0..signature {
            let (mut score, mut scored) = (0.0, 0);
            for j in -3_i64..4 {
                let index = signed(i) + j * signature + k;
                if index < 0 || index >= count {
                    continue;
                }
                // Upstream's check indexes beat_types absolutely, from its ends.
                if (1..j.abs())
                    .any(|m| py_index(&beat_types, m * j.signum()) == Some(BeatKind::TimingPoint))
                {
                    continue;
                }
                let other = beat_times[unsigned(index)];
                score += py_sum(&measures_hist, other - win, other + win)
                    + py_sum(&timing_points_hist, other - win, other + win);
                scored += 1;
            }
            offset_scores.push(if scored == 0 {
                0.0
            } else {
                score / f64::from(scored)
            });
        }
        let best = offset_scores
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        #[expect(
            clippy::float_cmp,
            reason = "the first score equal to the best is numpy's argmax"
        )]
        let argmax = signed(offset_scores.iter().position(|&s| s == best).unwrap_or(0));
        if argmax == 0 {
            beat_types[i] = BeatKind::Measure;
            cooldown = signature - 1;
        } else {
            beat_types[i] = BeatKind::Beat;
            cooldown = -argmax;
        }
    }

    Ok(beat_times
        .iter()
        .zip(&beat_types)
        .flat_map(|(&t, b)| {
            [
                Event::new(TIME_SHIFT, event_time(t)),
                Event::new(b.name(), 0),
            ]
        })
        .collect())
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_wrap,
    reason = "a song's milliseconds, peaks and beats number far below i64::MAX"
)]
/// A count or a position as a signed one.
const fn signed(value: usize) -> i64 {
    value as i64
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "every time and index is checked or clamped to be non-negative before it is cast"
)]
/// A non-negative time or index as an index.
const fn unsigned(value: i64) -> usize {
    value as usize
}

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "a song's milliseconds, its passes and the bins are far below 2^52, where f64 is exact"
)]
/// A count or a position as a float.
const fn count_float(count: usize) -> f64 {
    count as f64
}

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "beat lengths and bar sizes are small whole numbers"
)]
/// A whole number as a float.
const fn whole_float(value: i64) -> f64 {
    value as f64
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "beat times and bar sizes are far inside i64, and int() truncates toward zero too"
)]
/// `value` truncated toward zero, as Python's `int` does.
const fn truncated(value: f64) -> i64 {
    value as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pass(offset: i64, beats: &[(i64, &str)]) -> (i64, Vec<Event>) {
        (
            offset,
            beats
                .iter()
                .flat_map(|&(t, k)| {
                    [
                        Event::new(TIME_SHIFT, event_time(t.saturating_add(offset))),
                        Event::new(k, 0),
                    ]
                })
                .collect(),
        )
    }

    #[test]
    fn grouping_keeps_beat_kinds_and_resets_time_after_other_events() {
        let events = [
            Event::new(TIME_SHIFT, 100),
            Event::new(BEAT, 0),
            Event::new(TIME_SHIFT, 200),
            Event::new("circle", 0),
            Event::new(BEAT, 0),
            Event::new(TIME_SHIFT, 300),
            Event::new(MEASURE, 0),
            Event::new(TIME_SHIFT, 400),
            Event::new(TIMING_POINT, 0),
        ];
        assert_eq!(
            groups(&events),
            [
                (BeatKind::Beat, 100),
                (BeatKind::Beat, 0),
                (BeatKind::Measure, 300),
                (BeatKind::TimingPoint, 400),
            ]
        );
    }

    #[test]
    fn python_slices_and_indices() {
        let a = [1.0, 2.0, 3.0, 4.0];
        assert_eq!(py_sum(&a, -1, 2), 0.0);
        assert_eq!(py_sum(&a, -3, 4), 9.0);
        assert_eq!(py_sum(&a, 2, 10), 7.0);
        assert_eq!(py_index(&a, -1), Some(4.0));
        assert_eq!(py_index(&a, 4), None);
    }

    #[test]
    fn a_steady_120_bpm_in_four_comes_back_as_its_grid() {
        let beats: Vec<(i64, &str)> = (0..40)
            .map(|i| (1000 + i * 500, if i % 4 == 0 { MEASURE } else { BEAT }))
            .collect();
        let passes: Vec<_> = [-300, 0, 250].iter().map(|&o| pass(o, &beats)).collect();
        let events = fuse(22000, &passes, 0.1).unwrap();
        let grid: Vec<(i32, &str)> = events
            .as_chunks::<2>()
            .0
            .iter()
            .map(|[shift, beat]| (shift.value, beat.kind.as_str()))
            .collect();
        let on: Vec<_> = grid
            .iter()
            .filter(|(t, _)| (1000..=20500).contains(t))
            .collect();
        assert_eq!(on.len(), 40, "{grid:?}");
        for (i, (t, k)) in on.iter().enumerate() {
            assert_eq!(*t, 1000 + 500 * i32::try_from(i).unwrap());
            assert_eq!(*k, if i % 4 == 0 { MEASURE } else { BEAT }, "beat {i}");
        }
    }

    #[test]
    fn no_beats_is_an_error_not_a_panic() {
        fuse(1000, &[(0, Vec::new())], 0.1).unwrap_err();
    }
}
