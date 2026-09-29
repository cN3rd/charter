use crate::document::{Anchor, Timing, TimingEvent, TimingKind};
use crate::unit::{Bpm, ResolvedTime, Tick, TicksPerBeat};

impl Timing {
    /// Audio time of an object on `tick`; see [`Valid::time_at`](crate::Valid::time_at). Events
    /// and anchors must be sorted by tick. `None` means there is no anchor, or a segment cannot
    /// derive a positive finite rate, which a valid document rules out.
    pub(crate) fn seconds_at(
        &self,
        ticks_per_beat: TicksPerBeat,
        tick: Tick,
    ) -> Option<ResolvedTime> {
        let index = self.anchors.partition_point(|anchor| anchor.tick <= tick);
        let seconds = if let Some(anchor) = index
            .checked_sub(1)
            .and_then(|at| self.anchors.get(at))
            .filter(|anchor| anchor.tick == tick)
        {
            ResolvedTime::from(anchor.seconds).seconds()
        } else if index == 0 {
            let first = self.anchors.first()?;
            let per_tick = self.before_first(ticks_per_beat)?;
            ResolvedTime::from(first.seconds).seconds()
                - self.elapsed(tick, first.tick, per_tick)?
        } else if let Some(next) = self.anchors.get(index) {
            let previous = self.anchors.get(index.saturating_sub(1))?;
            let per_tick = self.segment_rate(previous, next)?;
            ResolvedTime::from(previous.seconds).seconds()
                + self.elapsed(previous.tick, tick, per_tick)?
        } else {
            let last = self.anchors.last()?;
            ResolvedTime::from(last.seconds).seconds()
                + self.elapsed(last.tick, tick, tail_rate(self.tempo, ticks_per_beat))?
        };
        Some(ResolvedTime::new(seconds))
    }

    /// The tempo governing `tick`, in quarter notes per minute: derived between anchors, the
    /// stored tempo after the last, and the first segment's before the first.
    pub(crate) fn tempo_at(&self, ticks_per_beat: TicksPerBeat, tick: Tick) -> Option<f64> {
        let index = self.anchors.partition_point(|anchor| anchor.tick <= tick);
        let per_tick = if index == 0 {
            self.before_first(ticks_per_beat)?
        } else {
            match (
                self.anchors.get(index.saturating_sub(1)),
                self.anchors.get(index),
            ) {
                (Some(previous), Some(next)) => self.segment_rate(previous, next)?,
                _ => tail_rate(self.tempo, ticks_per_beat),
            }
        };
        Some(bpm_of(per_tick, ticks_per_beat))
    }

    /// The tempo at `tick` as it is shown and typed: an anchored segment's to the fewest decimal
    /// places that land its next anchor on the same tick, and the tail's as stored.
    pub(crate) fn bpm_at(&self, ticks_per_beat: TicksPerBeat, tick: Tick) -> Option<f64> {
        let exact = self.tempo_at(ticks_per_beat, tick)?;
        let index = self
            .anchors
            .partition_point(|anchor| anchor.tick <= tick)
            .max(1);
        Some(
            match (
                self.anchors.get(index.saturating_sub(1)),
                self.anchors.get(index),
            ) {
                (Some(previous), Some(next)) => simplest(exact, previous.tick, next.tick),
                _ => exact,
            },
        )
    }

