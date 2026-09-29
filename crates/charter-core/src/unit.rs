//! Types that give a number its meaning, so values of different units cannot be swapped.

use std::fmt;
use std::num::{NonZeroU16, NonZeroU32};

use serde::{Deserialize, Serialize};

use crate::number::{decimal, from_millionths};

decimal!(
    /// Tempo, in quarter notes per minute: a decimal above 0.
    ///
    /// ```
    /// # use charter_core::{Bpm, Timing};
    /// let tempo = Bpm::try_from(120.0).unwrap();
    /// # let anchors = Vec::new();
    /// # let events = Vec::new();
    /// let _ = Timing { anchors, tempo, events };
    /// ```
    ///
    /// A tempo is not a pause, though both are positive:
    ///
    /// ```compile_fail
    /// # use charter_core::{Bpm, TimingKind};
    /// let tempo = Bpm::try_from(120.0).unwrap();
    /// let _ = TimingKind::Stop(tempo);
    /// ```
    Bpm, "a tempo above 0 of at most six places", |m| m > 0
);

impl Bpm {
    /// The tempo nearest `value` that six places hold, as a derived tempo is shown and edited;
    /// none for a value that is not a tempo at all.
    #[must_use]
    pub fn nearest(value: f64) -> Option<Self> {
        Self::try_from((value * 1e6).round() / 1e6).ok()
    }

    /// Seconds that `ticks` last at this tempo and `resolution`.
    pub(crate) fn seconds(self, ticks: Ticks, resolution: TicksPerBeat) -> f64 {
        f64::from(ticks.0.get()) * 60.0 / (from_millionths(self.0) * f64::from(resolution.0.get()))
    }
}

decimal!(
    /// A length of time in seconds: a decimal above 0.
    ///
    /// ```
    /// # use charter_core::{Seconds, TimingKind};
    /// let pause = Seconds::try_from(0.5).unwrap();
    /// let _ = TimingKind::Stop(pause);
    /// ```
    ///
    /// A pause is not a tempo:
    ///
    /// ```compile_fail
    /// # use charter_core::{Seconds, Timing};
    /// let pause = Seconds::try_from(0.5).unwrap();
    /// # let anchors = Vec::new();
    /// # let events = Vec::new();
    /// let _ = Timing { anchors, tempo: pause, events };
    /// ```
    Seconds, "a length of time above 0 of at most six places", |m| m > 0
);

impl Seconds {
    /// Positive duration as floating-point seconds for timing calculations.
    pub(crate) fn seconds(self) -> f64 {
        from_millionths(self.0)
    }
}

decimal!(
    /// A point in the audio file, in seconds from its start: a decimal of at least 0.
    AudioTime, "a time of at least 0 with at most six places", |m| m >= 0
);

decimal!(
    /// The audio time of tick 0, in seconds from the start of the audio file. Negative when the
    /// chart starts before the audio does.
    Origin, "a time of at most six places", |_m| true
);

/// An audio time in seconds, resolved from a timing map rather than stored, so a float.
/// Negative before the audio starts.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct ResolvedTime(f64);

impl ResolvedTime {
    /// A computed audio time in seconds, possibly before the audio starts.
    pub(crate) const fn new(seconds: f64) -> Self {
        Self(seconds)
    }

    /// Seconds from the start of the audio file, for an audio API or a display.
    #[must_use]
    pub const fn seconds(self) -> f64 {
        self.0
    }
}

impl From<Origin> for ResolvedTime {
    fn from(origin: Origin) -> Self {
        Self(from_millionths(origin.0))
    }
}

impl From<AudioTime> for ResolvedTime {
    fn from(time: AudioTime) -> Self {
        Self(from_millionths(time.0))
    }
}

impl std::ops::Add<Seconds> for ResolvedTime {
    type Output = Self;

    fn add(self, rhs: Seconds) -> Self {
        Self(self.0 + rhs.seconds())
    }
}

/// A length in ticks, never zero.
///
/// ```
/// # use charter_core::{Shape, Ticks};
/// let span = Ticks::new(480).unwrap();
/// let _ = Shape::Sustain { duration: span };
/// ```
///
/// A bare number has to say that it counts ticks:
///
/// ```compile_fail
/// # use std::num::NonZeroU32;
/// # use charter_core::{Shape, Ticks};
/// let span = NonZeroU32::new(480).unwrap();
/// let _ = Shape::Sustain { duration: span };
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Ticks(NonZeroU32);

