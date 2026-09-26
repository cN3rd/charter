//! Syncing beats to the audio with anchors, as a tempo mapper does: every anchor pins its
//! beat to a moment of the audio, the tempo between two anchors follows from where they are,
//! and only the tempo after the last one is set. Every edit here is a new set of pins, from
//! which the tempo is rebuilt, so no edit can leave the pins out of order.

use charter_core::{Anchor, Bpm, Extensions, Origin, Tick, Timing, ValidDocument};

use crate::edit::{Edit, Op};
use crate::song::{Refusal, Song};

/// An anchor: the beat on `tick` pinned `seconds` into the audio.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pin {
    pub tick: Tick,
    pub seconds: f64,
}

/// How far apart two moments have to be to count as two: a microsecond, the finest time the
/// format holds.
const APART: f64 = 1e-6;

impl Song {
    /// The anchors of the timing tempo edits go to, in order, each where it falls now.
    #[must_use]
    pub fn anchors(&self) -> Vec<Pin> {
        self.timing()
            .anchors
            .iter()
            .map(|anchor| Pin {
                tick: anchor.tick,
                seconds: charter_core::ResolvedTime::from(anchor.seconds).seconds(),
            })
            .collect()
    }

    /// The tempo after the last anchor, the one tempo set rather than following from anchors.
    #[must_use]
    pub fn tail_bpm(&self) -> Option<Bpm> {
        Some(self.timing().tempo)
    }

    /// Makes the beat on `tick` fall `seconds` into the audio, as one step, anchoring it there
    /// if it was not. The anchors on either side keep their moments, the tempos between follow,
    /// and a beat after the last anchor keeps the tempo after it. Does nothing when the beat is
    /// already an anchor there.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song, and with [`Refusal::Crosses`] when the beat would reach or
    /// pass a neighbouring anchor's moment.
    pub fn sync(&mut self, tick: Tick, seconds: f64) -> Result<(), Refusal> {
        let mut pins = self.anchors();
        match pins.iter_mut().find(|pin| pin.tick == tick) {
            Some(pin) if (pin.seconds - seconds).abs() <= APART => return Ok(()),
            Some(pin) => pin.seconds = seconds,
            None => {
                let at = pins.partition_point(|pin| pin.tick < tick);
                pins.insert(at, Pin { tick, seconds });
            }
        }
        self.repin(&pins, self.tail(), false)
    }

    /// Moves the anchor on `from` to the beat `to`, keeping its moment in the audio, as one step;
    /// see [`Song::move_anchors`].
    ///
    /// # Errors
    ///
    /// As [`Song::move_anchors`].
    pub fn move_anchor(&mut self, from: Tick, to: Tick) -> Result<(), Refusal> {
        self.move_anchors(&[(from, to)])
    }

    /// Moves each anchor on the first tick of `moves` to the beat on its second, all keeping
    /// their moments in the audio, as one step: the waveform slides under the grid. Moves of
    /// ticks that hold no anchor are left out. Nothing moves unless every anchor can.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song, and with [`Refusal::Crosses`] when an anchor would reach or
    /// pass one that stays.
    pub fn move_anchors(&mut self, moves: &[(Tick, Tick)]) -> Result<(), Refusal> {
        let mut pins = self.anchors();
        let mut changed = false;
        for pin in &mut pins {
            if let Some(&(_, to)) = moves.iter().find(|(from, _)| *from == pin.tick) {
                changed |= pin.tick != to;
                pin.tick = to;
            }
        }
        if !changed {
            return Ok(());
        }
        in_order(&pins, |pin| i64::from(pin.tick))?;
        pins.sort_by_key(|pin| pin.tick);
        self.repin(&pins, self.tail(), false)
    }

