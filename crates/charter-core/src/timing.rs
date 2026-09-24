use crate::document::{Timing, TimingKind};
use crate::unit::{Bpm, ResolvedTime, Tick, TicksPerBeat};

impl Timing {
    /// Audio time of an object on `tick`; see [`Valid::time_at`](crate::Valid::time_at). Events
    /// must be sorted by tick. `None` means there is no `bpm` event or a warp ends past the last
    /// tick, which a valid document rules out.
    pub(crate) fn seconds_at(
        &self,
        ticks_per_beat: TicksPerBeat,
        tick: Tick,
    ) -> Option<ResolvedTime> {
        let offset = ResolvedTime::from(self.offset_seconds).seconds();
        let zero = Tick::new(0);
        Some(ResolvedTime::new(if tick >= zero {
            offset + self.elapsed(ticks_per_beat, zero, tick)?
        } else {
            offset - self.elapsed(ticks_per_beat, tick, zero)?
        }))
    }

    /// Seconds between objects on `from` and on `to`, where `from <= to`.
    fn elapsed(&self, ticks_per_beat: TicksPerBeat, from: Tick, to: Tick) -> Option<f64> {
        // Ticks before `warp_end` take no time. A warp starts at its own tick, which the cursor
        // has always reached, so only the part of a span after `warp_end` counts.
        let timed = |cursor: Tick, until: Tick, warp_end: Option<Tick>, bpm: Bpm| {
            let start = warp_end.map_or(cursor, |end| end.max(cursor));
            start
                .ticks_until(until)
                .map_or(0.0, |ticks| bpm.seconds(ticks, ticks_per_beat))
        };
        let mut bpm = self.events.iter().find_map(|event| match event.kind {
            TimingKind::Bpm(value) => Some(value),
            TimingKind::Stop(_)
            | TimingKind::Delay(_)
            | TimingKind::Warp(_)
            | TimingKind::TimeSignature(..)
            | TimingKind::Other { .. } => None,
        })?;
        let mut elapsed = 0.0;
        let mut cursor = from;
        let mut warp_end = None;
        for event in &self.events {
            let at = event.tick;
            if at > to {
                break;
            }
            if at > from {
                elapsed += timed(cursor, at, warp_end, bpm);
                cursor = at;
            }
            match event.kind {
                TimingKind::Bpm(value) => bpm = value,
                TimingKind::Stop(pause) if at >= from && at < to => elapsed += pause.seconds(),
                TimingKind::Delay(pause) if at > from => elapsed += pause.seconds(),
                TimingKind::Warp(ticks) => warp_end = Some(at.checked_add(ticks)?),
                // Meter shapes the grid, not time; plugin events are never interpreted.
                TimingKind::Stop(_)
                | TimingKind::Delay(_)
                | TimingKind::TimeSignature(..)
                | TimingKind::Other { .. } => {}
            }
        }
        Some(elapsed + timed(cursor, to, warp_end, bpm))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashSet};
    use std::mem::discriminant;
    use std::num::NonZeroU16;

    use proptest::collection::vec;
    use proptest::prelude::*;

    use super::*;
    use crate::document::{DEFAULT_TICKS_PER_BEAT, TimingEvent, TimingKind};
    use crate::number::{MAX_MILLIONTHS, from_millionths};
    use crate::unit::{Origin, Seconds, Ticks, TimeSignature};

    fn timing(events: &[(i32, TimingKind)]) -> Timing {
        Timing {
            offset_seconds: Origin::try_from(1.0).unwrap(),
            events: events
                .iter()
                .map(|(tick, kind)| TimingEvent {
                    tick: Tick::new(*tick),
                    kind: kind.clone(),
                    ext: std::collections::BTreeMap::new(),
                })
                .collect(),
        }
    }

    fn bpm(value: f64) -> TimingKind {
        TimingKind::Bpm(Bpm::try_from(value).unwrap())
    }

    #[track_caller]
    fn assert_time(timing: &Timing, tick: i32, expected: f64) {
        let actual = timing
            .seconds_at(DEFAULT_TICKS_PER_BEAT, Tick::new(tick))
            .unwrap()
            .seconds();
        assert!(
            (actual - expected).abs() < 1e-9,
            "tick {tick}: {actual} != {expected}"
        );
    }

    #[test]
    fn bpm_sets_how_fast_ticks_become_seconds() {
        let t = timing(&[(0, bpm(60.0)), (1920, bpm(120.0))]);
        assert_time(&t, 0, 1.0);
        assert_time(&t, 960, 2.0);
        assert_time(&t, 2880, 3.5);
    }

    #[test]
    fn the_first_bpm_also_rules_the_ticks_before_it() {
        let t = timing(&[(960, bpm(60.0))]);
        assert_time(&t, 480, 1.5);
        assert_time(&t, 1440, 2.5);
    }

