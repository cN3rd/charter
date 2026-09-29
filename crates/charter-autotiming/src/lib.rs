//! A tempo map from a song's beats, the back end of automatic timing. The beats come from a beat
//! tracker a frame apart at best, so each is first moved onto the strongest onset near it. The
//! beats are then counted, a missed one as two, and split where the tempo changes into stretches
//! whose beats fall on a straight line in time, found as the partition with the least squared
//! error plus a cost for each stretch. Each stretch's tempo is fitted and snapped to a whole, half
//! or third BPM when the grid stays within a few milliseconds of the beats, and its bars come
//! from the tracker's downbeats. The stretches become the song's timing as sections.
//!
//! The method follows the pipeline surveyed in the book's automatic timing chapter: change
//! points on beat times, fitted and snapped a segment at a time.

use std::num::NonZeroU16;

use charter_audio::Onset;
use charter_core::{Section, SectionError, TicksPerBeat, Timing, sectioned};

/// How far from a beat, in seconds, an onset is looked for to learn the tracker's lag.
const LAG: f64 = 0.080;
/// How far from a beat, in seconds, an onset can be to take its place.
const REFINE: f64 = 0.030;
/// How much a beat no onset confirmed counts in a fit, next to one that an onset did.
const UNREFINED: f64 = 0.25;
/// How far from its stretch's line, in seconds, a beat can be and still count, unless the
/// stretch's beats spread wider.
const OUTLIER: f64 = 0.020;
/// The fewest beats a stretch of one tempo holds.
const MIN_BEATS: usize = 8;
/// The spread of a refined beat around its grid line, in seconds, which sets how much a split
/// has to explain before it is made.
const JITTER: f64 = 0.006;
/// The farthest, in seconds, a snapped grid may put a line from the fitted grid's across its
/// stretch.
const SNAP: f64 = 0.005;

/// A stretch of constant tempo the beats fit, and how well.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fitted {
    /// The start, tempo and meter of this stretch.
    pub section: Section,
    /// Beats of the song the stretch spans.
    pub beats: usize,
    /// Whether the tempo snapped to a whole, half or third BPM.
    pub snapped: bool,
    /// The root mean square distance, in seconds, from the stretch's beats to its grid.
    pub error: f64,
}

/// A tempo map estimated from a song's beats: its stretches, and the timing they make.
#[derive(Clone, Debug, PartialEq)]
pub struct Estimate {
    /// Fitted stretches, earliest first.
    pub fitted: Vec<Fitted>,
    /// The chart's resolved timing from those stretches.
    pub timing: Timing,
}

/// A beat as the fit sees it: its time, how much it counts, its place in the count of beats,
/// and whether it starts a bar.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Beat {
    /// Beat position in seconds from the audio's start.
    seconds: f64,
    /// Fit weight: one if onset-confirmed, [`UNREFINED`] otherwise.
    weight: f64,
    /// Counted position including any beats the tracker missed.
    index: i64,
    /// Whether the tracker marked this beat as a bar start.
    downbeat: bool,
}

/// The tempo map `beats` and `downbeats`, in seconds and earliest first, make with the song's
/// `onsets`, at `ticks_per_beat`. None with too few beats to fit a tempo to.
///
/// # Errors
///
/// Returns [`SectionError`] when a fitted tempo or start is past what the format holds.
pub fn estimate(
    beats: &[f64],
    downbeats: &[f64],
    onsets: &[Onset],
    ticks_per_beat: TicksPerBeat,
) -> Result<Option<Estimate>, SectionError> {
    let counted = counted(&refined(beats, downbeats, onsets));
    if counted.len() < MIN_BEATS {
        return Ok(None);
    }
    // Beats far off the line of the stretch they fall in are the tracker's mistakes; split again
    // without them, so they neither bend a tempo nor break a stretch in two.
    let kept: Vec<Beat> = split(&counted).into_iter().flat_map(on_line).collect();
    let all: Vec<Fitted> = split(&kept).into_iter().filter_map(fit).collect();
    // A stretch whose beats scatter wider than a beat can and still count is one the tracker
    // lost, such as a beatless intro; the grid of the stretches around it carries through.
    let steady: Vec<Fitted> = all
        .iter()
        .copied()
        .filter(|fitted| fitted.error <= OUTLIER)
        .collect();
    let fitted = if steady.is_empty() { all } else { steady };
    let sections: Vec<Section> = fitted.iter().map(|fitted| fitted.section).collect();
    let timing = sectioned(&sections, ticks_per_beat)?.timing;
    Ok(Some(Estimate { fitted, timing }))
}

