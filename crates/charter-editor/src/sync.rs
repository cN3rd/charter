//! Syncing beats to the audio the way Tempora does: every BPM change is an anchor pinning a beat
//! to a moment of the audio, and moving one retimes the tempo on either side of it so its
//! neighbours stay where they are in the audio.

use charter_core::{Bpm, Extensions, Origin, Tick, TimingEvent, TimingKind};

use crate::song::{Refusal, Song};

impl Song {
    /// Makes the beat on `tick` fall `seconds` into the audio, as one step. A BPM change on
    /// `tick` becomes its anchor, taking the tempo in effect there if there was none. The tempo
    /// from the anchor before it changes so that anchor stays put, or the offset does when there
    /// is none; the tempo after it changes so the next anchor stays put, and holds when there is
    /// none. Does nothing when the beat is already an anchor there.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song, and when the beat would pass a neighbouring anchor.
    pub fn sync(&mut self, tick: Tick, seconds: f64) -> Result<(), Refusal> {
        let timing = self.timing();
        let anchors = anchors(&timing.events);
        let now = |at: Tick| timing.time_at(at).seconds();
        let moved = (seconds - now(tick)).abs() > 1e-9;
        let anchored = anchors.iter().any(|(at, _)| *at == tick);
        if anchored && !moved {
            return Ok(());
        }
        let held = anchors
            .iter()
            .rev()
            .find(|(at, _)| *at <= tick)
            .or(anchors.first())
            .map(|(_, bpm)| *bpm)
            .ok_or(Refusal::Unreachable)?;
        let before = anchors.iter().rev().find(|(at, _)| *at < tick).map(|a| a.0);
        let after = anchors.iter().find(|(at, _)| *at > tick).map(|a| a.0);
        let mut new = (*timing).clone();
        set_tempo(&mut new.events, tick, held);
        if moved {
            if let Some(before) = before {
                let bpm = timing.tempo_for(before, tick, seconds - now(before));
                set_tempo(&mut new.events, before, bpm.ok_or(Refusal::Unreachable)?);
            } else {
                let offset = f64::from(new.offset_seconds) + seconds - now(tick);
                new.offset_seconds = micro(offset)
                    .and_then(|offset| Origin::try_from(offset).ok())
                    .ok_or(Refusal::Unreachable)?;
            }
            if let Some(after) = after {
                let bpm = timing.tempo_for(tick, after, now(after) - seconds);
                set_tempo(&mut new.events, tick, bpm.ok_or(Refusal::Unreachable)?);
            }
        }
        let (own, _) = self.edited_timing();
        self.retime(own, new)
    }

    /// Removes the anchor on `tick`, as one step, retiming the tempo from the anchor before it
    /// so the one after stays put. Does nothing when there is no anchor there.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song, and for the first anchor, which starts the song's tempo.
    pub fn unsync(&mut self, tick: Tick) -> Result<(), Refusal> {
        let timing = self.timing();
        let anchors = anchors(&timing.events);
        if !anchors.iter().any(|(at, _)| *at == tick) {
            return Ok(());
        }
        let before = anchors.iter().rev().find(|(at, _)| *at < tick).map(|a| a.0);
        let after = anchors.iter().find(|(at, _)| *at > tick).map(|a| a.0);
        let before = before.ok_or(Refusal::FirstTempo)?;
        let mut new = (*timing).clone();
        new.events
            .retain(|event| !(event.tick == tick && matches!(event.kind, TimingKind::Bpm(_))));
        if let Some(after) = after {
            let now = |at: Tick| timing.time_at(at).seconds();
            let bpm = timing.tempo_for(before, after, now(after) - now(before));
            set_tempo(&mut new.events, before, bpm.ok_or(Refusal::Unreachable)?);
        }
        let (own, _) = self.edited_timing();
        self.retime(own, new)
    }
}

/// The BPM changes among `events`, each by its tick.
fn anchors(events: &[TimingEvent]) -> Vec<(Tick, Bpm)> {
    events
        .iter()
        .filter_map(|event| match event.kind {
            TimingKind::Bpm(bpm) => Some((event.tick, bpm)),
            _ => None,
        })
        .collect()
}

