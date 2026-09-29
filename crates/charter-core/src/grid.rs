//! The measure grid that a timing map's time signatures lay over its ticks.

use std::fmt;
#[cfg(test)]
use std::num::NonZeroU16;

use crate::document::{Timing, TimingKind};
use crate::unit::{Tick, Ticks, TicksPerBeat, TimeSignature};

/// One measure of a timing map's grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Measure {
    /// Counting from 0 for the measure holding tick 0, and down from -1 before it.
    pub number: i64,
    /// First tick of this measure.
    pub start: Tick,
    /// A whole measure of its meter, or less where the next time signature cuts it short. Both
    /// ends stay inside the ticks a document can hold.
    pub length: Ticks,
    /// Meter governing this measure.
    pub meter: TimeSignature,
}

impl Measure {
    /// How far through the measure `tick` lies: 0 at its start and 1 at its end, outside that
    /// for ticks outside it. A view laying a measure out across a row places ticks by this.
    #[must_use]
    pub fn fraction(&self, tick: Tick) -> f64 {
        let past = i64::from(tick.get()).saturating_sub(i64::from(self.start.get()));
        let past = tick_distance_as_f64(past);
        past / f64::from(self.length.get())
    }

    /// The tick `fraction` of the way through the measure, to the nearest tick and kept within
    /// the measure.
    #[must_use]
    pub fn tick_at(&self, fraction: f64) -> Tick {
        let length = f64::from(self.length.get());
        let past = clamped_measure_tick((fraction.clamp(0.0, 1.0) * length).round());
        let tick = i64::from(self.start.get()).saturating_add(past);
        Tick::new(i32::try_from(tick).unwrap_or(i32::MAX))
    }
}

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "distance between i32 ticks is exact in f64"
)]
/// Tick distance as a float; differences between `i32` ticks remain exact.
fn tick_distance_as_f64(value: i64) -> f64 {
    value as f64
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "fraction is clamped to a u32 measure length"
)]
/// Whole tick offset after clamping a fraction to one measure.
fn clamped_measure_tick(value: f64) -> i64 {
    value as i64
}

/// Where a tick falls in the grid: its measure, the beats since that measure started, counting
/// from 0, and the ticks past the last of those beats. Shown as `measure:beat:tick`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridPosition {
    /// Zero-based measure number, negative before tick zero.
    pub measure: i64,
    /// Whole beats past the start of the measure.
    pub beat: u32,
    /// Ticks past the last whole beat.
    pub tick: u32,
}

impl fmt::Display for GridPosition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.measure, self.beat, self.tick)
    }
}

/// The meter before the first time signature.
const COMMON_TIME: TimeSignature = TimeSignature::COMMON;

/// A stretch of ticks over which one meter lays measures `length` apart from `origin`, until
/// the next stretch begins.
#[derive(Clone, Copy, Debug)]
struct Stretch {
    /// First tick governed by this time signature.
    begin: i64,
    /// Tick at which this signature starts a new measure.
    origin: i64,
    /// Ticks per measure under this signature.
    length: i64,
    /// Time signature applying throughout the stretch.
    meter: TimeSignature,
}

/// The measures of a grid in order.
#[derive(Clone, Debug)]
pub struct Measures {
    /// Consecutive meter stretches, including common time before the first event.
    stretches: Vec<Stretch>,
    /// Index of the stretch containing the next measure.
    stretch: usize,
    /// Tick where the next measure starts, possibly beyond the `i32` range.
    start: i64,
    /// Number assigned to the next measure, relative to the one holding tick zero.
    number: i64,
}

impl Measures {
    /// Where the stretch at `index` ends: where the next begins, or never.
    fn end_of(&self, index: usize) -> i64 {
        self.stretches
            .get(index.saturating_add(1))
            .map_or(i64::MAX, |next| next.begin)
    }
}

impl Iterator for Measures {
    type Item = Measure;

