//! Types that give a number its meaning, so values of different units cannot be swapped.

use std::fmt;
use std::num::{NonZeroU16, NonZeroU32};

use serde::{Deserialize, Serialize};

use crate::number::{decimal, from_millionths};

decimal!(
    /// Tempo, in quarter notes per minute: a decimal above 0.
    ///
    /// ```
    /// # use charter_core::{Bpm, TimingKind};
    /// let tempo = Bpm::try_from(120.0).unwrap();
    /// let _ = TimingKind::Bpm(tempo);
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
    /// # use charter_core::{Seconds, TimingKind};
    /// let pause = Seconds::try_from(0.5).unwrap();
    /// let _ = TimingKind::Bpm(pause);
    /// ```
    Seconds, "a length of time above 0 of at most six places", |m| m > 0
);

impl Seconds {
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
    pub(crate) fn new(seconds: f64) -> Self {
        Self(seconds)
    }

    /// Seconds from the start of the audio file, for an audio API or a display.
    #[must_use]
    pub fn seconds(self) -> f64 {
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

    fn add(self, length: Seconds) -> Self {
        Self(self.0 + length.seconds())
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
        LaneIndex(self.0.get() - 1)
    }

    /// The middle lane, or the left of the middle two.
    #[must_use]
    pub const fn middle(self) -> LaneIndex {
        LaneIndex((self.0.get() - 1) / 2)
    }
}

impl LaneIndex {
    #[must_use]
    pub const fn new(index: u16) -> Self {
        Self(index)
    }
}

/// The lanes a lane position covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LaneSpan {
    start: u32,
    end: u32,
}

impl LaneSpan {
    /// `width` lanes from `first`.
    #[must_use]
    pub fn new(first: LaneIndex, width: LaneCount) -> Self {
        let start = u32::from(first.0);
        Self {
            start,
            end: start + u32::from(width.0.get()),
        }
    }

    /// Whether every lane in the span exists among `lanes`.
    #[must_use]
    pub fn fits(self, lanes: LaneCount) -> bool {
        self.end <= u32::from(lanes.0.get())
    }

    /// Whether the two spans share a lane.
    #[must_use]
    pub fn overlaps(self, other: Self) -> bool {
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
    #[must_use]
    pub const fn new(tick: i32) -> Self {
        Self(tick)
    }

    /// The tick `span` after this one, or `None` past the last tick.
    #[must_use]
    pub fn checked_add(self, span: Ticks) -> Option<Self> {
        self.0.checked_add_unsigned(span.0.get()).map(Self)
    }

    /// How far `later` lies after this tick, or `None` unless it is later.
    #[must_use]
    pub fn ticks_until(self, later: Self) -> Option<Ticks> {
        u32::try_from(i64::from(later.0) - i64::from(self.0))
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
}

impl fmt::Display for LaneIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
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
    pub numerator: NonZeroU16,
    pub denominator: NonZeroU16,
}

impl TimeSignature {
    /// Whether a measure, `numerator * 4 / denominator` beats, is a whole number of ticks at
    /// `resolution`.
    #[must_use]
    pub fn measure_is_whole(self, resolution: TicksPerBeat) -> bool {
        let quarters = u64::from(self.numerator.get()) * 4 * u64::from(resolution.0.get());
        quarters.is_multiple_of(u64::from(self.denominator.get()))
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
    fn a_bpm_event_carries_a_tempo() {
        let kind: TimingKind = serde_json::from_value(json!({ "bpm": 120.0 })).unwrap();
        assert_eq!(kind, TimingKind::Bpm(Bpm::try_from(120.0).unwrap()));
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
            serde_json::from_value(json!({ "tick": -480, "kind": { "bpm": 120.0 } })).unwrap();
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
        assert!(serde_json::from_value::<Origin>(json!(-0.014)).is_ok());
        assert!(serde_json::from_value::<AudioTime>(json!(0.0)).is_ok());
        assert!(serde_json::from_value::<AudioTime>(json!(-0.000_001)).is_err());
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
}