/// Gives the BPM change on `tick` among `events` the tempo `bpm`, adding one if there is none.
fn set_tempo(events: &mut Vec<TimingEvent>, tick: Tick, bpm: Bpm) {
    let here = events
        .iter_mut()
        .find(|event| event.tick == tick && matches!(event.kind, TimingKind::Bpm(_)));
    if let Some(change) = here {
        change.kind = TimingKind::Bpm(bpm);
    } else {
        let at = events.partition_point(|event| event.tick <= tick);
        let change = TimingEvent {
            tick,
            kind: TimingKind::Bpm(bpm),
            ext: Extensions::new(),
        };
        events.insert(at, change);
    }
}

/// `seconds` to the nearest microsecond, the finest time the format holds.
fn micro(seconds: f64) -> Option<f64> {
    let rounded = (seconds * 1e6).round() / 1e6;
    rounded.is_finite().then_some(rounded)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::song::tests::{bpm_event, song_events, timed};

    fn at(tick: i32) -> Tick {
        Tick::new(tick)
    }

    fn offset(song: &Song) -> f64 {
        f64::from(song.document().document().timing.offset_seconds)
    }

    #[test]
    fn a_beat_synced_later_slows_the_tempo_before_it_and_hurries_the_one_after() {
        let events = json!([bpm_event(0, 120.0), bpm_event(3840, 120.0)]);
        let mut song = timed(&events, None);
        song.sync(at(1920), 1.25).unwrap();
        let synced = json!([
            bpm_event(0, 96.0),
            bpm_event(1920, 160.0),
            bpm_event(3840, 120.0)
        ]);
        assert_eq!(song_events(&song), synced);
        assert!((song.timing().time_at(at(3840)).seconds() - 2.0).abs() < 1e-9);
        song.undo().unwrap();
        assert_eq!(song_events(&song), events);
    }

    #[test]
    fn the_last_anchor_keeps_its_tempo_after_it() {
        let mut song = timed(&json!([bpm_event(0, 120.0)]), None);
        song.sync(at(1920), 1.25).unwrap();
        let synced = json!([bpm_event(0, 96.0), bpm_event(1920, 120.0)]);
        assert_eq!(song_events(&song), synced);
    }

    #[test]
    fn the_first_anchor_moves_the_offset_and_the_next_stays_put() {
        let mut song = timed(&json!([bpm_event(0, 120.0), bpm_event(3840, 120.0)]), None);
        song.sync(at(0), 0.5).unwrap();
        assert!((offset(&song) - 0.5).abs() < 1e-9);
        let synced = json!([bpm_event(0, 160.0), bpm_event(3840, 120.0)]);
        assert_eq!(song_events(&song), synced);
    }

    #[test]
    fn a_beat_cannot_pass_a_neighbouring_anchor() {
        let events = json!([bpm_event(0, 120.0), bpm_event(3840, 120.0)]);
        let mut song = timed(&events, None);
        assert!(matches!(
            song.sync(at(1920), 2.5),
            Err(Refusal::Unreachable)
        ));
        assert_eq!(song_events(&song), events);
    }

    #[test]
    fn a_beat_synced_where_it_is_becomes_an_anchor_without_retiming() {
        let mut song = timed(&json!([bpm_event(0, 120.0)]), None);
        song.sync(at(1920), 1.0).unwrap();
        let anchored = json!([bpm_event(0, 120.0), bpm_event(1920, 120.0)]);
        assert_eq!(song_events(&song), anchored);
        song.sync(at(1920), 1.0).unwrap();
        song.undo().unwrap();
        assert!(!song.can_undo());
    }

    #[test]
    fn an_anchor_removed_leaves_the_next_where_it_was() {
        let synced = json!([
            bpm_event(0, 96.0),
            bpm_event(1920, 160.0),
            bpm_event(3840, 120.0)
        ]);
        let mut song = timed(&synced, None);
        song.unsync(at(1920)).unwrap();
        let merged = json!([bpm_event(0, 120.0), bpm_event(3840, 120.0)]);
        assert_eq!(song_events(&song), merged);
    }

    #[test]
    fn the_first_anchor_stays_and_a_beat_without_one_has_nothing_to_remove() {
        let events = json!([bpm_event(0, 120.0), bpm_event(1920, 150.0)]);
        let mut song = timed(&events, None);
        assert!(matches!(song.unsync(at(0)), Err(Refusal::FirstTempo)));
        song.unsync(at(960)).unwrap();
        assert!(!song.can_undo());
    }
}