/// A musical position in ticks from beat 0, negative before it. Adding a span gives a tick, or
/// nothing past the last one, which a valid document never reaches.
///
/// ```
/// # use charter_core::{Shape, Tick, Ticks};
/// let start = Tick::new(-480);
/// let span = Ticks::new(960).unwrap();
/// assert_eq!(Shape::Sustain { duration: span }.end(start), Some(Tick::new(480)));
/// ```
///
/// A position and a length cannot trade places:
///
/// ```compile_fail
/// # use charter_core::{Shape, Tick, Ticks};
/// let start = Tick::new(-480);
/// let span = Ticks::new(960).unwrap();
/// assert_eq!(Shape::Sustain { duration: start }.end(span), Some(Tick::new(480)));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Tick(i32);

/// A lane, counting from 0.
///
/// ```
/// # use charter_core::{LaneCount, LaneIndex, Position};
/// let lane = LaneIndex::new(2);
/// let _ = Position::Lane { index: lane, width: LaneCount::ONE };
/// ```
///
/// An index is not a width:
///
/// ```compile_fail
/// # use charter_core::{LaneCount, LaneIndex, Position};
/// let lane = LaneIndex::new(2);
/// let _ = Position::Lane { index: lane, width: lane };
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LaneIndex(u16);

/// A number of lanes, never zero.
///
/// ```
/// # use charter_core::{LaneCount, LaneIndex, Position};
/// let both = LaneCount::new(2).unwrap();
/// let _ = Position::Lane { index: LaneIndex::new(1), width: both };
/// ```
///
/// A count is not an index:
///
/// ```compile_fail
/// # use charter_core::{LaneCount, LaneIndex, Position};
/// let both = LaneCount::new(2).unwrap();
/// let _ = Position::Lane { index: both, width: both };
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LaneCount(NonZeroU16);

impl LaneCount {
    /// The width of a single-lane position, and the default.
    pub const ONE: Self = Self(NonZeroU16::MIN);

    /// `count` lanes, if there are any.
    #[must_use]
    pub const fn new(count: u16) -> Option<Self> {
        match NonZeroU16::new(count) {
            Some(count) => Some(Self(count)),
            None => None,
        }
    }

    /// The last lane.
    #[must_use]
    pub const fn last(self) -> LaneIndex {
        LaneIndex(self.0.get().saturating_sub(1))
    }

    /// The middle lane, or the left of the middle two.
    #[must_use]
    pub const fn middle(self) -> LaneIndex {
        LaneIndex(self.0.get().saturating_sub(1).div_euclid(2))
    }
}

impl LaneIndex {
    /// Zero-based index; the chart's lane count determines whether it fits.
    #[must_use]
    pub const fn new(index: u16) -> Self {
        Self(index)
    }
}

/// The lanes a lane position covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LaneSpan {
    /// Inclusive first lane as a widened index.
    start: u32,
    /// Exclusive lane after the span, widened to accommodate a full `u16` width.
    end: u32,
}

impl LaneSpan {
    /// `width` lanes from `first`.
    #[must_use]
    pub fn new(first: LaneIndex, width: LaneCount) -> Self {
        let start = u32::from(first.0);
        Self {
            start,
            end: start.saturating_add(NonZeroU32::from(width.0).get()),
        }
    }

    /// Whether every lane in the span exists among `lanes`.
    #[must_use]
    pub fn fits(self, lanes: LaneCount) -> bool {
        self.end <= u32::from(lanes.0.get())
    }

    /// Whether the two spans share a lane.
    #[must_use]
    pub const fn overlaps(self, other: Self) -> bool {
        self.start < other.end && other.start < self.end
    }
}

/// Ticks per beat: the resolution of a document's time base.
///
/// ```
/// # use std::num::NonZeroU16;
/// # use charter_core::{LaneCount, TicksPerBeat, TimeSignature};
/// let seven_eight = TimeSignature { numerator: NonZeroU16::new(7).unwrap(), denominator: NonZeroU16::new(8).unwrap() };
/// let resolution = TicksPerBeat::new(480).unwrap();
/// assert!(seven_eight.measure_is_whole(resolution));
/// ```
///
/// A lane count is not a resolution:
///
/// ```compile_fail
/// # use std::num::NonZeroU16;
/// # use charter_core::{LaneCount, TicksPerBeat, TimeSignature};
/// let seven_eight = TimeSignature { numerator: NonZeroU16::new(7).unwrap(), denominator: NonZeroU16::new(8).unwrap() };
/// let resolution = LaneCount::new(480).unwrap();
/// assert!(seven_eight.measure_is_whole(resolution));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TicksPerBeat(NonZeroU16);