    /// Moves the anchors on `ticks` `by` seconds later in the audio, keeping their beats, as one
    /// step, or as part of the step before when `merge`, so a run of nudges undoes at once.
    /// Nothing moves unless every anchor can.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song, and with [`Refusal::Crosses`] when an anchor would reach or
    /// pass one that stays.
    pub fn nudge_anchors(&mut self, ticks: &[Tick], by: f64, merge: bool) -> Result<(), Refusal> {
        let mut pins = self.anchors();
        let mut moved = false;
        for pin in pins.iter_mut().filter(|pin| ticks.contains(&pin.tick)) {
            pin.seconds += by;
            moved = true;
        }
        if !moved || by == 0.0 {
            return Ok(());
        }
        self.repin(&pins, self.tail(), merge)
    }

    /// Removes the anchors on `ticks`, as one step. The anchors left keep their moments and the
    /// tempo between them follows; the last left keeps the tempo it had as the tail.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song, and with [`Refusal::OnlyTempo`] when no anchor would be left.
    pub fn unsync_anchors(&mut self, ticks: &[Tick]) -> Result<(), Refusal> {
        let before = self.anchors();
        let kept: Vec<Pin> = before
            .iter()
            .filter(|pin| !ticks.contains(&pin.tick))
            .copied()
            .collect();
        if kept.len() == before.len() {
            return Ok(());
        }
        if kept.is_empty() {
            return Err(Refusal::OnlyTempo);
        }
        self.repin(&kept, self.tail(), false)
    }

    /// Removes the anchor on `tick`; see [`Song::unsync_anchors`].
    ///
    /// # Errors
    ///
    /// As [`Song::unsync_anchors`].
    pub fn unsync(&mut self, tick: Tick) -> Result<(), Refusal> {
        self.unsync_anchors(&[tick])
    }

    /// Sets the tempo after the last anchor, as one step, or as part of the step before when
    /// `merge`. Does nothing when it has that tempo already.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song, or when the game refuses the tempo.
    pub fn set_tail_bpm(&mut self, bpm: Bpm, merge: bool) -> Result<(), Refusal> {
        if self.tail_bpm() == Some(bpm) {
            return Ok(());
        }
        let pins = self.anchors();
        self.repin(&pins, bpm, merge)
    }

    /// Doubles the tempo after each anchor on `ticks`, or halves it when not `double`, as one
    /// step. Every anchor keeps its moment in the audio: a segment's beats double or halve, and
    /// the anchors and timing events after it move by as many ticks, so the audio after keeps its
    /// grid. The tail's tempo simply doubles or halves. Notes and markers keep their ticks, so
    /// this is for mapping a song before charting it.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song, when a halved segment would not be whole ticks, and when a
    /// tempo or tick would go out of range.
    pub fn scale_tempo(&mut self, ticks: &[Tick], double: bool) -> Result<(), Refusal> {
        let pins = self.anchors();
        let tail = self.tail();
        // Each scaled segment, by its old start and end: ticks after it move by what it grew.
        let mut segments: Vec<(i64, i64)> = Vec::new();
        for pair in pins.windows(2) {
            if let [from, to] = pair
                && ticks.contains(&from.tick)
            {
                segments.push((i64::from(from.tick), i64::from(to.tick)));
            }
        }
        let mapped = |tick: Tick| -> Result<Tick, Refusal> {
            let old = i64::from(tick);
            let mut new = old;
            for &(start, end) in &segments {
                let span = end - start;
                let grown = if double { span } else { -span / 2 };
                if !double && span % 2 != 0 {
                    return Err(Refusal::NotWhole);
                }
                if old >= end {
                    new += grown;
                } else if old > start {
                    let within = old - start;
                    if !double && within % 2 != 0 {
                        return Err(Refusal::NotWhole);
                    }
                    new += if double { within } else { -within / 2 };
                }
            }
            i32::try_from(new)
                .map(Tick::new)
                .map_err(|_| Refusal::NotWhole)
        };
        let (_, timing) = self.edited_timing();
        let mut base = timing.clone();
        for event in &mut base.events {
            event.tick = mapped(event.tick)?;
        }
        let pins = pins
            .iter()
            .map(|pin| {
                Ok(Pin {
                    tick: mapped(pin.tick)?,
                    seconds: pin.seconds,
                })
            })
            .collect::<Result<Vec<_>, Refusal>>()?;
        let last = self.anchors().last().map(|pin| pin.tick);
        let tail = if last.is_some_and(|last| ticks.contains(&last)) {
            let scaled = f64::from(tail) * if double { 2.0 } else { 0.5 };
            micro(scaled)
                .and_then(|bpm| Bpm::try_from(bpm).ok())
                .ok_or(Refusal::NotWhole)?
        } else {
            tail
        };
        let new = self.rebuilt(base, &pins, tail)?;
        let (own, _) = self.edited_timing();
        self.retime(own, new)
    }