/// Each beat moved onto the nearest onset within [`REFINE`] of it, counting fully, or left where
/// it is counting [`UNREFINED`], and marked when a downbeat falls on it. The tracker's beats sit
/// a steady few tens of milliseconds from the song's onsets, so they are first moved by the
/// song's median of that; after it, the nearest onset is the beat's own even where a louder
/// one, such as a hi-hat's, is near.
fn refined(beats: &[f64], downbeats: &[f64], onsets: &[Onset]) -> Vec<(f64, f64, bool)> {
    let lag = lag(beats, onsets);
    beats
        .iter()
        .map(|&tracked| {
            let beat = tracked + lag;
            let from = onsets.partition_point(|onset| onset.seconds < beat - REFINE);
            let nearest = onsets
                .get(from..)
                .unwrap_or_default()
                .iter()
                .take_while(|onset| onset.seconds <= beat + REFINE)
                .min_by(|a, b| {
                    (a.seconds - beat)
                        .abs()
                        .total_cmp(&(b.seconds - beat).abs())
                });
            let downbeat = downbeats.iter().any(|down| (down - tracked).abs() < 1e-3);
            match nearest {
                Some(onset) => (onset.seconds, 1.0, downbeat),
                None => (beat, UNREFINED, downbeat),
            }
        })
        .collect()
}

/// How far the song's onsets typically sit from its beats: the median, over the beats with an
/// onset within [`LAG`], of the nearest one's distance, later positive.
fn lag(beats: &[f64], onsets: &[Onset]) -> f64 {
    let mut shifts: Vec<f64> = beats
        .iter()
        .filter_map(|&beat| {
            let from = onsets.partition_point(|onset| onset.seconds < beat - LAG);
            onsets
                .get(from..)
                .unwrap_or_default()
                .iter()
                .take_while(|onset| onset.seconds <= beat + LAG)
                .map(|onset| onset.seconds - beat)
                .min_by(|a, b| a.abs().total_cmp(&b.abs()))
        })
        .collect();
    shifts.sort_by(f64::total_cmp);
    shifts
        .get(shifts.len().div_euclid(2))
        .copied()
        .unwrap_or(0.0)
}

/// The beats counted against the song's typical gap between them: a gap of about two counts as
/// a beat missed, so a stretch the tracker followed at half the tempo stays on the song's grid,
/// and a beat much closer to the one before than that is an extra one, dropped.
fn counted(beats: &[(f64, f64, bool)]) -> Vec<Beat> {
    let mut gaps: Vec<f64> = beats
        .windows(2)
        .map(|pair| match pair {
            [earlier, later] => later.0 - earlier.0,
            _ => 0.0,
        })
        .collect();
    gaps.sort_by(f64::total_cmp);
    let period = gaps
        .get(gaps.len().div_euclid(2))
        .copied()
        .unwrap_or(f64::INFINITY);
    let mut counted: Vec<Beat> = Vec::with_capacity(beats.len());
    for &(seconds, weight, downbeat) in beats {
        let index = match counted.last() {
            None => 0,
            Some(last) => {
                let gap = seconds - last.seconds;
                if gap < period / 2.0 {
                    continue;
                }
                let steps = rounded_steps((gap / period).round().max(1.0));
                last.index.saturating_add(steps)
            }
        };
        counted.push(Beat {
            seconds,
            weight,
            index,
            downbeat,
        });
    }
    counted
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "a beat gap's rounded step count saturates at the integer limits"
)]
/// Rounded beat gap as a saturating integer count.
fn rounded_steps(steps: f64) -> i64 {
    steps as i64
}

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "song-length beat counts are converted for the floating-point fit"
)]
/// A beat count as a floating-point value for the fit.
fn beat_float(count: usize) -> f64 {
    count as f64
}

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "song-length beat indices are converted for the floating-point fit"
)]
/// A counted beat index as a floating-point value for the fit.
fn index_float(index: i64) -> f64 {
    index as f64
}

/// The beats of `stretch` within [`OUTLIER`], or three times the stretch's spread, of its line.
fn on_line(stretch: &[Beat]) -> impl Iterator<Item = Beat> + '_ {
    let sums = stretch.iter().fold(Sums::default(), Sums::add);
    let (start, period, error) = sums.line();
    let limit = OUTLIER.max(3.0 * (error / sums.w).sqrt());
    stretch
        .iter()
        .filter(move |beat| (grid(start, period, beat) - beat.seconds).abs() <= limit)
        .copied()
}