    #[test]
    fn a_stop_pauses_after_the_objects_on_its_tick() {
        let stop = TimingKind::Stop(Seconds::try_from(2.0).unwrap());
        let t = timing(&[(0, bpm(60.0)), (960, stop)]);
        assert_time(&t, 960, 2.0);
        assert_time(&t, 1920, 5.0);
    }

    #[test]
    fn a_delay_pauses_before_the_objects_on_its_tick() {
        let delay = TimingKind::Delay(Seconds::try_from(2.0).unwrap());
        let t = timing(&[(0, bpm(60.0)), (960, delay)]);
        assert_time(&t, 480, 1.5);
        assert_time(&t, 960, 4.0);
    }

    #[test]
    fn a_warp_skips_its_ticks_without_taking_time() {
        let warp = TimingKind::Warp(Ticks::new(960).unwrap());
        let t = timing(&[(0, bpm(60.0)), (960, warp)]);
        for warped in [960, 1500, 1920] {
            assert_time(&t, warped, 2.0);
        }
        assert_time(&t, 2880, 3.0);
    }

    #[test]
    fn pauses_inside_a_warp_still_pause() {
        let warp = TimingKind::Warp(Ticks::new(960).unwrap());
        let stop = TimingKind::Stop(Seconds::try_from(0.5).unwrap());
        let t = timing(&[(0, bpm(60.0)), (960, warp), (1200, stop)]);
        assert_time(&t, 1200, 2.0);
        assert_time(&t, 1920, 2.5);
    }

    #[test]
    fn negative_ticks_resolve_backward_through_pauses() {
        let stop = TimingKind::Stop(Seconds::try_from(1.0).unwrap());
        let t = timing(&[(-960, stop), (0, bpm(60.0))]);
        assert_time(&t, -960, -1.0);
        assert_time(&t, -1920, -2.0);
    }

    #[test]
    fn a_delay_on_tick_zero_is_inside_the_offset() {
        let delay = TimingKind::Delay(Seconds::try_from(2.0).unwrap());
        let t = timing(&[(0, bpm(60.0)), (0, delay)]);
        assert_time(&t, 0, 1.0);
        assert_time(&t, 960, 2.0);
    }

    #[test]
    fn timing_without_a_bpm_has_no_times() {
        assert_eq!(
            timing(&[]).seconds_at(DEFAULT_TICKS_PER_BEAT, Tick::new(0)),
            None
        );
    }

    #[test]
    fn the_resolution_sets_how_many_ticks_make_a_beat() {
        let t = timing(&[(0, bpm(60.0))]);
        let half = TicksPerBeat::new(480).unwrap();
        let seconds = t.seconds_at(half, Tick::new(480)).unwrap().seconds();
        assert!((seconds - 2.0).abs() < 1e-9, "{seconds}");
    }

    /// A timing map built from `first` and `events`, keeping only what validation accepts: one
    /// event of each kind per tick, and warps that neither overlap nor end past the last tick.
    fn valid_timing(offset: i64, first: (i32, Bpm), events: Vec<(i32, TimingKind)>) -> Timing {
        let mut events: Vec<_> = std::iter::once((first.0, TimingKind::Bpm(first.1)))
            .chain(events)
            .collect();
        events.sort_by_key(|(tick, _)| *tick);
        let mut seen = HashSet::new();
        let mut warp_end = None;
        let mut kept = Vec::new();
        for (tick, kind) in events {
            let tick = Tick::new(tick);
            if !seen.insert((tick, discriminant(&kind))) {
                continue;
            }
            if let TimingKind::Warp(span) = kind {
                match tick.checked_add(span) {
                    Some(end) if warp_end.is_none_or(|last| tick >= last) => warp_end = Some(end),
                    _ => continue,
                }
            }
            kept.push(TimingEvent {
                tick,
                kind,
                ext: BTreeMap::new(),
            });
        }
        Timing {
            offset_seconds: Origin::try_from(from_millionths(offset)).unwrap(),
            events: kept,
        }
    }