    /// One tempo per anchor, in quarter notes per minute: each the derived tempo of the segment
    /// it starts, and the last the stored tempo after it.
    pub(crate) fn tempos(&self, ticks_per_beat: TicksPerBeat) -> Option<Vec<(Tick, f64)>> {
        let mut tempos = Vec::with_capacity(self.anchors.len());
        for (start, _, rate) in self.segment_rates() {
            tempos.push((start, bpm_of(rate?, ticks_per_beat)));
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
        let per_tick = Self::rate_for(from, to, seconds, &self.events)?;
        let tempo = bpm_of(per_tick, ticks_per_beat);
        Bpm::try_from((tempo * 1e6).round() / 1e6).ok()
    }

    /// Seconds a timed tick takes between the anchors `previous` and `next`, exactly: their
    /// time apart less the pauses between, over the ticks between that warps leave timed.
    pub(crate) fn segment_rate(&self, previous: &Anchor, next: &Anchor) -> Option<f64> {
        Self::segment_rate_in(previous, next, &self.events)
    }

    /// Derive a segment's rate from only the events that can affect it.
    pub(crate) fn segment_rate_in(
        previous: &Anchor,
        next: &Anchor,
        events: &[TimingEvent],
    ) -> Option<f64> {
        let apart = ResolvedTime::from(next.seconds).seconds()
            - ResolvedTime::from(previous.seconds).seconds();
        Self::rate_for(previous.tick, next.tick, apart, events)
    }

    /// Each anchored segment's rate in a forward sweep of sorted events. Anchors must be outside
    /// warps.
    #[expect(
        clippy::unreachable,
        reason = "windows(2) always yields exactly two anchors"
    )]
    pub(crate) fn segment_rates(&self) -> impl Iterator<Item = (Tick, Tick, Option<f64>)> + '_ {
        let mut first_event = 0;
        self.anchors.windows(2).map(move |pair| {
            let [previous, next] = pair else {
                unreachable!()
            };
            while self
                .events
                .get(first_event)
                .is_some_and(|event| event.tick < previous.tick)
            {
                first_event = first_event.saturating_add(1);
            }
            let mut after = first_event;
            while self
                .events
                .get(after)
                .is_some_and(|event| event.tick <= next.tick)
            {
                after = after.saturating_add(1);
            }
            (
                previous.tick,
                next.tick,
                self.events
                    .get(first_event..after)
                    .and_then(|events| Self::segment_rate_in(previous, next, events)),
            )
        })
    }

    /// Seconds a timed tick takes if the ticks from `from` to `to` last `seconds`, pauses
    /// included; none unless that is positive and finite.
    fn rate_for(from: Tick, to: Tick, seconds: f64, events: &[TimingEvent]) -> Option<f64> {
        let (timed_ticks, pauses) = Self::span_parts_in(events, from, to)?;
        let per_tick = (seconds - pauses) / timed_ticks;
        (timed_ticks > 0.0 && per_tick.is_finite() && per_tick > 0.0).then_some(per_tick)
    }

    /// Seconds a timed tick takes before the first anchor: the first segment's, or the stored
    /// tempo's when there is one anchor.
    fn before_first(&self, ticks_per_beat: TicksPerBeat) -> Option<f64> {
        match self.anchors.as_slice() {
            [_] => Some(tail_rate(self.tempo, ticks_per_beat)),
            [first, second, ..] => self.segment_rate(first, second),
            [] => None,
        }
    }

    /// Seconds between objects on `from` and on `to`, where `from <= to`, at `per_tick` seconds
    /// a timed tick, with the pauses between.
    fn elapsed(&self, from: Tick, to: Tick, per_tick: f64) -> Option<f64> {
        let (timed_ticks, pauses) = self.span_parts(from, to)?;
        Some(pauses + timed_ticks * per_tick)
    }

    /// The ticks from `from` to `to` that warps leave timed, and the seconds of the pauses
    /// between them, by the format's rules: a stop on a tick counts from it, a delay up to it.
    /// None when `to` is before `from` or a warp ends past the last tick.
    pub(crate) fn span_parts(&self, from: Tick, to: Tick) -> Option<(f64, f64)> {
        Self::span_parts_in(&self.events, from, to)
    }

    /// Timed ticks and pause seconds between `from` and `to` under `events`.
    fn span_parts_in(events: &[TimingEvent], from: Tick, to: Tick) -> Option<(f64, f64)> {
        if to < from {
            return None;
        }
        // Ticks before `warp_end` take no time. A warp starts at its own tick, which the cursor
        // has always reached, so only the part of a span after `warp_end` counts.
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
        for event in events {
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
                // Meter shapes the grid, not time; plugin events are never interpreted.
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

/// Seconds a tick takes at `tempo`.
fn tail_rate(tempo: Bpm, ticks_per_beat: TicksPerBeat) -> f64 {
    60.0 / (f64::from(tempo) * f64::from(ticks_per_beat.get()))
}

/// The tempo, in quarter notes per minute, at which a tick takes `per_tick` seconds.
fn bpm_of(per_tick: f64, ticks_per_beat: TicksPerBeat) -> f64 {
    60.0 / (per_tick * f64::from(ticks_per_beat.get()))
}

/// `exact`, the tempo of a segment from `from` to `to`, to the fewest decimal places that still
/// land `to` on the same tick. An anchor sits on a whole tick, so the exact rate between two is a
/// typed tempo off by up to half a tick; this is the tempo that was typed.
fn simplest(exact: f64, from: Tick, to: Tick) -> f64 {
    let span = span_as_f64(i64::from(to).saturating_sub(i64::from(from)));
    (0..6)
        .map(|places| {
            let scale = 10_f64.powi(places);
            (exact * scale).round() / scale
        })
        .find(|tempo| (span * tempo / exact - span).abs() < 0.5)
        .unwrap_or(exact)
}

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "two i32 tick positions differ by less than 2^53"
)]
/// Tick distance as a float; the difference between two chart ticks stays exact.
const fn span_as_f64(ticks: i64) -> f64 {
    ticks as f64
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashSet};
    use std::mem::discriminant;

    use proptest::collection::vec;
    use proptest::prelude::*;

    use super::*;
    use crate::document::TimingEvent;
    use crate::unit::TicksPerBeat;
    use crate::unit::{Origin, Seconds, Ticks};

    fn bpm(value: f64) -> Bpm {
        Bpm::try_from(value).unwrap()
    }

    fn anchor(tick: i32, seconds: f64) -> Anchor {
        Anchor {
            tick: Tick::new(tick),
            seconds: Origin::try_from(seconds).unwrap(),
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

    fn stop(seconds: f64) -> TimingKind {
        TimingKind::Stop(Seconds::try_from(seconds).unwrap())
    }

    fn delay(seconds: f64) -> TimingKind {
        TimingKind::Delay(Seconds::try_from(seconds).unwrap())
    }

    fn warp(ticks: u32) -> TimingKind {
        TimingKind::Warp(Ticks::new(ticks).unwrap())
    }

    fn time(timing: &Timing, tick: i32) -> f64 {
        timing
            .seconds_at(TicksPerBeat::default(), Tick::new(tick))
            .unwrap()
            .seconds()
    }

    #[track_caller]
    fn assert_time(timing: &Timing, tick: i32, expected: f64) {
        let actual = time(timing, tick);
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
        let tempo = t.tempo_at(TicksPerBeat::default(), Tick::new(960)).unwrap();
        assert!((tempo - 60.0).abs() < 1e-9);
    }

    #[test]
    fn a_tempo_that_does_not_come_out_on_six_places_is_kept_exact() {
        // Seven beats in two seconds is 210 BPM; three in 1.4 s is 128.571428…, which a BPM of six
        // places would round and land the next anchor a little off.
        let t = timing(&[(0, 0.0), (2880, 1.4), (5760, 2.8)], 128.0, Vec::new());
        let tempo = t.tempo_at(TicksPerBeat::default(), Tick::new(1)).unwrap();
        assert!((tempo - 180.0 / 1.4).abs() < 1e-9, "{tempo}");
        assert!(time(&t, 2879) < 1.4);
        assert_time(&t, 2880, 1.4);
    }

    #[test]
    fn the_tail_uses_the_stored_tempo() {
        let t = timing(&[(0, 0.0), (960, 1.0)], 120.0, Vec::new());
        assert_time(&t, 1920, 1.5);
        let tempos = t.tempos(TicksPerBeat::default()).unwrap();
        assert_eq!(tempos.len(), 2);
        assert!((tempos[0].1 - 60.0).abs() < 1e-9 && (tempos[1].1 - 120.0).abs() < 1e-9);
    }

    #[test]
    fn many_anchored_tempos_do_not_rescan_earlier_events() {
        let name: crate::Key = "test.pulse".parse().unwrap();
        let timing = Timing {
            anchors: (0..100_000)
                .map(|tick| {
                    anchor(
                        tick,
                        crate::number::from_millionths(i64::from(tick) * 1_000),
                    )
                })
                .collect(),
            tempo: bpm(120.0),
            events: (0..100_000)
                .map(|tick| {
                    event(
                        tick,
                        TimingKind::Other {
                            name: name.clone(),
                            value: serde_json::Value::Null,
                        },
                    )
                })
                .collect(),
        };
        let tempos = timing.tempos(TicksPerBeat::default()).unwrap();
        assert_eq!(tempos.len(), 100_000);
        assert_eq!(tempos[0], (Tick::new(0), 62.5));
        assert_eq!(tempos[99_999], (Tick::new(99_999), 120.0));
    }

    #[test]
    fn before_the_first_anchor_extrapolates_with_the_first_segment() {
        let t = timing(&[(960, 1.0), (1920, 2.0)], 30.0, Vec::new());
        assert_time(&t, 0, 0.0);
        assert_time(&t, -960, -1.0);
    }

    #[test]
    fn one_anchor_extrapolates_before_it_with_the_tail_tempo() {
        let t = timing(&[(960, 1.0)], 60.0, Vec::new());
        assert_time(&t, 0, 0.0);
    }

    #[test]
    fn a_stop_pauses_after_the_objects_on_its_tick() {
        let t = timing(&[(0, 0.0)], 60.0, vec![event(960, stop(2.0))]);
        assert_time(&t, 960, 1.0);
        assert_time(&t, 961, 3.0 + 1.0 / 960.0);
        assert_time(&t, 1920, 4.0);
    }

    #[test]
    fn a_delay_pauses_before_the_objects_on_its_tick() {
        let t = timing(&[(0, 0.0)], 60.0, vec![event(960, delay(3.0))]);
        assert_time(&t, 959, 959.0 / 960.0);
        assert_time(&t, 960, 4.0);
        assert_time(&t, 1920, 5.0);
    }

    #[test]
    fn an_anchor_on_a_delay_is_the_time_after_the_delay() {
        let t = timing(&[(0, 0.0), (960, 4.0)], 60.0, vec![event(960, delay(3.0))]);
        let tempo = t.tempo_at(TicksPerBeat::default(), Tick::new(480)).unwrap();
        assert!(
            (tempo - 60.0).abs() < 1e-9,
            "the delay takes three of the four seconds"
        );
        assert_time(&t, 959, 959.0 / 960.0);
    }

    #[test]
    fn a_stop_inside_a_segment_shortens_the_time_its_beats_get() {
        let t = timing(&[(0, 0.0), (1920, 3.0)], 60.0, vec![event(960, stop(1.0))]);
        let tempo = t.tempo_at(TicksPerBeat::default(), Tick::new(1)).unwrap();
        assert!((tempo - 60.0).abs() < 1e-9);
        assert_time(&t, 960, 1.0);
        assert_time(&t, 961, 2.0 + 1.0 / 960.0);
    }

    #[test]
    fn a_warp_skips_its_ticks_without_taking_time() {
        let t = timing(&[(0, 0.0)], 60.0, vec![event(960, warp(960))]);
        assert_time(&t, 960, 1.0);
        assert_time(&t, 1500, 1.0);
        assert_time(&t, 1920, 1.0);
        assert_time(&t, 2880, 2.0);
    }

    #[test]
    fn pauses_inside_a_warp_still_pause() {
        let events = vec![event(960, warp(960)), event(1200, stop(0.5))];
        let t = timing(&[(0, 0.0)], 60.0, events);
        assert_time(&t, 1200, 1.0);
        assert_time(&t, 1201, 1.5);
        assert_time(&t, 1920, 1.5);
    }

    #[test]
    fn negative_ticks_resolve_backward_through_pauses() {
        let t = timing(&[(0, 0.0)], 60.0, vec![event(-960, stop(1.0))]);
        assert_time(&t, -960, -2.0);
        assert_time(&t, -1920, -3.0);
    }

    #[test]
    fn a_segment_reads_as_the_simplest_tempo_that_lands_its_next_anchor_on_the_same_tick() {
        // 175.5 BPM for 26.1 seconds is 73,288.8 ticks, so the anchor sits on 73,289 and the
        // exact rate between the two is 175.500479 BPM.
        let typed = timing(&[(0, 0.0), (73_289, 26.1)], 120.0, Vec::new());
        let at = |tick| {
            typed
                .bpm_at(TicksPerBeat::default(), Tick::new(tick))
                .unwrap()
        };
        assert!((at(0) - 175.5).abs() < 1e-12, "{}", at(0));
        assert!((at(-960) - 175.5).abs() < 1e-12, "{}", at(-960));
        assert!((at(80_000) - 120.0).abs() < 1e-12, "{}", at(80_000));
        assert!((time(&typed, 73_289) - 26.1).abs() < 1e-9);
        // A rate that no shorter decimal lands on the same tick is read exactly.
        let odd = timing(&[(0, 0.0), (73_289, 26.123_457)], 120.0, Vec::new());
        let exact = odd.tempo_at(TicksPerBeat::default(), Tick::new(0)).unwrap();
        let read = odd.bpm_at(TicksPerBeat::default(), Tick::new(0)).unwrap();
        assert!(
            (read - exact).abs() * 73_289.0 / exact < 0.5,
            "{read} {exact}"
        );
    }

    #[test]
    fn a_timing_without_anchors_has_no_times() {
        let empty = timing(&[], 60.0, Vec::new());
        assert_eq!(
            empty.seconds_at(TicksPerBeat::default(), Tick::new(0)),
            None
        );
        assert_eq!(empty.tempo_at(TicksPerBeat::default(), Tick::new(0)), None);
    }

    #[test]
    fn a_segment_its_pauses_outlast_has_no_rate() {
        let t = timing(&[(0, 0.0), (1920, 1.0)], 60.0, vec![event(960, stop(1.0))]);
        assert_eq!(t.seconds_at(TicksPerBeat::default(), Tick::new(480)), None);
        let warped = timing(&[(0, 0.0), (960, 1.0)], 60.0, vec![event(0, warp(960))]);
        assert_eq!(
            warped.seconds_at(TicksPerBeat::default(), Tick::new(480)),
            None
        );
    }

    #[test]
    fn the_resolution_sets_how_many_ticks_make_a_beat() {
        let t = timing(&[(0, 0.0)], 60.0, Vec::new());
        let half = TicksPerBeat::new(480).unwrap();
        let seconds = t.seconds_at(half, Tick::new(480)).unwrap().seconds();
        assert!((seconds - 1.0).abs() < 1e-9, "{seconds}");
    }

    #[test]
    fn a_span_takes_the_tempo_that_makes_it_last_the_seconds_asked_to_six_places() {
        let t = timing(&[(0, 0.0)], 60.0, vec![event(480, stop(0.5))]);
        let tempo = |to: i32, seconds: f64| {
            t.tempo_for(
                TicksPerBeat::default(),
                Tick::new(0),
                Tick::new(to),
                seconds,
            )
            .map(f64::from)
        };
        assert_eq!(tempo(1920, 1.5), Some(120.0));
        assert_eq!(tempo(1920, 0.5), None);
        assert_eq!(tempo(960, 1.2), Some(85.714_286));
        assert_eq!(
            t.tempo_for(TicksPerBeat::default(), Tick::new(960), Tick::new(0), 1.0),
            None
        );
    }

    /// A valid timing: `events` kept one of a kind a tick with warps that do not overlap, then
    /// anchors on unwarped ticks, each after the last by its segment's pauses and `slack` more.
    fn valid_timing(
        mut events: Vec<(i32, TimingKind)>,
        mut anchors: Vec<(i32, f64)>,
        (first, tempo): (f64, Bpm),
    ) -> Timing {
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
        let mut timing = Timing {
            anchors: Vec::new(),
            tempo,
            events: kept,
        };
        let warped = |map_timing: &Timing, tick: Tick| {
            map_timing.events.iter().any(|event| match event.kind {
                TimingKind::Warp(span) => {
                    event.tick <= tick && event.tick.checked_add(span).is_none_or(|end| tick < end)
                }
                TimingKind::Stop(_)
                | TimingKind::Delay(_)
                | TimingKind::TimeSignature(_)
                | TimingKind::Other { .. } => false,
            })
        };
        anchors.sort_by_key(|(tick, _)| *tick);
        anchors.dedup_by_key(|(tick, _)| *tick);
        let mut seconds = first;
        for (tick, slack) in anchors {
            let tick = Tick::new(tick);
            if warped(&timing, tick) {
                continue;
            }
            if let Some(last) = timing.anchors.last() {
                let Some((timed, pauses)) = timing.span_parts(last.tick, tick) else {
                    continue;
                };
                if timed <= 0.0 {
                    continue;
                }
                seconds += pauses + slack;
            }
            let Ok(at) = Origin::try_from((seconds * 1e6).round() / 1e6) else {
                continue;
            };
            timing.anchors.push(Anchor { tick, seconds: at });
            seconds = ResolvedTime::from(at).seconds();
        }
        timing
    }

    /// The time of `tick` found by walking one tick at a time from the anchor governing it, at
    /// the rate its segment derives: the format chapter's pause rules restated without spans.
    fn walked(timing: &Timing, tick: i32) -> f64 {
        let at = |k: i32| Tick::new(k);
        let warped = |k: i32| {
            timing.events.iter().any(|event| match event.kind {
                TimingKind::Warp(span) => {
                    event.tick <= at(k)
                        && event.tick.checked_add(span).is_none_or(|end| at(k) < end)
                }
                TimingKind::Stop(_)
                | TimingKind::Delay(_)
                | TimingKind::TimeSignature(_)
                | TimingKind::Other { .. } => false,
            })
        };
        let pauses = |k: i32, stops: bool| -> f64 {
            timing
                .events
                .iter()
                .filter(|event| event.tick == at(k))
                .map(|event| match event.kind {
                    TimingKind::Stop(pause) if stops => pause.seconds(),
                    TimingKind::Delay(pause) if !stops => pause.seconds(),
                    TimingKind::Stop(_)
                    | TimingKind::Delay(_)
                    | TimingKind::Warp(_)
                    | TimingKind::TimeSignature(_)
                    | TimingKind::Other { .. } => 0.0,
                })
                .sum()
        };
        let index = timing
            .anchors
            .partition_point(|anchor| anchor.tick <= at(tick));
        let from = timing
            .anchors
            .get(index.max(1).saturating_sub(1))
            .expect("generated timing has an anchor");
        let per_tick = timing
            .tempo_at(TicksPerBeat::default(), at(tick))
            .map(|tempo| 60.0 / (tempo * f64::from(TicksPerBeat::default().get())))
            .unwrap();
        let start = i64::from(from.tick);
        let (low, high) = if i64::from(tick) >= start {
            (start, i64::from(tick))
        } else {
            (i64::from(tick), start)
        };
        let mut elapsed = 0.0;
        for k in low..high {
            let k = i32::try_from(k).unwrap();
            // A stop on `k` holds after its objects; a delay on `k + 1` holds before its own.
            elapsed += pauses(k, true);
            if !warped(k) {
                elapsed += per_tick;
            }
            elapsed += pauses(k.saturating_add(1), false);
        }
        let base = ResolvedTime::from(from.seconds).seconds();
        if i64::from(tick) >= start {
            base + elapsed
        } else {
            base - elapsed
        }
    }

    fn kind() -> impl Strategy<Value = TimingKind> {
        let pause =
            (1_u32..=2_000_000).prop_map(|m| Seconds::try_from(f64::from(m) / 1e6).unwrap());
        prop_oneof![
            pause.clone().prop_map(TimingKind::Stop),
            pause.prop_map(TimingKind::Delay),
            (1_u32..=2000).prop_map(|w| TimingKind::Warp(Ticks::new(w).unwrap())),
        ]
    }

    fn map() -> impl Strategy<Value = Timing> {
        (
            vec((-8000_i32..8000, kind()), 0..8),
            vec((-8000_i32..8000, 0.01_f64..5.0), 1..6),
            (-5.0_f64..5.0, 20.0_f64..400.0),
        )
            .prop_map(|(events, anchors, (first, tempo))| {
                valid_timing(events, anchors, (first, bpm((tempo * 1e6).round() / 1e6)))
            })
            .prop_filter("has an anchor", |timing| !timing.anchors.is_empty())
    }

    proptest! {
        #![proptest_config(crate::proptest_config())]

        #[test]
        fn every_anchor_of_a_valid_map_resolves_to_its_own_time(timing in map()) {
            for anchor in &timing.anchors {
                let at = timing.seconds_at(TicksPerBeat::default(), anchor.tick).unwrap();
                prop_assert!((at.seconds() - ResolvedTime::from(anchor.seconds).seconds()).abs() < 1e-9);
            }
        }

        #[test]
        fn time_never_runs_backwards_even_across_an_anchor(timing in map(), tick in -9000_i32..9000) {
            let now = timing.seconds_at(TicksPerBeat::default(), Tick::new(tick)).unwrap().seconds();
            let next = timing.seconds_at(TicksPerBeat::default(), Tick::new(tick.saturating_add(1))).unwrap().seconds();
            prop_assert!(now.is_finite() && next.is_finite());
            prop_assert!(next >= now - 1e-9, "{tick}: {now} then {next}");
            for anchor in &timing.anchors {
                let before = i32::try_from(i64::from(anchor.tick).saturating_sub(1)).unwrap();
                let just = timing.seconds_at(TicksPerBeat::default(), Tick::new(before)).unwrap().seconds();
                prop_assert!(just <= ResolvedTime::from(anchor.seconds).seconds() + 1e-9);
            }
        }

        #[test]
        fn resolving_a_tick_agrees_with_walking_the_map_one_tick_at_a_time(
            timing in map(),
            tick in -9000_i32..9000,
        ) {
            let resolved = timing.seconds_at(TicksPerBeat::default(), Tick::new(tick)).unwrap().seconds();
            let walked = walked(&timing, tick);
            prop_assert!((resolved - walked).abs() < 1e-6, "{tick}: {resolved} vs {walked}");
        }
    }
}