/// Sums over a run of beats that give its weighted least-squares line in O(1).
#[derive(Clone, Copy, Debug, Default)]
struct Sums {
    /// Sum of fit weights.
    w: f64,
    /// Sum of weight times beat index.
    wk: f64,
    /// Sum of weight times squared beat index.
    wkk: f64,
    /// Sum of weight times beat time in seconds.
    wt: f64,
    /// Sum of weight times index times time in seconds.
    wkt: f64,
    /// Sum of weight times squared time in seconds.
    wtt: f64,
}

impl Sums {
    /// The sums after including `beat`.
    fn add(self, beat: &Beat) -> Self {
        let k = index_float(beat.index);
        let (w, t) = (beat.weight, beat.seconds);
        Self {
            w: self.w + w,
            wk: self.wk + w * k,
            wkk: self.wkk + w * k * k,
            wt: self.wt + w * t,
            wkt: self.wkt + w * k * t,
            wtt: self.wtt + w * t * t,
        }
    }

    /// The sums for a suffix after subtracting the prefix `earlier`.
    fn minus(self, earlier: Self) -> Self {
        Self {
            w: self.w - earlier.w,
            wk: self.wk - earlier.wk,
            wkk: self.wkk - earlier.wkk,
            wt: self.wt - earlier.wt,
            wkt: self.wkt - earlier.wkt,
            wtt: self.wtt - earlier.wtt,
        }
    }

    /// The line `seconds = start + period × index` fitting the run, and its weighted squared
    /// error.
    #[expect(
        clippy::suspicious_operation_groupings,
        reason = "w·wkk − wk·wk is the normal equations' determinant"
    )]
    fn line(self) -> (f64, f64, f64) {
        let spread = self.w * self.wkk - self.wk * self.wk;
        let period = (self.w * self.wkt - self.wk * self.wt) / spread;
        let start = (self.wt - period * self.wk) / self.w;
        let error = (self.wtt - start * self.wt - period * self.wkt).max(0.0);
        (start, period, error)
    }
}

#[expect(
    clippy::indexing_slicing,
    reason = "every table index is bounded by beats.len()"
)]
/// `beats` split into stretches of one tempo each: the partition with the least squared error
/// from each stretch's line plus a cost for each stretch, a little more for one that does not
/// start on a downbeat.
fn split(beats: &[Beat]) -> Vec<&[Beat]> {
    let len = beats.len();
    let mut sums = vec![Sums::default(); len.saturating_add(1)];
    for (at, beat) in beats.iter().enumerate() {
        sums[at.saturating_add(1)] = sums[at].add(beat);
    }
    let penalty = 2.0 * JITTER * JITTER * beat_float(len).ln().max(1.0) * 8.0;
    let mut best = vec![f64::INFINITY; len.saturating_add(1)];
    let mut from = vec![0; len.saturating_add(1)];
    best[0] = 0.0;
    for end in MIN_BEATS..=len {
        for start in (0..=end.saturating_sub(MIN_BEATS)).rev() {
            if !best[start].is_finite() {
                continue;
            }
            let (_, _, error) = sums[end].minus(sums[start]).line();
            let off_bar = if beats[start].downbeat {
                0.0
            } else {
                penalty / 2.0
            };
            let cost = best[start] + error + penalty + off_bar;
            if cost < best[end] {
                best[end] = cost;
                from[end] = start;
            }
        }
    }
    let mut stretches = Vec::new();
    let mut end = len;
    while end > 0 {
        let start = from[end];
        stretches.push(&beats[start..end]);
        end = start;
    }
    stretches.reverse();
    stretches
}

/// The section `stretch` fits: its tempo snapped where the snapped grid stays within [`SNAP`] of
/// the fitted one across the stretch, which one stray beat cannot spoil, its bars as long as the
/// downbeats most often are apart, starting on its first downbeat.
fn fit(stretch: &[Beat]) -> Option<Fitted> {
    let sums = stretch.iter().fold(Sums::default(), Sums::add);
    let (_, period, error) = sums.line();
    if !(period.is_finite() && period > 0.0) {
        return None;
    }
    let spread = (error / sums.w).sqrt();
    let bpm = 60.0 / period;
    let free = offset(stretch, period);
    let (first, last) = (stretch.first()?, stretch.last()?);
    let snapped = [1.0, 2.0, 3.0].into_iter().find_map(|per| {
        let snapped_period = 60.0 / ((bpm * per).round() / per);
        let start = offset(stretch, snapped_period);
        let apart =
            |beat: &Beat| (grid(start, snapped_period, beat) - grid(free, period, beat)).abs();
        (apart(first).max(apart(last)) <= SNAP).then_some((start, snapped_period))
    });
    let (start, final_period, is_snapped) = snapped.map_or_else(
        || {
            let quantized_period = 60.0 / ((bpm * 1000.0).round() / 1000.0);
            (offset(stretch, quantized_period), quantized_period, false)
        },
        |(snapped_start, snapped_period)| (snapped_start, snapped_period, true),
    );
    let bar = stretch.iter().find(|beat| beat.downbeat).unwrap_or(first);
    Some(Fitted {
        section: Section {
            seconds: grid(start, final_period, bar),
            beat_seconds: final_period,
            meter: meter(stretch),
        },
        beats: usize::try_from(last.index.saturating_sub(first.index).saturating_add(1))
            .unwrap_or(0),
        snapped: is_snapped,
        error: spread,
    })
}

