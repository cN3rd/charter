use crate::document::{Timing, TimingKind};
use crate::unit::{Bpm, ResolvedTime, Tick, TicksPerBeat};

impl Timing {
    /// Audio time of an object on `tick`; see [`Valid::time_at`](crate::Valid::time_at). Events
    /// and anchors must be sorted by tick. `None` means a segment cannot derive a positive finite
    /// rate, which a valid document rules out.
    pub(crate) fn seconds_at(
        &self,
        ticks_per_beat: TicksPerBeat,
        tick: Tick,
    ) -> Option<ResolvedTime> {
        let index = self.anchors.partition_point(|anchor| anchor.tick <= tick);
        let seconds = if let Some(anchor) = self
            .anchors
            .get(index.wrapping_sub(1))
            .filter(|a| a.tick == tick)
        {
            ResolvedTime::from(anchor.seconds).seconds()
        } else if index == 0 {
            let first = self.anchors.first()?;
            let rate = self.pre_anchor_rate(ticks_per_beat)?;
            ResolvedTime::from(first.seconds).seconds()
                - self.elapsed_at_rate(ticks_per_beat, tick, first.tick, rate)?
        } else if let Some(next) = self.anchors.get(index) {
            let previous = self.anchors.get(index - 1)?;
            let rate = self.segment_rate(ticks_per_beat, previous, next)?;
            ResolvedTime::from(previous.seconds).seconds()
                + self.elapsed_at_rate(ticks_per_beat, previous.tick, tick, rate)?
        } else {
            let last = self.anchors.last()?;
            ResolvedTime::from(last.seconds).seconds()
                + self.elapsed_at_rate(ticks_per_beat, last.tick, tick, self.tempo)?
        };
        Some(ResolvedTime::new(seconds))
    }

    /// The derived tempo governing `tick`, as quarter notes per minute.
    pub(crate) fn tempo_at(&self, ticks_per_beat: TicksPerBeat, tick: Tick) -> Option<f64> {
        let index = self.anchors.partition_point(|anchor| anchor.tick <= tick);
        if index == 0 {
            return self.pre_anchor_rate(ticks_per_beat).map(f64::from);
        }
        self.anchors
            .get(index)
            .and_then(|next| {
                self.anchors
                    .get(index - 1)
                    .and_then(|previous| self.segment_rate(ticks_per_beat, previous, next))
            })
            .map_or(Some(f64::from(self.tempo)), |bpm| Some(f64::from(bpm)))
    }

    /// One tempo per anchor. Each interior anchor reports the derived tempo for the following
    /// anchored segment; the last reports the tail tempo.
    pub(crate) fn tempos(&self, ticks_per_beat: TicksPerBeat) -> Option<Vec<(Tick, f64)>> {
        let mut tempos = Vec::with_capacity(self.anchors.len());
        for pair in self.anchors.windows(2) {
            let [previous, next] = pair else {
                unreachable!()
            };
            tempos.push((
                previous.tick,
                f64::from(self.segment_rate(ticks_per_beat, previous, next)?),
            ));
        }
        tempos.push((self.anchors.last()?.tick, f64::from(self.tempo)));
        Some(tempos)
    }

    /// The tempo a segment from `from` to `to` needs for `to` to fall `seconds` after `from`, to
    /// six places. Stops, delays and warps keep their lengths.
    pub(crate) fn tempo_for(
        &self,
        ticks_per_beat: TicksPerBeat,
        from: Tick,
        to: Tick,
        seconds: f64,
    ) -> Option<Bpm> {
        let (timed_ticks, pauses) = self.span_parts(from, to)?;
        let beat_seconds = seconds - pauses;
        let tempo = 60.0 * timed_ticks / (f64::from(ticks_per_beat.get()) * beat_seconds);
        if timed_ticks <= 0.0 || !tempo.is_finite() || tempo <= 0.0 {
            return None;
        }
        Bpm::try_from((tempo * 1e6).round() / 1e6).ok()
    }

    pub(crate) fn segment_rate(
        &self,
        ticks_per_beat: TicksPerBeat,
        previous: &crate::Anchor,
        next: &crate::Anchor,
    ) -> Option<Bpm> {
        self.tempo_for(
            ticks_per_beat,
            previous.tick,
            next.tick,
            ResolvedTime::from(next.seconds).seconds()
                - ResolvedTime::from(previous.seconds).seconds(),
        )
    }

    fn pre_anchor_rate(&self, ticks_per_beat: TicksPerBeat) -> Option<Bpm> {
        match self.anchors.as_slice() {
            [_] => Some(self.tempo),
            [first, second, ..] => self.segment_rate(ticks_per_beat, first, second),
            [] => None,
        }
    }

    /// Seconds between objects on `from` and on `to`, where `from <= to`, at `bpm` for timed
    /// ticks and with the stored pauses.
    fn elapsed_at_rate(
        &self,
        ticks_per_beat: TicksPerBeat,
        from: Tick,
        to: Tick,
        bpm: Bpm,
    ) -> Option<f64> {
        let (timed_ticks, pauses) = self.span_parts(from, to)?;
        Some(pauses + timed_ticks * 60.0 / (f64::from(bpm) * f64::from(ticks_per_beat.get())))
    }

