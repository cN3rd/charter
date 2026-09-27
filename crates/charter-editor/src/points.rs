//! Timing points: every tick of a timing that holds events or an anchor, a tempo anchor and a meter at one tick
//! making one point, as osu! lists them. The song panel edits the point in effect at the cursor.

use charter_core::{Bpm, Extensions, Tick, TimeSignature, TimingEvent, TimingKind};

use crate::song::{Refusal, Song};

impl Song {
    /// The tick of the timing point in effect at the cursor: the latest at or before it, or the
    /// first when the cursor is before every one.
    #[must_use]
    pub fn timing_point(&self) -> Option<Tick> {
        let (_, timing) = self.edited_timing();
        let mut ticks: Vec<_> = timing
            .anchors
            .iter()
            .map(|anchor| anchor.tick)
            .chain(timing.events.iter().map(|event| event.tick))
            .collect();
        ticks.sort_unstable();
        ticks.dedup();
        ticks
            .iter()
            .copied()
            .rfind(|tick| *tick <= self.cursor())
            .or_else(|| ticks.first().copied())
    }

    /// Gives the timing point on `tick` the tempo `bpm`, adding a BPM change to it if it has
    /// none, as one step. Does nothing when it already has that tempo.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song, or when the game refuses the tempo.
    pub fn set_bpm_at(&mut self, tick: Tick, bpm: Bpm) -> Result<(), Refusal> {
        if self.timing().bpm_at(tick) == Some(bpm) {
            return Ok(());
        }
        self.set_bpm_change_at(tick, bpm)
    }

    /// Gives the timing point on `tick` the meter `meter`, starting the measures there, adding a
    /// time signature to it if it has none, as one step. Does nothing when it already has it.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song, when a measure of `meter` is not a whole number of ticks, or
    /// when the game refuses the meter.
    pub fn set_meter_at(&mut self, tick: Tick, meter: TimeSignature) -> Result<(), Refusal> {
        self.set_at(tick, TimingKind::TimeSignature(meter))
    }

    /// Removes every event of the timing point on `tick`, as one step.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song, and for the point holding the first BPM change, which starts
    /// the tempo.
    pub fn remove_timing_at(&mut self, tick: Tick) -> Result<(), Refusal> {
        if self.read_only().is_some() {
            return Err(Refusal::ReadOnly);
        }
        let (own, timing) = self.edited_timing();
        if timing.anchors.len() == 1
            && timing
                .anchors
                .first()
                .is_some_and(|anchor| anchor.tick == tick)
        {
            return Err(Refusal::OnlyTempo);
        }
        if !timing.events.iter().any(|event| event.tick == tick)
            && !timing.anchors.iter().any(|anchor| anchor.tick == tick)
        {
            return Ok(());
        }
        let mut new = timing.clone();
        if new.anchors.last().is_some_and(|anchor| anchor.tick == tick)
            && let Some(previous) = new.anchors.iter().rev().nth(1)
            && let Ok(tempo) = self.timing().bpm_at(previous.tick).ok_or(())
        {
            new.tempo = tempo;
        }
        new.events.retain(|event| event.tick != tick);
        new.anchors.retain(|anchor| anchor.tick != tick);
        self.retime(own, new)
    }