/// Where the grid with `period` puts `beat`'s line, from the line of index 0 at `start`.
fn grid(start: f64, period: f64, beat: &Beat) -> f64 {
    let index = index_float(beat.index);
    start + period * index
}

/// The start of index 0 that best fits `stretch` to a grid of `period`, by weight.
fn offset(stretch: &[Beat], period: f64) -> f64 {
    let (weighted, weight) = stretch.iter().fold((0.0, 0.0), |(sum, weight), beat| {
        let index = index_float(beat.index);
        (
            sum + beat.weight * (beat.seconds - period * index),
            weight + beat.weight,
        )
    });
    weighted / weight
}

/// The beats a bar of `stretch` most often holds, counted between its downbeats; four when its
/// downbeats do not say.
fn meter(stretch: &[Beat]) -> NonZeroU16 {
    let downbeats: Vec<i64> = stretch
        .iter()
        .filter(|beat| beat.downbeat)
        .map(|beat| beat.index)
        .collect();
    let mut counts = [0_usize; 17];
    for pair in downbeats.windows(2) {
        let [earlier, later] = pair else {
            continue;
        };
        if let Some(count) = usize::try_from(later.saturating_sub(*earlier))
            .ok()
            .and_then(|beats| counts.get_mut(beats))
        {
            *count = count.saturating_add(1);
        }
    }
    let (most_common_beats, times) = counts
        .iter()
        .enumerate()
        .skip(1)
        .max_by_key(|&(beats, times)| (*times, beats == 4))
        .unwrap_or((4, &0));
    let meter_beats = if *times >= 2 { most_common_beats } else { 4 };
    NonZeroU16::new(u16::try_from(meter_beats).unwrap_or(4)).unwrap_or(NonZeroU16::MIN)
}

#[cfg(test)]
mod tests {
    use super::*;
    use charter_core::{Bpm, DEFAULT_TICKS_PER_BEAT, ResolvedTime, Tick};

    /// Beats `period` apart from `start`, `count` of them, each pushed by `jitter(n)`, with a
    /// downbeat every `bar` beats from the first.
    fn beats(start: f64, bpm: f64, count: usize, bar: usize) -> (Vec<f64>, Vec<f64>) {
        let period = 60.0 / bpm;
        let times: Vec<f64> = (0..count)
            .map(|n| {
                let n = beat_float(n);
                // A tracker's beats sit a frame's worth either side of the audio's.
                start + period * n + 0.008 * (n * 1.7).sin()
            })
            .collect();
        let downbeats = times.iter().copied().step_by(bar).collect();
        (times, downbeats)
    }

    /// An onset exactly on each true beat.
    fn onsets(start: f64, bpm: f64, count: usize) -> Vec<Onset> {
        (0..count)
            .map(|n| {
                let n = beat_float(n);
                Onset {
                    seconds: start + 60.0 / bpm * n,
                    strength: 0.8,
                }
            })
            .collect()
    }

    fn seconds_of(timing: &Timing) -> Vec<f64> {
        timing
            .anchors
            .iter()
            .map(|anchor| ResolvedTime::from(anchor.seconds).seconds())
            .collect()
    }

    #[test]
    fn a_nearby_onset_refines_the_beat_even_when_a_louder_onset_is_farther_away() {
        let heard = [
            Onset {
                seconds: 1.012,
                strength: 0.1,
            },
            Onset {
                seconds: 1.020,
                strength: 1.0,
            },
        ];
        assert_eq!(refined(&[1.0], &[], &heard), [(1.012, 1.0, false)]);
    }

    #[test]
    fn the_typical_lag_uses_the_nearest_onset_to_each_beat() {
        let heard = [
            Onset {
                seconds: 0.02,
                strength: 1.0,
            },
            Onset {
                seconds: 0.06,
                strength: 1.0,
            },
            Onset {
                seconds: 1.02,
                strength: 1.0,
            },
            Onset {
                seconds: 1.06,
                strength: 1.0,
            },
            Onset {
                seconds: 2.02,
                strength: 1.0,
            },
            Onset {
                seconds: 2.06,
                strength: 1.0,
            },
        ];
        assert!((lag(&[0.0, 1.0, 2.0], &heard) - 0.02).abs() < 1e-9);
    }