    /// Makes `pins` the anchors, with `tail` after the last, as one step, or as part of the step
    /// before when `merge`: the tempo between each two follows from where they are, the offset
    /// from where the first is, and every other timing event stays. A segment whose ends did not
    /// move keeps its tempo exactly.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song, with [`Refusal::Crosses`] when the pins are not in order in
    /// both beats and moments, naming the first out of place, and with [`Refusal::OnlyTempo`]
    /// when there are none.
    pub fn repin(&mut self, pins: &[Pin], tail: Bpm, merge: bool) -> Result<(), Refusal> {
        let (_, timing) = self.edited_timing();
        let new = self.rebuilt(timing.clone(), pins, tail)?;
        let (own, old) = self.edited_timing();
        if *old == new {
            return Ok(());
        }
        if merge {
            self.retime_merged(own, new)
        } else {
            self.retime(own, new)
        }
    }

    /// `base` with its anchors rebuilt from `pins` and `tail`.
    fn rebuilt(&self, base: Timing, pins: &[Pin], tail: Bpm) -> Result<Timing, Refusal> {
        if pins.is_empty() {
            return Err(Refusal::OnlyTempo);
        }
        in_order(pins, |pin| i64::from(pin.tick))?;
        in_order_in_time(pins)?;
        let mut new = base;
        new.anchors = pins
            .iter()
            .map(|pin| {
                Ok(Anchor {
                    tick: pin.tick,
                    seconds: Origin::try_from(micro(pin.seconds).ok_or(Refusal::NotWhole)?)
                        .map_err(|_| Refusal::NotWhole)?,
                    ext: Extensions::new(),
                })
            })
            .collect::<Result<Vec<_>, Refusal>>()?;
        new.tempo = tail;
        self.checked(&new)?;
        Ok(new)
    }

    /// The tempo after the last anchor now.
    fn tail(&self) -> Bpm {
        self.tail_bpm()
            .unwrap_or_else(|| Bpm::try_from(120.0).unwrap_or_else(|_| unreachable!()))
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
}

/// Refuses `pins` unless `key` rises strictly from each to the next, naming the first that
/// does not.
fn in_order(pins: &[Pin], key: impl Fn(&Pin) -> i64) -> Result<(), Refusal> {
    for pair in pins.windows(2) {
        if let [a, b] = pair
            && key(b) <= key(a)
        {
            return Err(Refusal::Crosses(b.tick));
        }
    }
    Ok(())
}

/// Refuses `pins` unless each falls later in the audio than the one before.
fn in_order_in_time(pins: &[Pin]) -> Result<(), Refusal> {
    for pair in pins.windows(2) {
        if let [a, b] = pair
            && b.seconds - a.seconds <= APART
        {
            return Err(Refusal::Crosses(b.tick));
        }
    }
    Ok(())
}

/// `seconds` to the nearest microsecond, the finest time the format holds.
fn micro(seconds: f64) -> Option<f64> {
    let rounded = (seconds * 1e6).round() / 1e6;
    rounded.is_finite().then_some(rounded)
}

#[cfg(test)]
mod tests {
    use charter_core::TimingKind;
    use serde_json::{Value, json};

