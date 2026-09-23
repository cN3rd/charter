use crate::document::{Timing, TimingKind};
use crate::unit::{Bpm, Tick, TicksPerBeat};

impl Timing {
    /// Audio time in seconds of an object on `tick`.
    ///
    /// The time of a tick includes a `delay` on it and excludes a `stop` on it; warps take no
    /// time, and the first `bpm` also rules earlier ticks. Events must be sorted by tick. `None`
    /// means there is no `bpm` event or a warp ends past the last tick; a valid document rules
    /// out both.
    #[must_use]
    pub fn seconds_at(&self, ticks_per_beat: TicksPerBeat, tick: Tick) -> Option<f64> {
        let offset = self.offset_seconds.seconds();
        let zero = Tick::new(0);
        Some(if tick >= zero {
            offset + self.elapsed(ticks_per_beat, zero, tick)?
        } else {
            offset - self.elapsed(ticks_per_beat, tick, zero)?
        })
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
    use super::*;
    use crate::document::{DEFAULT_TICKS_PER_BEAT, TimingEvent, TimingKind};
    use crate::unit::{Origin, Seconds, Ticks};

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
            .unwrap();
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
        let seconds = t.seconds_at(half, Tick::new(480)).unwrap();
        assert!((seconds - 2.0).abs() < 1e-9, "{seconds}");
    }
}