    /// Timed ticks and pause seconds in `[from, to]` using the format's stop, delay and warp
    /// rules. `from` must not be after `to`.
    pub(crate) fn span_parts(&self, from: Tick, to: Tick) -> Option<(f64, f64)> {
        if to < from {
            return None;
        }
        let timed = |cursor: Tick, until: Tick, warp_end: Option<Tick>| {
            let start = warp_end.map_or(cursor, |end| end.max(cursor));
            start
                .ticks_until(until)
                .map_or(0.0, |ticks| f64::from(ticks.get()))
        };
        let mut timed_ticks = 0.0;
        let mut pauses = 0.0;
        let mut cursor = from;
        let mut warp_end = None;
        for event in &self.events {
            let at = event.tick;
            if at > to {
                break;
            }
            if at > from {
                timed_ticks += timed(cursor, at, warp_end);
                cursor = at;
            }
            match event.kind {
                TimingKind::Stop(pause) if at >= from && at < to => pauses += pause.seconds(),
                TimingKind::Delay(pause) if at > from => pauses += pause.seconds(),
                TimingKind::Warp(ticks) => warp_end = Some(at.checked_add(ticks)?),
                TimingKind::Stop(_)
                | TimingKind::Delay(_)
                | TimingKind::TimeSignature(..)
                | TimingKind::Other { .. } => {}
            }
        }
        timed_ticks += timed(cursor, to, warp_end);
        Some((timed_ticks, pauses))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::document::{Anchor, DEFAULT_TICKS_PER_BEAT, TimingEvent};
    use crate::unit::{Origin, Seconds, Ticks};

    fn bpm(value: f64) -> Bpm {
        Bpm::try_from(value).unwrap()
    }

    fn origin(value: f64) -> Origin {
        Origin::try_from(value).unwrap()
    }

    fn anchor(tick: i32, seconds: f64) -> Anchor {
        Anchor {
            tick: Tick::new(tick),
            seconds: origin(seconds),
            ext: BTreeMap::new(),
        }
    }

    fn event(tick: i32, kind: TimingKind) -> TimingEvent {
        TimingEvent {
            tick: Tick::new(tick),
            kind,
            ext: BTreeMap::new(),
        }
    }

    fn timing(anchors: &[(i32, f64)], tempo: f64, events: Vec<TimingEvent>) -> Timing {
        Timing {
            anchors: anchors
                .iter()
                .map(|&(tick, seconds)| anchor(tick, seconds))
                .collect(),
            tempo: bpm(tempo),
            events,
        }
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
    fn anchors_resolve_exactly_and_derive_the_tempo_between_them() {
        let t = timing(&[(0, 1.0), (1920, 3.0)], 240.0, Vec::new());
        assert_time(&t, 0, 1.0);
        assert_time(&t, 960, 2.0);
        assert_time(&t, 1920, 3.0);
        assert!((t.tempo_at(DEFAULT_TICKS_PER_BEAT, Tick::new(960)).unwrap() - 60.0).abs() < 1e-9);
    }

    #[test]
    fn the_tail_uses_the_stored_tempo() {
        let t = timing(&[(0, 0.0), (960, 1.0)], 120.0, Vec::new());
        assert_time(&t, 1920, 1.5);
        assert_eq!(
            t.tempos(DEFAULT_TICKS_PER_BEAT).unwrap(),
            vec![(Tick::new(0), 60.0), (Tick::new(960), 120.0)]
        );
    }

    #[test]
    fn before_the_first_anchor_extrapolates_with_the_first_segment() {
        let t = timing(&[(960, 1.0), (1920, 2.0)], 240.0, Vec::new());
        assert_time(&t, 0, 0.0);
    }

    #[test]
    fn one_anchor_extrapolates_before_it_with_the_tail_tempo() {
        let t = timing(&[(960, 1.0)], 60.0, Vec::new());
        assert_time(&t, 0, 0.0);
    }

    #[test]
    fn stops_and_delays_keep_stepmania_tick_semantics() {
        let events = vec![
            event(960, TimingKind::Stop(Seconds::try_from(2.0).unwrap())),
            event(1920, TimingKind::Delay(Seconds::try_from(3.0).unwrap())),
        ];
        let t = timing(&[(0, 0.0)], 60.0, events);
        assert_time(&t, 960, 1.0);
        assert_time(&t, 1920, 7.0);
        assert_time(&t, 2880, 8.0);
    }

    #[test]
    fn warps_skip_ticks_without_taking_time() {
        let events = vec![event(960, TimingKind::Warp(Ticks::new(960).unwrap()))];
        let t = timing(&[(0, 0.0)], 60.0, events);
        assert_time(&t, 960, 1.0);
        assert_time(&t, 1500, 1.0);
        assert_time(&t, 1920, 1.0);
        assert_time(&t, 2880, 2.0);
    }

    #[test]
    fn tempo_for_uses_timed_ticks_after_pauses() {
        let events = vec![event(
            480,
            TimingKind::Stop(Seconds::try_from(0.5).unwrap()),
        )];
        let t = timing(&[(0, 0.0)], 120.0, events);
        let tempo = t.tempo_for(DEFAULT_TICKS_PER_BEAT, Tick::new(0), Tick::new(1920), 1.5);
        assert_eq!(tempo.map(f64::from), Some(120.0));
    }
}