    use super::*;
    use crate::song::tests::{bpm_event, song_events, timed};

    fn at(tick: i32) -> Tick {
        Tick::new(tick)
    }

    fn offset(song: &Song) -> f64 {
        song.document().timing().time_at(Tick::new(0)).seconds()
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
        assert!(matches!(song.sync(at(1920), 2.5), Err(Refusal::Crosses(_))));
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
            Err(Refusal::Crosses(_))
        ));
        assert!(matches!(
            song.move_anchor(at(1920), at(0)),
            Err(Refusal::Crosses(_))
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
    fn the_first_anchor_can_go_while_another_is_left_but_not_the_only_one() {
        let events = json!([bpm_event(0, 120.0), bpm_event(1920, 150.0)]);
        let mut song = timed(&events, None);
        song.unsync(at(960)).unwrap();
        assert!(!song.can_undo());
        song.unsync(at(0)).unwrap();
        assert_eq!(song_events(&song), json!([bpm_event(1920, 150.0)]));
        assert!((song.timing().time_at(at(1920)).seconds() - 1.0).abs() < 1e-6);
        assert!(matches!(song.unsync(at(1920)), Err(Refusal::OnlyTempo)));
    }

    #[test]
    fn anchors_read_back_where_they_fall_and_the_tail_is_the_last_tempo() {
        let song = timed(&json!([bpm_event(0, 120.0), bpm_event(1920, 60.0)]), None);
        let pins = song.anchors();
        assert_eq!(pins.len(), 2);
        assert!((pins[1].seconds - 1.0).abs() < 1e-9);
        assert_eq!(song.tail_bpm(), Some(Bpm::try_from(60.0).unwrap()));
    }

    fn four() -> Value {
        json!([
            bpm_event(0, 120.0),
            bpm_event(1920, 150.0),
            bpm_event(3840, 133.333_333),
            bpm_event(5760, 97.5)
        ])
    }

    #[test]
    fn a_nudge_leaves_the_tempo_of_segments_whose_ends_stay_exactly_as_it_was() {
        let mut song = timed(&four(), None);
        song.nudge_anchors(&[at(1920)], 0.01, false).unwrap();
        let events = song_events(&song);
        assert_eq!(events[2], bpm_event(3840, 133.333_333));
        assert_eq!(events[3], bpm_event(5760, 97.5));
        assert_ne!(events[1], bpm_event(1920, 150.0));
        assert_eq!(
            serde_json::to_value(song.timing().anchors[0].seconds).unwrap(),
            json!(0.0)
        );
    }

    #[test]
    fn a_selection_nudged_into_a_neighbour_moves_nothing_and_says_which() {
        let mut song = timed(&four(), None);
        let far = song.nudge_anchors(&[at(1920), at(3840)], 5.0, false);
        assert!(matches!(far, Err(Refusal::Crosses(tick)) if tick == at(5760)));
        assert_eq!(song_events(&song), four());
        song.nudge_anchors(&[at(1920), at(3840)], 0.05, false)
            .unwrap();
        let seconds: Vec<f64> = song.anchors().iter().map(|pin| pin.seconds).collect();
        for (got, want) in seconds.iter().zip([0.0, 1.05, 1.85, 2.7]) {
            assert!((got - want).abs() < 1e-6, "{seconds:?}");
        }
    }

    #[test]
    fn twenty_nudges_merged_undo_as_one_step() {
        let mut song = timed(&four(), None);
        let before = song.anchors()[1].seconds;
        song.nudge_anchors(&[at(1920)], 0.001, false).unwrap();
        for _ in 1..20 {
            song.nudge_anchors(&[at(1920)], 0.001, true).unwrap();
        }
        assert!((song.anchors()[1].seconds - before - 0.02).abs() < 1e-6);
        song.undo().unwrap();
        assert_eq!(song_events(&song), four());
        assert!(!song.can_undo());
    }

    #[test]
    fn a_selection_moved_on_the_grid_moves_as_a_whole_or_not_at_all() {
        let mut song = timed(&four(), None);
        let blocked = song.move_anchors(&[(at(1920), at(3840))]);
        assert!(matches!(blocked, Err(Refusal::Crosses(_))));
        song.move_anchors(&[(at(1920), at(2400)), (at(3840), at(4320))])
            .unwrap();
        let ticks: Vec<Tick> = song.anchors().iter().map(|pin| pin.tick).collect();
        assert_eq!(ticks, [at(0), at(2400), at(4320), at(5760)]);
    }

    #[test]
    fn the_tail_tempo_is_set_and_a_run_of_settings_undoes_at_once() {
        let mut song = timed(&four(), None);
        song.set_tail_bpm(Bpm::try_from(97.6).unwrap(), false)
            .unwrap();
        song.set_tail_bpm(Bpm::try_from(97.7).unwrap(), true)
            .unwrap();
        assert_eq!(song_events(&song)[3], bpm_event(5760, 97.7));
        assert_eq!(song_events(&song)[2], bpm_event(3840, 133.333_333));
        song.undo().unwrap();
        assert_eq!(song_events(&song), four());
    }

    #[test]
    fn doubling_a_segment_keeps_every_anchor_in_the_audio_and_moves_the_ticks_after() {
        let meter = json!({ "tick": 3840, "kind": { "time_signature": [3, 4] } });
        let events = json!([
            bpm_event(0, 120.0),
            bpm_event(1920, 60.0),
            meter,
            bpm_event(3840, 60.0)
        ]);
        let mut song = timed(&events, None);
        let times: Vec<f64> = song.anchors().iter().map(|pin| pin.seconds).collect();
        song.scale_tempo(&[at(1920)], true).unwrap();
        let pins = song.anchors();
        let ticks: Vec<Tick> = pins.iter().map(|pin| pin.tick).collect();
        assert_eq!(ticks, [at(0), at(1920), at(5760)]);
        for (pin, was) in pins.iter().zip(times) {
            assert!((pin.seconds - was).abs() < 1e-6);
        }
        assert_eq!(song_events(&song)[1], bpm_event(1920, 120.0));
        let moved = song.timing().events.iter().any(|event| {
            event.tick == at(5760) && matches!(event.kind, TimingKind::TimeSignature(_))
        });
        assert!(moved);
        song.scale_tempo(&[at(5760)], false).unwrap();
        assert_eq!(song.tail_bpm(), Some(Bpm::try_from(30.0).unwrap()));
    }

    #[test]
    fn two_anchors_a_bar_and_two_seconds_apart_make_120_bpm_either_side() {
        let mut song = timed(&json!([bpm_event(0, 90.0)]), None);
        let pins = [
            Pin {
                tick: at(0),
                seconds: 1.0,
            },
            Pin {
                tick: at(3840),
                seconds: 3.0,
            },
        ];
        song.repin(&pins, Bpm::try_from(120.0).unwrap(), false)
            .unwrap();
        assert_eq!(song_events(&song)[0], bpm_event(0, 120.0));
        let timing = song.timing();
        assert!((timing.time_at(at(1920)).seconds() - 2.0).abs() < 1e-9);
        assert_eq!(timing.latest_tick_by(4.0), at(5760));
        assert_eq!(timing.latest_tick_by(0.0), at(-1920));
    }

    #[test]
    fn a_segment_of_an_odd_number_of_ticks_does_not_halve() {
        let mut song = timed(&json!([bpm_event(0, 120.0), bpm_event(1921, 120.0)]), None);
        assert!(matches!(
            song.scale_tempo(&[at(0)], false),
            Err(Refusal::NotWhole)
        ));
    }
}
