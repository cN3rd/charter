//! Syncing beats to the audio the way Tempora does: every BPM change is an anchor pinning a beat
//! to a moment of the audio, and moving one retimes the tempo on either side of it so its
//! neighbours stay where they are in the audio.

use charter_core::{
    Bpm, Extensions, Origin, Tick, Timing, TimingEvent, TimingKind, Valid, ValidDocument,
};

use crate::edit::{Edit, Op};
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
        let moved = (seconds - timing.time_at(tick).seconds()).abs() > 1e-9;
        if anchors.iter().any(|(at, _)| *at == tick) && !moved {
            return Ok(());
        }
        let (before, after) = neighbours(&anchors, tick);
        let kept = kept(timing, before, after);
        let base = (*timing).clone();
        let new = if moved {
            self.pinned(base, tick, seconds, kept)?
        } else {
            let mut new = base;
            let held = held(&anchors, tick).ok_or(Refusal::Unreachable)?;
            set_tempo(&mut new.events, tick, held);
            new
        };
        let (own, _) = self.edited_timing();
        self.retime(own, new)
    }

    /// Moves the anchor on `from` to the beat `to`, as one step, keeping the moment of the audio
    /// it pins: the waveform slides under the grid, as dragging a timing point does in Tempora.
    /// The tempos on either side change so its neighbours keep theirs, or the offset does when
    /// it is the first. Does nothing when there is no anchor on `from` or it is already on `to`.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song, and when `to` is not strictly between its neighbours.
    pub fn move_anchor(&mut self, from: Tick, to: Tick) -> Result<(), Refusal> {
        let timing = self.timing();
        let anchors = anchors(&timing.events);
        let Some(&(_, bpm)) = anchors.iter().find(|(at, _)| *at == from) else {
            return Ok(());
        };
        if from == to {
            return Ok(());
        }
        let (before, after) = neighbours(&anchors, from);
        if before.is_some_and(|before| to <= before) || after.is_some_and(|after| to >= after) {
            return Err(Refusal::Unreachable);
        }
        let seconds = timing.time_at(from).seconds();
        let kept = kept(timing, before, after);
        let mut base = (*timing).clone();
        base.events
            .retain(|event| !(event.tick == from && matches!(event.kind, TimingKind::Bpm(_))));
        set_tempo(&mut base.events, to, bpm);
        let new = self.pinned(base, to, seconds, kept)?;
        let (own, _) = self.edited_timing();
        self.retime(own, new)
    }

    /// `base` with the beat on `tick` falling `seconds` into the audio and the anchors `kept`
    /// names, the ones before and after `tick`, at the moments it gives them.
    fn pinned(
        &self,
        base: Timing,
        tick: Tick,
        seconds: f64,
        kept: Kept,
    ) -> Result<Timing, Refusal> {
        let checked = self.checked(&base)?;
        let timing = timing_of(&checked, self.chart_index());
        let anchors = anchors(&base.events);
        let held = held(&anchors, tick).ok_or(Refusal::Unreachable)?;
        let mut new = base;
        set_tempo(&mut new.events, tick, held);
        if let Some((before, at)) = kept.before {
            let bpm = timing.tempo_for(before, tick, seconds - at);
            set_tempo(&mut new.events, before, bpm.ok_or(Refusal::Unreachable)?);
        }
        if let Some((after, at)) = kept.after {
            let bpm = timing.tempo_for(tick, after, at - seconds);
            set_tempo(&mut new.events, tick, bpm.ok_or(Refusal::Unreachable)?);
        }
        if kept.before.is_none() {
            // With no anchor before it, the beat lands by the offset, which moves every beat
            // alike, so the one after keeps the span its tempo gives it.
            let checked = self.checked(&new)?;
            let landed = timing_of(&checked, self.chart_index())
                .time_at(tick)
                .seconds();
            let offset = f64::from(new.offset_seconds) + seconds - landed;
            new.offset_seconds = micro(offset)
                .and_then(|offset| Origin::try_from(offset).ok())
                .ok_or(Refusal::Unreachable)?;
        }
        Ok(new)
    }

    /// The song with `timing` in place of the one tempo edits go to, checked as an edit would be.
    fn checked(&self, timing: &Timing) -> Result<ValidDocument, Refusal> {
        let (own, old) = self.edited_timing();
        let edit = Edit(vec![Op::Timing {
            own,
            old: Box::new(old.clone()),
            new: Box::new(timing.clone()),
        }]);
        self.edited(self.chart_index(), &edit)
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

/// The anchors on either side of one being synced, with the moments they keep.
#[derive(Clone, Copy, Debug)]
struct Kept {
    before: Option<(Tick, f64)>,
    after: Option<(Tick, f64)>,
}

/// `before` and `after` with the moments `timing` gives them now.
fn kept(timing: Valid<'_, Timing>, before: Option<Tick>, after: Option<Tick>) -> Kept {
    let at = |tick: Tick| (tick, timing.time_at(tick).seconds());
    Kept {
        before: before.map(at),
        after: after.map(at),
    }
}

/// The anchors before and after `tick` among `anchors`, `tick`'s own left out.
fn neighbours(anchors: &[(Tick, Bpm)], tick: Tick) -> (Option<Tick>, Option<Tick>) {
    let before = anchors.iter().rev().find(|(at, _)| *at < tick).map(|a| a.0);
    let after = anchors.iter().find(|(at, _)| *at > tick).map(|a| a.0);
    (before, after)
}

/// The tempo in effect on `tick`: the latest anchor's at or before it, or the first's.
fn held(anchors: &[(Tick, Bpm)], tick: Tick) -> Option<Bpm> {
    anchors
        .iter()
        .rev()
        .find(|(at, _)| *at <= tick)
        .or(anchors.first())
        .map(|(_, bpm)| *bpm)
}

/// The timing the open chart `chart` of `document` resolves through.
fn timing_of(document: &ValidDocument, chart: usize) -> Valid<'_, Timing> {
    document
        .charts()
        .nth(chart)
        .map_or_else(|| document.timing(), |chart| chart.timing())
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
    fn a_first_anchor_after_tick_zero_lands_where_it_is_synced() {
        let mut song = timed(
            &json!([bpm_event(480, 120.0), bpm_event(1920, 120.0)]),
            None,
        );
        let next = song.timing().time_at(at(1920)).seconds();
        song.sync(at(480), 0.5).unwrap();
        assert!((song.timing().time_at(at(480)).seconds() - 0.5).abs() < 1e-6);
        assert!((song.timing().time_at(at(1920)).seconds() - next).abs() < 1e-6);
    }

    #[test]
    fn an_anchor_moved_on_the_grid_keeps_its_moment_and_its_neighbours_theirs() {
        let events = json!([
            bpm_event(0, 120.0),
            bpm_event(1920, 120.0),
            bpm_event(3840, 120.0)
        ]);
        let mut song = timed(&events, None);
        song.move_anchor(at(1920), at(2400)).unwrap();
        let moved = json!([
            bpm_event(0, 150.0),
            bpm_event(2400, 90.0),
            bpm_event(3840, 120.0)
        ]);
        assert_eq!(song_events(&song), moved);
        assert!((song.timing().time_at(at(2400)).seconds() - 1.0).abs() < 1e-9);
        assert!((song.timing().time_at(at(3840)).seconds() - 2.0).abs() < 1e-9);
        song.undo().unwrap();
        assert_eq!(song_events(&song), events);
    }

    #[test]
    fn an_anchor_cannot_move_past_a_neighbour_on_the_grid() {
        let events = json!([
            bpm_event(0, 120.0),
            bpm_event(1920, 120.0),
            bpm_event(3840, 120.0)
        ]);
        let mut song = timed(&events, None);
        assert!(matches!(
            song.move_anchor(at(1920), at(3840)),
            Err(Refusal::Unreachable)
        ));
        assert!(matches!(
            song.move_anchor(at(1920), at(0)),
            Err(Refusal::Unreachable)
        ));
        song.move_anchor(at(1920), at(1920)).unwrap();
        song.move_anchor(at(960), at(1200)).unwrap();
        assert!(!song.can_undo());
    }

    #[test]
    fn the_first_anchor_moved_on_the_grid_moves_the_offset_to_keep_its_moment() {
        let mut song = timed(&json!([bpm_event(0, 120.0), bpm_event(1920, 120.0)]), None);
        song.move_anchor(at(0), at(480)).unwrap();
        let moved = json!([bpm_event(480, 90.0), bpm_event(1920, 120.0)]);
        assert_eq!(song_events(&song), moved);
        assert!(song.timing().time_at(at(480)).seconds().abs() < 1e-6);
        assert!((song.timing().time_at(at(1920)).seconds() - 1.0).abs() < 1e-6);
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