impl Tick {
    /// Signed tick position, including positions before tick zero.
    #[must_use]
    pub const fn new(tick: i32) -> Self {
        Self(tick)
    }

    /// Signed tick position for arithmetic and ordering.
    pub(crate) const fn get(self) -> i32 {
        self.0
    }

    /// The tick `span` after this one, or `None` past the last tick.
    #[must_use]
    pub fn checked_add(self, span: Ticks) -> Option<Self> {
        self.0.checked_add_unsigned(span.0.get()).map(Self)
    }

    /// The first tick after this one on a grid of lines `step` apart from tick 0, or `None` past
    /// the last tick.
    #[must_use]
    pub fn next_on(self, step: Ticks) -> Option<Self> {
        let step = std::num::NonZeroI64::from(step.0).get();
        let line = i64::from(self.0)
            .div_euclid(step)
            .saturating_add(1)
            .saturating_mul(step);
        i32::try_from(line).ok().map(Self)
    }

    /// The last tick before this one on a grid of lines `step` apart from tick 0, or `None`
    /// before the first tick.
    #[must_use]
    pub fn previous_on(self, step: Ticks) -> Option<Self> {
        let step = std::num::NonZeroI64::from(step.0).get();
        let line = i64::from(self.0)
            .saturating_sub(1)
            .div_euclid(step)
            .saturating_mul(step);
        i32::try_from(line).ok().map(Self)
    }

    /// How far `later` lies after this tick, or `None` unless it is later.
    #[must_use]
    pub fn ticks_until(self, later: Self) -> Option<Ticks> {
        u32::try_from(i64::from(later.0).saturating_sub(i64::from(self.0)))
            .ok()
            .and_then(Ticks::new)
    }
}

impl Ticks {
    /// A span of `ticks`, if it is not zero.
    #[must_use]
    pub const fn new(ticks: u32) -> Option<Self> {
        match NonZeroU32::new(ticks) {
            Some(ticks) => Some(Self(ticks)),
            None => None,
        }
    }

    /// Nonzero tick distance for calculations that need a plain count.
    pub(crate) const fn get(self) -> u32 {
        self.0.get()
    }
}

impl fmt::Display for LaneIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// A span of ticks as a number, as a grid of them is counted.
impl From<Ticks> for u32 {
    fn from(ticks: Ticks) -> Self {
        ticks.0.get()
    }
}

/// An editor counts ticks by number where it lays out or moves many of them at once.
impl From<Tick> for i64 {
    fn from(tick: Tick) -> Self {
        Self::from(tick.0)
    }
}

/// A format converter counts ticks by number, turning another program's times into them.
impl From<TicksPerBeat> for u16 {
    fn from(resolution: TicksPerBeat) -> Self {
        resolution.0.get()
    }
}

/// A view places lanes by number, which is the one place a lane stops being a type.
impl From<LaneIndex> for u16 {
    fn from(lane: LaneIndex) -> Self {
        lane.0
    }
}

impl From<LaneCount> for u16 {
    fn from(lanes: LaneCount) -> Self {
        lanes.0.get()
    }
}

impl fmt::Display for LaneCount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Display for TimeSignature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.numerator, self.denominator)
    }
}

impl fmt::Display for Ticks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Display for Tick {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// A meter: `numerator` beats of `1/denominator` notes per measure. Written as
/// `[numerator, denominator]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "(NonZeroU16, NonZeroU16)", into = "(NonZeroU16, NonZeroU16)")]
pub struct TimeSignature {
    /// Number of denominator-sized beats in a measure.
    pub numerator: NonZeroU16,
    /// Note value of each beat relative to a whole note.
    pub denominator: NonZeroU16,
}

impl TimeSignature {
    /// Four quarter notes a measure, the meter a song has unless it says otherwise.
    pub const COMMON: Self = match NonZeroU16::new(4) {
        Some(four) => Self {
            numerator: four,
            denominator: four,
        },
        None => Self {
            numerator: NonZeroU16::MIN,
            denominator: NonZeroU16::MIN,
        },
    };