    fn next(&mut self) -> Option<Measure> {
        let stretch = *self.stretches.get(self.stretch)?;
        let first = i64::from(i32::MIN);
        let last = i64::from(i32::MAX);
        if self.start > last {
            return None;
        }
        let end = self
            .start
            .saturating_add(stretch.length)
            .min(self.end_of(self.stretch));
        let shown = self.start.max(first);
        let length = u32::try_from(end.min(last.saturating_add(1)).saturating_sub(shown))
            .unwrap_or(u32::MAX);
        let measure = Measure {
            number: self.number,
            start: Tick::new(i32::try_from(shown).ok()?),
            length: Ticks::new(length)?,
            meter: stretch.meter,
        };
        if end == self.end_of(self.stretch) {
            self.stretch = self.stretch.saturating_add(1);
        }
        self.start = end;
        self.number = self.number.saturating_add(1);
        Some(measure)
    }
}

/// Ticks in a measure of `meter`, if that is a whole number.
fn measure_length(meter: TimeSignature, resolution: TicksPerBeat) -> Option<i64> {
    let quarters = std::num::NonZeroI64::from(meter.numerator)
        .get()
        .saturating_mul(4)
        .saturating_mul(i64::from(resolution.get()));
    let denominator = std::num::NonZeroI64::from(meter.denominator);
    (quarters.rem_euclid(denominator.get()) == 0).then_some(quarters.div_euclid(denominator.get()))
}

/// `n / d` rounded up, for a positive `d`.
const fn ceil_div(n: i64, d: i64) -> i64 {
    n.div_euclid(d)
        .saturating_add(if n.rem_euclid(d) != 0 { 1 } else { 0 })
}

impl Timing {
    /// The stretches of the grid: common time from tick 0 until the first time signature, then
    /// each time signature from its own tick. `None` if a measure is not a whole number of ticks.
    fn stretches(&self, resolution: TicksPerBeat) -> Option<Vec<Stretch>> {
        let mut stretches = vec![Stretch {
            begin: i64::MIN,
            origin: 0,
            length: measure_length(COMMON_TIME, resolution)?,
            meter: COMMON_TIME,
        }];
        for event in &self.events {
            if let TimingKind::TimeSignature(meter) = event.kind {
                let at = i64::from(event.tick.get());
                stretches.push(Stretch {
                    begin: at,
                    origin: at,
                    length: measure_length(meter, resolution)?,
                    meter,
                });
            }
        }
        Some(stretches)
    }

    /// The measures from the one holding `from` onward; see
    /// [`Valid::measures_from`](crate::Valid::measures_from). Events must be sorted by tick.
    /// `None` if a measure is not a whole number of ticks, which a valid document rules out.
    pub(crate) fn measures_from(&self, resolution: TicksPerBeat, from: Tick) -> Option<Measures> {
        let grid = Grid {
            stretches: self.stretches(resolution)?,
        };
        let (stretch, start) = grid.holding(i64::from(from.get()))?;
        let number = grid.number(start)?;
        Some(Measures {
            stretches: grid.stretches,
            stretch,
            start,
            number,
        })
    }

    /// Where `tick` falls in the grid; see [`Valid::grid_position`](crate::Valid::grid_position).
    /// `None` if a measure is not a whole number of ticks.
    pub(crate) fn grid_position(
        &self,
        resolution: TicksPerBeat,
        tick: Tick,
    ) -> Option<GridPosition> {
        let grid = Grid {
            stretches: self.stretches(resolution)?,
        };
        let tick = i64::from(tick.get());
        let (_, start) = grid.holding(tick)?;
        let along = tick.saturating_sub(start);
        let beat = i64::from(resolution.get());
        Some(GridPosition {
            measure: grid.number(start)?,
            beat: u32::try_from(along.div_euclid(beat)).ok()?,
            tick: u32::try_from(along.rem_euclid(beat)).ok()?,
        })
    }
}

/// A timing map's stretches, able to say where measures start and how they are numbered, in
/// ticks that may run past either end of what a document can hold.
struct Grid {
    /// Meter stretches used to locate and number measures.
    stretches: Vec<Stretch>,
}

impl Grid {
    /// The stretch holding `tick`, and where the measure holding it starts.
    fn holding(&self, tick: i64) -> Option<(usize, i64)> {
        let index = self
            .stretches
            .partition_point(|stretch| stretch.begin <= tick)
            .checked_sub(1)?;
        let stretch = self.stretches.get(index)?;
        let measures = tick
            .saturating_sub(stretch.origin)
            .div_euclid(stretch.length);
        Some((
            index,
            stretch
                .origin
                .saturating_add(measures.saturating_mul(stretch.length)),
        ))
    }