    /// Puts `kind` on `tick` in place of the event of its kind there, or beside the others.
    fn set_at(&mut self, tick: Tick, kind: TimingKind) -> Result<(), Refusal> {
        let (own, timing) = self.edited_timing();
        let same = |event: &TimingEvent| {
            event.tick == tick
                && std::mem::discriminant(&event.kind) == std::mem::discriminant(&kind)
        };
        if timing
            .events
            .iter()
            .any(|event| same(event) && event.kind == kind)
        {
            return Ok(());
        }
        let mut new = timing.clone();
        if let Some(event) = new.events.iter_mut().find(|event| same(event)) {
            event.kind = kind;
        } else {
            let at = new.events.partition_point(|event| event.tick <= tick);
            let event = TimingEvent {
                tick,
                kind,
                ext: Extensions::new(),
            };
            new.events.insert(at, event);
        }
        self.retime(own, new)
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU16;

    use serde_json::json;

    use super::*;
    use crate::Step;
    use crate::song::tests::{bpm_event, song_events, timed};

    fn meter(numerator: u16, denominator: u16) -> TimeSignature {
        TimeSignature {
            numerator: NonZeroU16::new(numerator).unwrap(),
            denominator: NonZeroU16::new(denominator).unwrap(),
        }
    }

    fn meter_event(tick: i32, numerator: u16, denominator: u16) -> serde_json::Value {
        json!({ "tick": tick, "kind": { "time_signature": [numerator, denominator] } })
    }

    fn tempo(bpm: f64) -> Bpm {
        Bpm::try_from(bpm).unwrap()
    }

    #[test]
    fn the_point_in_effect_is_the_latest_at_or_before_the_cursor() {
        let mut song = timed(&json!([bpm_event(0, 120.0), meter_event(480, 3, 4)]), None);
        assert_eq!(song.timing_point(), Some(Tick::new(0)));
        song.step(Step::Later);
        assert_eq!(song.timing_point(), Some(Tick::new(0)));
        song.step(Step::Later);
        assert_eq!(song.timing_point(), Some(Tick::new(480)));
    }

    #[test]
    fn a_point_takes_a_tempo_or_a_meter_beside_what_it_has() {
        let mut song = timed(&json!([bpm_event(0, 120.0), meter_event(480, 3, 4)]), None);
        song.set_bpm_at(Tick::new(480), tempo(150.0)).unwrap();
        song.set_meter_at(Tick::new(0), meter(7, 8)).unwrap();
        let expected = json!([
            bpm_event(0, 120.0),
            meter_event(0, 7, 8),
            meter_event(480, 3, 4),
            bpm_event(480, 150.0),
        ]);
        assert_eq!(song_events(&song), expected);
        song.set_meter_at(Tick::new(480), meter(5, 4)).unwrap();
        assert_eq!(song_events(&song)[2], meter_event(480, 5, 4));
        song.set_meter_at(Tick::new(480), meter(5, 4)).unwrap();
        for _ in 0..3 {
            song.undo().unwrap();
        }
        assert!(!song.can_undo());
    }

    #[test]
    fn setting_a_point_bpm_undoes_the_new_anchor_and_tempo_together() {
        let events = json!([bpm_event(0, 120.0), meter_event(480, 3, 4)]);
        let mut song = timed(&events, None);
        song.set_bpm_at(Tick::new(480), tempo(150.0)).unwrap();
        song.undo().unwrap();
        assert_eq!(song_events(&song), events);
        assert!(!song.can_undo());
    }

    #[test]
    fn a_refused_point_bpm_keeps_the_timing_history_and_cursor() {
        let events = json!([
            bpm_event(0, 120.0),
            bpm_event(1920, 120.0),
            bpm_event(3840, 120.0)
        ]);
        let mut song = timed(&events, None);
        // A slower tempo fits fewer ticks before the point at 1920, which pulls the tap at 2160
        // back onto the one at 1440.
        for tick in [1440, 2160] {
            song.toggle(
                Tick::new(tick),
                charter_core::LaneIndex::new(0),
                charter_core::Kind::Tap,
            )
            .unwrap();
        }
        let history = song.can_undo();
        song.jump_to(Tick::new(240));
        let modified = song.is_modified();
        let edits = song.revision();
        assert!(song.set_bpm_at(Tick::new(960), tempo(30.0)).is_err());
        assert_eq!(song_events(&song), events);
        assert_eq!(song.cursor(), Tick::new(240));
        assert_eq!(
            (song.can_undo(), song.is_modified(), song.revision()),
            (history, modified, edits)
        );
    }

    #[test]
    fn a_read_only_point_bpm_refusal_does_not_move_the_cursor() {
        let document = timed(&json!([bpm_event(0, 120.0)]), None)
            .document()
            .clone();
        let mut song = Song::new(document, &[]);
        assert!(matches!(
            song.set_bpm_at(Tick::new(960), tempo(150.0)),
            Err(Refusal::ReadOnly)
        ));
        assert_eq!(song.cursor(), Tick::new(0));
    }

    #[test]
    fn setting_a_point_to_its_current_bpm_adds_no_anchor() {
        let events = json!([bpm_event(0, 120.0)]);
        let mut song = timed(&events, None);
        song.set_bpm_at(Tick::new(960), tempo(120.0)).unwrap();
        assert_eq!(song_events(&song), events);
        assert!(!song.can_undo());
    }

    #[test]
    fn a_meter_whose_measure_is_not_whole_ticks_is_refused() {
        let mut song = timed(&json!([bpm_event(0, 120.0)]), None);
        assert!(song.set_meter_at(Tick::new(0), meter(1, 7)).is_err());
        assert_eq!(song_events(&song), json!([bpm_event(0, 120.0)]));
    }

    #[test]
    fn a_point_is_removed_whole_but_not_the_one_starting_the_tempo() {
        let events = json!([
            bpm_event(0, 120.0),
            meter_event(480, 3, 4),
            bpm_event(480, 150.0)
        ]);
        let mut song = timed(&events, None);
        song.remove_timing_at(Tick::new(480)).unwrap();
        assert_eq!(song_events(&song), json!([bpm_event(0, 120.0)]));
        assert!(matches!(
            song.remove_timing_at(Tick::new(0)),
            Err(Refusal::OnlyTempo)
        ));
        song.remove_timing_at(Tick::new(960)).unwrap();
        song.undo().unwrap();
        assert_eq!(song_events(&song), events);
    }
}