    #[test]
    fn a_steady_song_is_one_snapped_section_on_its_first_downbeat() {
        let (beats, downbeats) = beats(0.371, 128.0, 200, 4);
        let found = estimate(
            &beats,
            &downbeats,
            &onsets(0.371, 128.0, 200),
            DEFAULT_TICKS_PER_BEAT,
        )
        .unwrap()
        .unwrap();
        assert_eq!(found.fitted.len(), 1, "{:?}", found.fitted);
        assert!(found.fitted[0].snapped);
        assert_eq!(found.timing.tempo, Bpm::try_from(128.0).unwrap());
        assert!((seconds_of(&found.timing)[0] - 0.371).abs() < 1e-4);
    }

    #[test]
    fn a_tempo_change_starts_a_section_where_it_happens() {
        let (mut all, mut downs) = beats(0.2, 120.0, 64, 4);
        let change = 0.2 + 64.0 * 0.5;
        let (later, later_downs) = beats(change, 150.0, 80, 4);
        let mut heard = onsets(0.2, 120.0, 64);
        heard.extend(onsets(change, 150.0, 80));
        all.extend(later);
        downs.extend(later_downs);
        let found = estimate(&all, &downs, &heard, DEFAULT_TICKS_PER_BEAT)
            .unwrap()
            .unwrap();
        let bpms: Vec<f64> = found
            .fitted
            .iter()
            .map(|fitted| (60.0 / fitted.section.beat_seconds * 1000.0).round() / 1000.0)
            .collect();
        assert_eq!(bpms, [120.0, 150.0]);
        let starts = seconds_of(&found.timing);
        assert!((starts[1] - change).abs() < 1e-3, "{starts:?}");
    }

    #[test]
    fn a_missed_beat_counts_two_and_an_extra_one_is_dropped() {
        let (mut all, downs) = beats(0.5, 100.0, 60, 4);
        all.remove(20);
        all.insert(30, all[29] + 0.1);
        let found = estimate(
            &all,
            &downs,
            &onsets(0.5, 100.0, 60),
            DEFAULT_TICKS_PER_BEAT,
        )
        .unwrap()
        .unwrap();
        assert_eq!(found.fitted.len(), 1, "{:?}", found.fitted);
        assert_eq!(found.timing.tempo, Bpm::try_from(100.0).unwrap());
    }

    #[test]
    fn the_downbeats_give_the_bars_their_beats() {
        let (beats, downbeats) = beats(0.0, 90.0, 60, 3);
        let found = estimate(
            &beats,
            &downbeats,
            &onsets(0.0, 90.0, 60),
            DEFAULT_TICKS_PER_BEAT,
        )
        .unwrap()
        .unwrap();
        assert_eq!(found.fitted[0].section.meter.get(), 3);
        let meters: Vec<_> = found.timing.events.iter().map(|event| event.tick).collect();
        assert_eq!(meters, [Tick::new(0)]);
    }

    #[test]
    fn a_tempo_between_whole_bpms_keeps_three_places() {
        let (beats, downbeats) = beats(0.0, 133.2, 300, 4);
        let found = estimate(
            &beats,
            &downbeats,
            &onsets(0.0, 133.2, 300),
            DEFAULT_TICKS_PER_BEAT,
        )
        .unwrap()
        .unwrap();
        assert!(!found.fitted[0].snapped);
        assert!((f64::from(found.timing.tempo) - 133.2).abs() < 1e-9);
    }

    #[test]
    fn too_few_beats_make_no_estimate() {
        let (beats, downbeats) = beats(0.0, 120.0, 5, 4);
        assert_eq!(
            estimate(&beats, &downbeats, &[], DEFAULT_TICKS_PER_BEAT),
            Ok(None)
        );
    }
}

/// Proofs for Kani, run with `cargo kani -p charter-autotiming`.
#[cfg(kani)]
mod proofs {
    use super::*;

    #[kani::proof]
    #[kani::unwind(18)]
    fn three_matching_downbeat_gaps_choose_their_meter() {
        let gap: i64 = if kani::any::<bool>() { 3 } else { 5 };
        let stretch: [Beat; 4] = std::array::from_fn(|index| Beat {
            seconds: 0.0,
            weight: 1.0,
            index: i64::try_from(index).unwrap_or_default() * gap,
            downbeat: true,
        });
        assert_eq!(
            meter(&stretch).get(),
            u16::try_from(gap).unwrap_or_default()
        );
    }
}