    /// Whether a measure, `numerator * 4 / denominator` beats, is a whole number of ticks at
    /// `resolution`.
    #[must_use]
    pub fn measure_is_whole(self, resolution: TicksPerBeat) -> bool {
        let quarters = std::num::NonZeroU64::from(self.numerator)
            .get()
            .saturating_mul(4)
            .saturating_mul(std::num::NonZeroU64::from(resolution.0).get());
        quarters.is_multiple_of(std::num::NonZeroU64::from(self.denominator).get())
    }
}

impl TicksPerBeat {
    /// A resolution of `ticks` per beat, if it is not zero.
    #[must_use]
    pub const fn new(ticks: u16) -> Option<Self> {
        match NonZeroU16::new(ticks) {
            Some(ticks) => Some(Self(ticks)),
            None => None,
        }
    }

    /// Nonzero number of ticks in a quarter-note beat.
    pub(crate) const fn get(self) -> u16 {
        self.0.get()
    }

    /// How long an `n`th note lasts: `4 / n` beats, if that is a whole number of ticks.
    #[must_use]
    pub fn note_length(self, n: NonZeroU16) -> Option<Ticks> {
        let quarters = 4_u32.saturating_mul(NonZeroU32::from(self.0).get());
        let n = NonZeroU32::from(n).get();
        if quarters.is_multiple_of(n) {
            Ticks::new(quarters.div_euclid(n))
        } else {
            None
        }
    }
}

impl From<(NonZeroU16, NonZeroU16)> for TimeSignature {
    fn from((numerator, denominator): (NonZeroU16, NonZeroU16)) -> Self {
        Self {
            numerator,
            denominator,
        }
    }
}

impl From<TimeSignature> for (NonZeroU16, NonZeroU16) {
    fn from(meter: TimeSignature) -> Self {
        (meter.numerator, meter.denominator)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{Position, Shape, TimingEvent, TimingKind};

    #[test]
    fn a_time_signature_names_its_parts_and_travels_as_a_pair() {
        let meter: TimeSignature = serde_json::from_value(json!([7, 8])).unwrap();
        assert_eq!((meter.numerator.get(), meter.denominator.get()), (7, 8));
        assert_eq!(serde_json::to_value(meter).unwrap(), json!([7, 8]));
    }

    #[test]
    fn a_timing_map_carries_a_tail_tempo() {
        let timing: crate::Timing = serde_json::from_value(json!({
            "anchors": [ { "tick": 0, "seconds": 0.0 } ],
            "tempo": 120.0,
            "events": []
        }))
        .unwrap();
        assert_eq!(timing.tempo, Bpm::try_from(120.0).unwrap());
    }

    #[test]
    fn stop_and_delay_events_carry_seconds() {
        let half = || Seconds::try_from(0.5).unwrap();
        let stop: TimingKind = serde_json::from_value(json!({ "stop": 0.5 })).unwrap();
        let delay: TimingKind = serde_json::from_value(json!({ "delay": 0.5 })).unwrap();
        assert_eq!(
            (stop, delay),
            (TimingKind::Stop(half()), TimingKind::Delay(half()))
        );
    }

    #[test]
    fn warps_and_sustains_span_ticks() {
        let warp: TimingKind = serde_json::from_value(json!({ "warp": 480 })).unwrap();
        let sustain: Shape =
            serde_json::from_value(json!({ "sustain": { "duration": 480 } })).unwrap();
        let span = Ticks::new(480).unwrap();
        assert_eq!(
            (warp, sustain),
            (TimingKind::Warp(span), Shape::Sustain { duration: span })
        );
    }

    #[test]
    fn timing_events_sit_on_ticks() {
        let event: TimingEvent =
            serde_json::from_value(json!({ "tick": -480, "kind": { "stop": 1.0 } })).unwrap();
        assert_eq!(event.tick, Tick::new(-480));
    }

    #[test]
    fn lane_positions_start_at_a_lane_index() {
        let position: Position = serde_json::from_value(json!({ "lane": { "index": 2 } })).unwrap();
        assert!(matches!(position, Position::Lane { index, .. } if index == LaneIndex::new(2)));
    }

    #[test]
    fn lane_positions_are_a_lane_count_wide() {
        let position: Position =
            serde_json::from_value(json!({ "lane": { "index": 0, "width": 3 } })).unwrap();
        assert!(
            matches!(position, Position::Lane { width, .. } if Some(width) == LaneCount::new(3))
        );
    }

    #[test]
    fn tempos_and_pauses_are_positive_six_place_decimals() {
        for bad in [json!(0.0), json!(-1.0), json!(120.123_456_7)] {
            assert!(serde_json::from_value::<Bpm>(bad.clone()).is_err(), "{bad}");
            assert!(
                serde_json::from_value::<Seconds>(bad.clone()).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn the_origin_may_precede_the_audio_but_a_preview_may_not() {
        serde_json::from_value::<Origin>(json!(-0.014)).unwrap();
        serde_json::from_value::<AudioTime>(json!(0.0)).unwrap();
        serde_json::from_value::<AudioTime>(json!(-0.000_001)).unwrap_err();
    }

    #[test]
    fn a_tick_plus_a_span_is_a_tick_up_to_the_last_one() {
        let span = Ticks::new(480).unwrap();
        assert_eq!(Tick::new(-480).checked_add(span), Some(Tick::new(0)));
        assert_eq!(
            Tick::new(i32::MAX - 480).checked_add(span),
            Some(Tick::new(i32::MAX))
        );
        assert_eq!(Tick::new(i32::MAX - 479).checked_add(span), None);
    }

    #[test]
    fn a_tick_steps_forward_to_the_next_line_of_a_grid() {
        let step = Ticks::new(240).unwrap();
        assert_eq!(Tick::new(0).next_on(step), Some(Tick::new(240)));
        assert_eq!(Tick::new(100).next_on(step), Some(Tick::new(240)));
        assert_eq!(Tick::new(-100).next_on(step), Some(Tick::new(0)));
        assert_eq!(Tick::new(i32::MAX - 100).next_on(step), None);
    }

    #[test]
    fn a_tick_steps_back_to_the_last_line_of_a_grid_before_it() {
        let step = Ticks::new(240).unwrap();
        assert_eq!(Tick::new(240).previous_on(step), Some(Tick::new(0)));
        assert_eq!(Tick::new(250).previous_on(step), Some(Tick::new(240)));
        assert_eq!(Tick::new(0).previous_on(step), Some(Tick::new(-240)));
        assert_eq!(Tick::new(i32::MIN + 100).previous_on(step), None);
    }

    proptest::proptest! {
        #![proptest_config(crate::proptest_config())]

        #[test]
        fn stepping_along_a_grid_finds_the_nearest_line_each_way(tick in -500_i32..500, step in 1_u32..=64) {
            let gap = i32::try_from(step).unwrap();
            let on_grid = |k: &i32| k.rem_euclid(gap) == 0;
            let next = (tick.saturating_add(1)..tick.saturating_add(100)).find(on_grid).unwrap();
            let previous = (tick.saturating_sub(100)..tick).rev().find(on_grid).unwrap();
            let span = Ticks::new(step).unwrap();
            proptest::prop_assert_eq!(Tick::new(tick).next_on(span), Some(Tick::new(next)));
            proptest::prop_assert_eq!(Tick::new(tick).previous_on(span), Some(Tick::new(previous)));
        }
    }

    #[test]
    fn a_later_tick_is_a_span_away() {
        assert_eq!(Tick::new(-480).ticks_until(Tick::new(480)), Ticks::new(960));
        assert_eq!(
            Tick::new(i32::MIN).ticks_until(Tick::new(i32::MAX)),
            Ticks::new(u32::MAX)
        );
        assert_eq!(Tick::new(5).ticks_until(Tick::new(5)), None);
        assert_eq!(Tick::new(5).ticks_until(Tick::new(4)), None);
    }

    #[test]
    fn an_nth_note_lasts_four_nths_of_a_beat_when_that_is_whole() {
        let note = |resolution: u16, n: u16| {
            TicksPerBeat::new(resolution)
                .unwrap()
                .note_length(NonZeroU16::new(n).unwrap())
        };
        assert_eq!(note(960, 4), Ticks::new(960));
        assert_eq!(note(960, 12), Ticks::new(320));
        assert_eq!(note(960, 192), Ticks::new(20));
        assert_eq!(note(48, 64), Ticks::new(3));
        assert_eq!(note(960, 7), None);
        assert_eq!(note(1, 8), None);
    }

    #[test]
    fn a_span_of_ticks_shows_as_its_count() {
        assert_eq!(Ticks::new(1920).unwrap().to_string(), "1920");
    }

    #[test]
    fn lanes_give_their_numbers_to_a_view_that_places_them() {
        assert_eq!(u16::from(LaneIndex::new(3)), 3);
        assert_eq!(u16::from(LaneCount::new(4).unwrap()), 4);
    }

    #[test]
    fn a_lane_count_names_its_last_and_middle_lanes() {
        let five = LaneCount::new(5).unwrap();
        assert_eq!(
            (five.last(), five.middle()),
            (LaneIndex::new(4), LaneIndex::new(2))
        );
        let four = LaneCount::new(4).unwrap();
        assert_eq!(
            (four.last(), four.middle()),
            (LaneIndex::new(3), LaneIndex::new(1))
        );
        assert_eq!(LaneCount::new(0), None);
    }

    #[test]
    fn a_lane_span_fits_a_chart_and_overlaps_its_neighbours() {
        let span =
            |index, width| LaneSpan::new(LaneIndex::new(index), LaneCount::new(width).unwrap());
        let wide = span(2, 3);
        assert!(wide.fits(LaneCount::new(5).unwrap()));
        assert!(!wide.fits(LaneCount::new(4).unwrap()));
        assert!(wide.overlaps(span(4, 1)) && span(4, 1).overlaps(wide));
        assert!(!wide.overlaps(span(5, 1)) && !wide.overlaps(span(0, 2)));
    }

    #[test]
    fn a_measure_is_whole_only_when_the_resolution_allows() {
        let meter = |numerator, denominator| TimeSignature {
            numerator: NonZeroU16::new(numerator).unwrap(),
            denominator: NonZeroU16::new(denominator).unwrap(),
        };
        let fine = TicksPerBeat::new(960).unwrap();
        let coarse = TicksPerBeat::new(48).unwrap();
        assert!(meter(7, 8).measure_is_whole(fine) && meter(1, 64).measure_is_whole(coarse));
        assert!(!meter(1, 128).measure_is_whole(coarse));
    }

    #[test]
    fn resolved_times_start_at_stored_points_and_move_by_lengths() {
        let origin = ResolvedTime::from(Origin::try_from(-0.25).unwrap());
        let later = origin + Seconds::try_from(1.5).unwrap();
        assert_eq!(
            later,
            ResolvedTime::from(AudioTime::try_from(1.25).unwrap())
        );
        assert!(origin < later);
    }

    #[test]
    fn ticks_at_the_integer_edges_refuse_steps_past_the_range() {
        let one = Ticks::new(1).unwrap();
        let largest = Ticks::new(u32::MAX).unwrap();
        assert_eq!(Tick::new(i32::MAX).checked_add(one), None);
        assert_eq!(
            Tick::new(i32::MIN).checked_add(largest),
            Some(Tick::new(i32::MAX))
        );
        assert_eq!(Tick::new(i32::MAX).next_on(largest), None);
        assert_eq!(Tick::new(i32::MIN).previous_on(largest), None);
        assert_eq!(
            Tick::new(i32::MIN).ticks_until(Tick::new(i32::MAX)),
            Some(largest)
        );
    }

    #[test]
    fn the_largest_resolution_and_meter_do_not_overflow() {
        let resolution = TicksPerBeat::new(u16::MAX).unwrap();
        assert_eq!(resolution.note_length(NonZeroU16::MIN), Ticks::new(262_140));
        let meter = TimeSignature {
            numerator: NonZeroU16::MAX,
            denominator: NonZeroU16::MIN,
        };
        assert!(meter.measure_is_whole(resolution));
    }
}

/// Proofs for Kani, run with `cargo kani -p charter-core`. Each input ranges over every value its
/// type allows, and Kani also fails a proof on any arithmetic overflow.
#[cfg(kani)]
mod proofs {
    use super::*;

    fn tick() -> Tick {
        Tick(kani::any())
    }

    fn span() -> Ticks {
        Ticks(kani::any())
    }

    fn lanes() -> LaneCount {
        LaneCount(kani::any())
    }

    fn lane_span() -> LaneSpan {
        LaneSpan::new(LaneIndex(kani::any()), lanes())
    }

    #[kani::proof]
    fn a_tick_plus_a_span_is_a_tick_exactly_when_it_fits() {
        let (start, span) = (tick(), span());
        let sum = i64::from(start.0) + i64::from(span.0.get());
        match start.checked_add(span) {
            Some(end) => {
                assert!(i64::from(end.0) == sum);
            }
            None => {
                assert!(sum > i64::from(i32::MAX));
            }
        }
    }

    #[kani::proof]
    fn a_later_tick_is_the_span_that_reaches_it() {
        let (from, to) = (tick(), tick());
        match from.ticks_until(to) {
            Some(span) => {
                assert!(to > from && from.checked_add(span) == Some(to));
            }
            None => {
                assert!(to <= from);
            }
        }
    }

    #[kani::proof]
    fn stepping_forward_finds_the_first_representable_grid_line_after_a_tick() {
        let (tick, step) = (tick(), Ticks::new(256).unwrap());
        let step_size = i64::from(step.0.get());
        let answer = tick.next_on(step);
        let expected = (i64::from(tick.0).div_euclid(step_size) + 1) * step_size;
        assert!(
            answer.map(|line| i64::from(line.0)) == i32::try_from(expected).ok().map(i64::from)
        );
        if let Some(line) = answer {
            assert!(line > tick && i64::from(line.0).rem_euclid(step_size) == 0);
        }
    }

    #[kani::proof]
    fn stepping_backward_finds_the_last_representable_grid_line_before_a_tick() {
        let (tick, step) = (tick(), Ticks::new(256).unwrap());
        let step_size = i64::from(step.0.get());
        let answer = tick.previous_on(step);
        let expected = (i64::from(tick.0) - 1).div_euclid(step_size) * step_size;
        assert!(
            answer.map(|line| i64::from(line.0)) == i32::try_from(expected).ok().map(i64::from)
        );
        if let Some(line) = answer {
            assert!(line < tick && i64::from(line.0).rem_euclid(step_size) == 0);
        }
    }

    #[kani::proof]
    fn a_quarter_note_has_exactly_the_resolution_in_ticks() {
        let resolution = TicksPerBeat(kani::any());
        let length = resolution.note_length(NonZeroU16::new(4).unwrap());
        assert!(length.map(u32::from) == Some(u32::from(resolution.0.get())));
    }

    #[kani::proof]
    fn a_lane_counts_last_and_middle_are_inside_its_bounds() {
        let lanes = lanes();
        let count = u32::from(lanes.0.get());
        assert!(u32::from(lanes.last().0) + 1 == count);
        assert!(u32::from(lanes.middle().0) == (count - 1) / 2);
    }

    #[kani::proof]
    fn lane_spans_overlap_exactly_when_they_share_a_lane() {
        let (a, b) = (lane_span(), lane_span());
        let lane: u32 = kani::any();
        let in_both = a.start <= lane && lane < a.end && b.start <= lane && lane < b.end;
        if in_both {
            assert!(a.overlaps(b));
        }
        if a.overlaps(b) {
            let shared = a.start.max(b.start);
            assert!(shared < a.end && shared < b.end);
        }
        assert!(a.overlaps(b) == b.overlaps(a));
    }

    #[kani::proof]
    fn a_span_fits_exactly_when_its_last_lane_exists() {
        let (span, lanes) = (lane_span(), lanes());
        assert!(span.fits(lanes) == (span.end - 1 < u32::from(lanes.0.get())));
    }

    #[kani::proof]
    fn the_middle_lane_is_never_past_the_last_and_the_last_exists() {
        let lanes = lanes();
        assert!(lanes.middle() <= lanes.last());
        assert!(u32::from(lanes.last().0) < u32::from(lanes.0.get()));
    }

    #[kani::proof]
    fn meters_over_one_two_or_four_are_whole_at_every_resolution() {
        let denominator = match kani::any::<u8>() % 3 {
            0 => NonZeroU16::MIN,
            1 => NonZeroU16::MIN.saturating_add(1),
            _ => NonZeroU16::MIN.saturating_add(3),
        };
        let meter = TimeSignature {
            numerator: kani::any(),
            denominator,
        };
        assert!(meter.measure_is_whole(TicksPerBeat(kani::any())));
    }

    #[kani::proof]
    fn checking_any_meter_at_any_resolution_cannot_overflow() {
        let meter = TimeSignature {
            numerator: kani::any(),
            denominator: kani::any(),
        };
        let _ = meter.measure_is_whole(TicksPerBeat(kani::any()));
    }
}