    /// The time of `tick` found by walking the map one tick at a time: the format chapter's
    /// rules restated without segments.
    fn walked(timing: &Timing, resolution: TicksPerBeat, tick: i32) -> f64 {
        let bpms = || {
            timing.events.iter().filter_map(|event| match event.kind {
                TimingKind::Bpm(bpm) => Some((event.tick, bpm)),
                _ => None,
            })
        };
        let first = bpms().next().unwrap().1;
        let bpm_at = |k: Tick| bpms().rev().find(|(at, _)| *at <= k).map_or(first, |b| b.1);
        let warped = |k: Tick| {
            timing.events.iter().any(|event| match event.kind {
                TimingKind::Warp(span) => event.tick <= k && Some(k) < event.tick.checked_add(span),
                _ => false,
            })
        };
        let pauses = |k: Tick, stops: bool| -> f64 {
            timing
                .events
                .iter()
                .filter(|event| event.tick == k)
                .map(|event| match event.kind {
                    TimingKind::Stop(pause) if stops => pause.seconds(),
                    TimingKind::Delay(pause) if !stops => pause.seconds(),
                    _ => 0.0,
                })
                .sum()
        };
        let one = Ticks::new(1).unwrap();
        let (from, to) = if tick >= 0 { (0, tick) } else { (tick, 0) };
        let mut elapsed = 0.0;
        for k in from..to {
            // A stop on `k` holds after its objects; a delay on `k + 1` holds before its own.
            elapsed += pauses(Tick::new(k), true);
            if !warped(Tick::new(k)) {
                elapsed += bpm_at(Tick::new(k)).seconds(one, resolution);
            }
            elapsed += pauses(Tick::new(k + 1), false);
        }
        let offset = ResolvedTime::from(timing.offset_seconds).seconds();
        if tick >= 0 {
            offset + elapsed
        } else {
            offset - elapsed
        }
    }

    fn tempo(millionths: impl Strategy<Value = i64>) -> impl Strategy<Value = Bpm> {
        millionths.prop_map(|m| Bpm::try_from(from_millionths(m)).unwrap())
    }

    fn pause(millionths: impl Strategy<Value = i64>) -> impl Strategy<Value = Seconds> {
        millionths.prop_map(|m| Seconds::try_from(from_millionths(m)).unwrap())
    }

    fn kind(
        tempos: impl Strategy<Value = i64>,
        pauses: impl Strategy<Value = i64> + Clone,
        warps: impl Strategy<Value = u32>,
    ) -> impl Strategy<Value = TimingKind> {
        let seven_eight = TimeSignature {
            numerator: NonZeroU16::new(7).unwrap(),
            denominator: NonZeroU16::new(8).unwrap(),
        };
        prop_oneof![
            tempo(tempos).prop_map(TimingKind::Bpm),
            pause(pauses.clone()).prop_map(TimingKind::Stop),
            pause(pauses).prop_map(TimingKind::Delay),
            warps.prop_map(|w| TimingKind::Warp(Ticks::new(w).unwrap())),
            Just(TimingKind::TimeSignature(seven_eight)),
        ]
    }

    /// Any tick, with the ends of the range and zero drawn often.
    fn edge_tick() -> impl Strategy<Value = i32> {
        prop_oneof![Just(i32::MIN), Just(i32::MAX), Just(0), any::<i32>()]
    }

    /// Any count of millionths from 1 up, with both ends drawn often.
    fn edge_positive() -> impl Strategy<Value = i64> + Clone {
        prop_oneof![Just(1), Just(MAX_MILLIONTHS), 1..=MAX_MILLIONTHS]
    }

    proptest! {
        #![proptest_config(crate::proptest_config())]

        #[test]
        fn resolving_a_tick_agrees_with_walking_the_map_one_tick_at_a_time(
            offset in -5_000_000_i64..=5_000_000,
            resolution in 1_u16..=48,
            first in (-150_i32..150, tempo(1_000_000_i64..=600_000_000)),
            events in vec((-150_i32..150, kind(1_000_000_i64..=600_000_000, 1_i64..=5_000_000, 1_u32..=40)), 0..12),
            tick in -200_i32..200,
        ) {
            let timing = valid_timing(offset, first, events);
            let resolution = TicksPerBeat::new(resolution).unwrap();
            let resolved = timing.seconds_at(resolution, Tick::new(tick)).unwrap().seconds();
            let walked = walked(&timing, resolution, tick);
            prop_assert!((resolved - walked).abs() <= 1e-9 * walked.abs().max(1.0), "{resolved} != {walked}");
        }

        #[test]
        fn every_tick_of_every_valid_map_resolves_to_a_finite_time(
            offset in -MAX_MILLIONTHS..=MAX_MILLIONTHS,
            resolution in prop_oneof![Just(1_u16), Just(u16::MAX), 1_u16..],
            first in (edge_tick(), tempo(edge_positive())),
            events in vec((edge_tick(), kind(edge_positive(), edge_positive(), prop_oneof![Just(u32::MAX), 1_u32..])), 0..32),
            tick in edge_tick(),
        ) {
            let timing = valid_timing(offset, first, events);
            let resolved = timing.seconds_at(TicksPerBeat::new(resolution).unwrap(), Tick::new(tick));
            prop_assert!(resolved.is_some_and(|time| time.seconds().is_finite()), "{resolved:?}");
        }
    }
}