    /// The number of the measure starting at `start`: how many measures lie between it and the
    /// one holding tick 0.
    fn number(&self, start: i64) -> Option<i64> {
        let (_, zero) = self.holding(0)?;
        Some(if start >= zero {
            self.starts_between(zero, start)
        } else {
            self.starts_between(start, zero).saturating_neg()
        })
    }

    /// How many measures start in `from..to`.
    fn starts_between(&self, from: i64, to: i64) -> i64 {
        let ends = self
            .stretches
            .iter()
            .skip(1)
            .map(|next| next.begin)
            .chain(std::iter::once(i64::MAX));
        self.stretches
            .iter()
            .zip(ends)
            .map(|(stretch, end)| {
                let (low, high) = (from.max(stretch.begin), to.min(end));
                if low < high {
                    ceil_div(high.saturating_sub(stretch.origin), stretch.length).saturating_sub(
                        ceil_div(low.saturating_sub(stretch.origin), stretch.length),
                    )
                } else {
                    0
                }
            })
            .fold(0_i64, i64::saturating_add)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::document::{Anchor, DEFAULT_TICKS_PER_BEAT, TimingEvent, TimingKind};
    use crate::unit::{Bpm, Origin};

    fn meter(numerator: u16, denominator: u16) -> TimeSignature {
        TimeSignature {
            numerator: NonZeroU16::new(numerator).unwrap(),
            denominator: NonZeroU16::new(denominator).unwrap(),
        }
    }

    /// A timing map at 120 BPM with a time signature on each of `signatures`.
    fn timing(signatures: &[(i32, TimeSignature)]) -> Timing {
        let mut events = Vec::new();
        events.extend(signatures.iter().map(|&(tick, meter)| TimingEvent {
            tick: Tick::new(tick),
            kind: TimingKind::TimeSignature(meter),
            ext: BTreeMap::new(),
        }));
        events.sort_by_key(|event| event.tick);
        Timing {
            anchors: vec![Anchor {
                tick: Tick::new(0),
                seconds: Origin::try_from(0.0).unwrap(),
            }],
            tempo: Bpm::try_from(120.0).unwrap(),
            events,
        }
    }

    fn measure(number: i64, start: i32, length: u32, meter: TimeSignature) -> Measure {
        Measure {
            number,
            start: Tick::new(start),
            length: Ticks::new(length).unwrap(),
            meter,
        }
    }

    fn first_measures(timing: &Timing, from: i32, count: usize) -> Vec<Measure> {
        timing
            .measures_from(DEFAULT_TICKS_PER_BEAT, Tick::new(from))
            .unwrap()
            .take(count)
            .collect()
    }

    #[test]
    fn a_measure_places_its_ticks_by_fraction_and_back() {
        let bar = measure(1, 3840, 3840, meter(4, 4));
        assert!((bar.fraction(Tick::new(4800)) - 0.25).abs() < 1e-12);
        assert!((bar.fraction(Tick::new(0)) + 1.0).abs() < 1e-12);
        assert_eq!(bar.tick_at(0.25), Tick::new(4800));
        assert_eq!(bar.tick_at(-3.0), Tick::new(3840));
        assert_eq!(bar.tick_at(7.0), Tick::new(7680));
    }

    #[test]
    fn the_grid_is_four_four_from_tick_zero_until_a_time_signature() {
        assert_eq!(
            first_measures(&timing(&[]), 5000, 2),
            [
                measure(1, 3840, 3840, meter(4, 4)),
                measure(2, 7680, 3840, meter(4, 4))
            ]
        );
    }

    #[test]
    fn measures_before_the_one_holding_tick_zero_count_down() {
        assert_eq!(
            first_measures(&timing(&[]), -1, 2),
            [
                measure(-1, -3840, 3840, meter(4, 4)),
                measure(0, 0, 3840, meter(4, 4))
            ]
        );
    }

    #[test]
    fn a_time_signature_cuts_the_measure_in_progress_short_and_restarts_the_grid() {
        assert_eq!(
            first_measures(&timing(&[(5000, meter(7, 8))]), 3840, 3),
            [
                measure(1, 3840, 1160, meter(4, 4)),
                measure(2, 5000, 3360, meter(7, 8)),
                measure(3, 8360, 3360, meter(7, 8)),
            ]
        );
    }

    /// The meter that rules `tick` and the tick its grid starts from, from the definition alone:
    /// the last time signature at or before it, or common time from tick 0.
    fn ruling(signatures: &[(i32, TimeSignature)], tick: i32) -> (i32, TimeSignature) {
        signatures
            .iter()
            .rev()
            .find(|(at, _)| *at <= tick)
            .map_or_else(|| (0, meter(4, 4)), |&(at, meter)| (at, meter))
    }

    /// Whether `tick` starts a measure, found without stretches or counting.
    fn starts_a_measure(signatures: &[(i32, TimeSignature)], resolution: u16, tick: i32) -> bool {
        let (origin, meter) = ruling(signatures, tick);
        let quarters = std::num::NonZeroI64::from(meter.numerator)
            .get()
            .saturating_mul(4)
            .saturating_mul(i64::from(resolution));
        let denominator = std::num::NonZeroI64::from(meter.denominator);
        let length = quarters.div_euclid(denominator.get());
        i64::from(tick)
            .saturating_sub(i64::from(origin))
            .rem_euclid(length)
            == 0
    }

    proptest::proptest! {
        #![proptest_config(crate::proptest_config())]

        #[test]
        fn the_grid_agrees_with_checking_every_tick_against_the_definition(
            resolution in 1_u16..=6,
            placed in proptest::collection::btree_map(-150_i32..150, (1_u16..=7, proptest::sample::select(vec![1_u16, 2, 4])), 0..4),
            from in -150_i32..150,
        ) {
            let signatures: Vec<_> = placed.into_iter().map(|(tick, (n, d))| (tick, meter(n, d))).collect();
            let starts: Vec<i32> = (-700..700).filter(|&tick| starts_a_measure(&signatures, resolution, tick)).collect();
            let zero = starts.iter().rposition(|&tick| tick <= 0).unwrap();
            let resolution = TicksPerBeat::new(resolution).unwrap();
            let measures = timing(&signatures).measures_from(resolution, Tick::new(from)).unwrap();
            let mut expected = starts.iter().rposition(|&tick| tick <= from).unwrap();
            for measure in measures.take_while(|measure| measure.start.get() < 300) {
                let (start, next) = (starts[expected], starts[expected.saturating_add(1)]);
                proptest::prop_assert_eq!(measure, Measure {
                    number: i64::try_from(expected).unwrap().saturating_sub(i64::try_from(zero).unwrap()),
                    start: Tick::new(start),
                    length: Ticks::new(u32::try_from(next.saturating_sub(start)).unwrap()).unwrap(),
                    meter: ruling(&signatures, start).1,
                });
                expected = expected.saturating_add(1);
            }
            proptest::prop_assert!(starts[expected] >= 300, "measures stopped before {}", starts[expected]);
        }
    }

    #[test]
    fn a_measure_starting_on_the_last_tick_still_has_a_place() {
        let last = Tick::new(i32::MAX);
        let timing = timing(&[(i32::MAX, meter(4, 4))]);
        let measure = timing
            .measures_from(DEFAULT_TICKS_PER_BEAT, last)
            .unwrap()
            .next()
            .unwrap();
        assert_eq!(
            (measure.start, measure.length),
            (last, Ticks::new(1).unwrap())
        );
        let position = timing.grid_position(DEFAULT_TICKS_PER_BEAT, last).unwrap();
        assert_eq!((position.beat, position.tick), (0, 0));
    }

    #[test]
    fn the_first_tick_counts_from_where_its_measure_really_starts() {
        let position = timing(&[])
            .grid_position(DEFAULT_TICKS_PER_BEAT, Tick::new(i32::MIN))
            .unwrap();
        assert_eq!(
            position,
            GridPosition {
                measure: -559_241,
                beat: 1,
                tick: 832
            }
        );
    }

    #[test]
    fn a_tick_falls_in_a_measure_on_a_beat_some_ticks_along() {
        let position = timing(&[])
            .grid_position(
                DEFAULT_TICKS_PER_BEAT,
                Tick::new(
                    3840_i32
                        .saturating_add(2_i32.saturating_mul(960))
                        .saturating_add(120),
                ),
            )
            .unwrap();
        assert_eq!(
            position,
            GridPosition {
                measure: 1,
                beat: 2,
                tick: 120
            }
        );
        assert_eq!(position.to_string(), "1:2:120");
    }
}
