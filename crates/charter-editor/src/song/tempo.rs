//! Timing, tempo and timed spans.

use super::notes::{moved, rounded_tick_delta, shifted_at, tick_delta_as_f64};
#[cfg_attr(
    not(test),
    expect(
        clippy::wildcard_imports,
        reason = "song submodules share one private implementation vocabulary"
    )
)]
use super::*;

/// Where a tempo edit sends each tick.
enum TempoShift<'a> {
    /// Ticks before `old_next` stay put; ticks after it move together.
    After {
        /// Old start of the following segment.
        old_next: Tick,
        /// Tick delta for `old_next` and everything after it.
        by: i64,
    },
    /// Timed ticks after `start` are stretched until `next`; ticks after it move together.
    Stretch {
        /// Timing map whose pauses and warps decide what stretches.
        timing: Valid<'a, Timing>,
        /// Tick count in one beat.
        resolution: TicksPerBeat,
        /// Floating-point copy of `resolution`.
        per_beat: f64,
        /// New tempo divided by old tempo.
        ratio: f64,
        /// First tick of the edited segment.
        start: Tick,
        /// Old start of the next segment, when there is one.
        next: Option<Tick>,
        /// Tick delta for `next` and everything after it.
        by: i64,
    },
}

impl TempoShift<'_> {
    /// The new tick for `tick`.
    fn apply(&self, tick: Tick) -> Result<Tick, Refusal> {
        match *self {
            Self::After { old_next, by } => {
                if tick < old_next {
                    Ok(tick)
                } else {
                    moved(tick, by)
                }
            }
            Self::Stretch {
                timing,
                resolution,
                per_beat,
                ratio,
                start,
                next,
                by,
            } => {
                if next.is_some_and(|old_next| tick >= old_next) {
                    return moved(tick, by);
                }
                if tick <= start {
                    return Ok(tick);
                }
                let span = TimedSpan::between(timing, start, tick).ok_or(Refusal::NotWhole)?;
                let timed = span.timed_beats(resolution) * per_beat;
                let warped = tick_delta_as_f64(tick, start) - timed;
                moved(start, rounded_tick_delta(timed * ratio + warped))
            }
        }
    }
}

impl Song {
    /// The timing tempo edits go to: the open chart's own when it has one, and whether it is.
    pub(crate) fn edited_timing(&self) -> (bool, &charter_core::Timing) {
        let document = self.document.document();
        let own = document
            .charts
            .get(self.chart)
            .and_then(|chart| chart.timing.as_ref());
        own.map_or((false, &document.timing), |own| (true, own))
    }

    /// The tempo in effect at the cursor, and the anchor tick whose segment owns it.
    #[must_use]
    pub fn bpm(&self) -> Option<(Tick, Bpm)> {
        let timing = self.timing();
        let anchors = &timing.anchors;
        let index = anchors.partition_point(|anchor| anchor.tick <= self.cursor);
        let anchor = anchors
            .get(index.saturating_sub(1))
            .or_else(|| anchors.first())?;
        timing.bpm_at(anchor.tick).map(|bpm| (anchor.tick, bpm))
    }

    /// Gives the BPM change in effect at the cursor, the one [`Song::bpm`] names, the tempo
    /// `bpm`, as one step. Does nothing when it already has it.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song, or when the game refuses the tempo.
    pub fn set_bpm(&mut self, bpm: Bpm) -> Result<(), Refusal> {
        let Some((at, held)) = self.bpm() else {
            return Ok(());
        };
        if held == bpm {
            return Ok(());
        }
        self.set_bpm_change_at(at, bpm)
    }

    /// Sets `bpm` at `at`, inserting an anchor there if needed, in one checked edit.
    pub(crate) fn set_bpm_change_at(&mut self, at: Tick, bpm: Bpm) -> Result<(), Refusal> {
        let (own, timing) = self.edited_timing();
        let mut new = timing.clone();
        let index = if let Some(index) = new.anchors.iter().position(|anchor| anchor.tick == at) {
            index
        } else {
            let index = new.anchors.partition_point(|anchor| anchor.tick < at);
            let seconds = self.timing().time_at(at).seconds();
            new.anchors.insert(
                index,
                charter_core::Anchor {
                    tick: at,
                    seconds: charter_core::Origin::nearest(seconds)
                        .map_err(|_invalid_origin| Refusal::NotWhole)?,
                },
            );
            index
        };
        let (Some(from), next) = (
            new.anchors.get(index).cloned(),
            new.anchors.get(index.saturating_add(1)).cloned(),
        ) else {
            return Ok(());
        };
        if next.is_none() {
            new.tempo = bpm;
        }
        let map = match (self.keep, next) {
            (KeepNotes::OnBeats, None) => return self.retime(own, new),
            (KeepNotes::OnBeats, Some(next)) => self.on_beats(&from, &next, bpm)?,
            (KeepNotes::InTime, next) => self.in_time(&from, next.as_ref(), bpm)?,
        };
        for anchor in new.anchors.iter_mut().skip(index.saturating_add(1)) {
            anchor.tick = map.apply(anchor.tick)?;
        }
        for event in &mut new.events {
            let tick = map.apply(event.tick)?;
            if let TimingKind::Warp(length) = &mut event.kind {
                let end = map.apply(event.tick.checked_add(*length).ok_or(Refusal::NotWhole)?)?;
                *length = tick.ticks_until(end).ok_or(Refusal::Crosses(event.tick))?;
            }
            event.tick = tick;
        }
        new.events.sort_by_key(|event| event.tick);
        let mut ops = vec![Op::Timing {
            own,
            old: Box::new(timing.clone()),
            new: Box::new(new),
        }];
        ops.extend(self.shifted_along(own, &map)?);
        let edit = Edit(ops);
        self.document = self.edited(self.chart, &edit)?;
        self.record(edit);
        Ok(())
    }

    /// Where each tick goes when `bpm` is set from `from`, keeping everything on its beats up to
    /// `next`: every anchor keeps its seconds, so `bpm` sets how many ticks fit before `next`,
    /// and everything from there on moves by the difference to stay on its audio.
    fn on_beats(&self, from: &Anchor, next: &Anchor, bpm: Bpm) -> Result<TempoShift<'_>, Refusal> {
        let resolution = self.timing().ticks_per_beat();
        let per_beat = f64::from(u16::from(resolution));
        let apart = charter_core::ResolvedTime::from(next.seconds).seconds()
            - charter_core::ResolvedTime::from(from.seconds).seconds();
        // Only the pauses and warps before the new end count, and the end depends on them, so
        // it is found again until it settles; each pass can only drop events, so it soon does.
        let mut to = i64::from(next.tick);
        for _ in 0..8 {
            let end = Tick::new(
                i32::try_from(to.min(i64::from(next.tick)))
                    .map_err(|_out_of_range| Refusal::NotWhole)?,
            );
            let span =
                TimedSpan::between(self.timing(), from.tick, end).ok_or(Refusal::NotWhole)?;
            let timed = span.timed_beats(resolution) * per_beat;
            let warped = tick_delta_as_f64(end, from.tick) - timed;
            let beats = (apart - span.paused_seconds()) * f64::from(bpm) / 60.0;
            let settled =
                i64::from(from.tick).saturating_add(rounded_tick_delta(beats * per_beat + warped));
            if settled == to {
                break;
            }
            to = settled;
        }
        if to <= i64::from(from.tick) {
            return Err(Refusal::Crosses(next.tick));
        }
        let old_next = next.tick;
        let by = to.saturating_sub(i64::from(old_next));
        Ok(TempoShift::After { old_next, by })
    }

    /// Where each tick goes when `bpm` is set from `from`, keeping everything where it sounds:
    /// the timed ticks up to `next`, or to the end without one, stretch by the change of tempo
    /// while warps keep their lengths and pauses their seconds, and everything from `next` on
    /// moves by the ticks `next` moved.
    fn in_time(
        &self,
        from: &Anchor,
        next: Option<&Anchor>,
        bpm: Bpm,
    ) -> Result<TempoShift<'_>, Refusal> {
        let timing = self.timing();
        let resolution = timing.ticks_per_beat();
        let per_beat = f64::from(u16::from(resolution));
        let ratio = f64::from(bpm) / timing.tempo_at(from.tick).bpm();
        let start = from.tick;
        let stretched = TempoShift::Stretch {
            timing,
            resolution,
            per_beat,
            ratio,
            start,
            next: None,
            by: 0,
        };
        let Some(next) = next else {
            return Ok(stretched);
        };
        let old_next = next.tick;
        let to = stretched.apply(old_next)?;
        if to <= start {
            return Err(Refusal::Crosses(old_next));
        }
        let by = old_next.offset_to(to).get();
        Ok(TempoShift::Stretch {
            timing,
            resolution,
            per_beat,
            ratio,
            start,
            next: Some(old_next),
            by,
        })
    }

    /// The edits that move by `shift` the objects of every chart the timing tempo edits go to
    /// times, the open chart's own when `own` and otherwise every chart on the song's, and the
    /// song's markers along with the song's timing.
    fn shifted_along(&self, own: bool, shift: &TempoShift<'_>) -> Result<Vec<Op>, Refusal> {
        let document = self.document.document();
        let mut ops = Vec::new();
        for (chart, edited) in document.charts.iter().enumerate() {
            let follows = if own {
                chart == self.chart
            } else {
                edited.timing.is_none()
            };
            if !follows {
                continue;
            }
            let mut objects = edited
                .objects
                .iter()
                .map(|object| shifted_at(object, |tick| shift.apply(tick)))
                .collect::<Result<Vec<_>, _>>()?;
            objects.sort_by_key(|object| object.tick);
            if objects != edited.objects {
                ops.push(Op::Objects {
                    chart,
                    old: edited.objects.clone(),
                    new: objects,
                });
            }
        }
        if !own {
            let mut markers = document.markers.clone();
            for marker in &mut markers {
                marker.tick = shift.apply(marker.tick)?;
            }
            markers.sort_by_key(|marker| marker.tick);
            if markers != document.markers {
                ops.push(Op::Markers {
                    old: document.markers.clone(),
                    new: markers,
                });
            }
        }
        Ok(ops)
    }

    /// Starts a new BPM change at the cursor, at the tempo already in effect there, as one
    /// step, so the tempo from the cursor on can be set apart. Does nothing when one is there.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song.
    pub fn add_bpm_change(&mut self) -> Result<(), Refusal> {
        let at = self.cursor;
        let Some((from, _held)) = self.bpm() else {
            return Ok(());
        };
        if from == at {
            return Ok(());
        }
        let (own, timing) = self.edited_timing();
        let mut new = timing.clone();
        let index = new.anchors.partition_point(|anchor| anchor.tick <= at);
        let change = charter_core::Anchor {
            tick: at,
            seconds: charter_core::Origin::nearest(self.timing().time_at(at).seconds())
                .map_err(|_invalid_origin| Refusal::NotWhole)?,
        };
        new.anchors.insert(index, change);
        self.retime(own, new)
    }

    /// Removes the BPM change in effect at the cursor, as one step, so the one before it rules
    /// on.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song, and for the first BPM change, which starts the tempo.
    pub fn remove_bpm(&mut self) -> Result<(), Refusal> {
        if self.read_only().is_some() {
            return Err(Refusal::ReadOnly);
        }
        let Some((at, _)) = self.bpm() else {
            return Ok(());
        };
        let (own, timing) = self.edited_timing();
        if timing.anchors.len() == 1 {
            return Err(Refusal::FirstTempo);
        }
        let mut new = timing.clone();
        if new.anchors.last().is_some_and(|anchor| anchor.tick == at)
            && let Some(previous) = new.anchors.iter().rev().nth(1)
            && let Ok(tempo) = self.timing().bpm_at(previous.tick).ok_or(())
        {
            new.tempo = tempo;
        }
        new.anchors.retain(|anchor| anchor.tick != at);
        self.retime(own, new)
    }

    /// Moves every event at `from` of the timing tempo edits go to onto `to`, a BPM change and
    /// a meter at one tick moving as one timing point, as one step. Does nothing when there are
    /// none at `from` or `from` is `to`.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song, and when an event of a kind moved is already at `to`.
    pub fn move_timing_at(&mut self, from: Tick, to: Tick) -> Result<(), Refusal> {
        let (own, timing) = self.edited_timing();
        if from == to
            || (!timing.events.iter().any(|event| event.tick == from)
                && !timing.anchors.iter().any(|anchor| anchor.tick == from))
        {
            return Ok(());
        }
        let mut new = timing.clone();
        for anchor in &mut new.anchors {
            if anchor.tick == from {
                anchor.tick = to;
            }
        }
        new.anchors.sort_by_key(|anchor| anchor.tick);
        let (mut moved, kept): (Vec<_>, Vec<_>) =
            new.events.drain(..).partition(|event| event.tick == from);
        new.events = kept;
        for event in &mut moved {
            event.tick = to;
        }
        let at = new.events.partition_point(|event| event.tick <= to);
        new.events.splice(at..at, moved);
        self.retime(own, new)
    }
}

impl Song {
    /// Puts tick 0 of the timing tempo edits go to `offset` seconds into the audio.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song.
    pub fn set_offset(&mut self, offset: charter_core::Origin) -> Result<(), Refusal> {
        let (own, timing) = self.edited_timing();
        let current = self.timing().time_at(Tick::new(0)).seconds();
        let wanted = charter_core::ResolvedTime::from(offset).seconds();
        let delta = wanted - current;
        if delta.abs() <= 1e-9 {
            return Ok(());
        }
        let mut new = timing.clone();
        for anchor in &mut new.anchors {
            let seconds = charter_core::ResolvedTime::from(anchor.seconds).seconds() + delta;
            anchor.seconds = charter_core::Origin::nearest(seconds)
                .map_err(|_invalid_origin| Refusal::NotWhole)?;
        }
        self.retime(own, new)
    }

    /// Gives the timing tempo edits go to `new`, as part of the step before when that step
    /// changed only the same timing and nothing was undone since, and as a step of its own
    /// otherwise, so a run of nudges undoes at once.
    pub(crate) fn retime_merged(
        &mut self,
        own: bool,
        new: charter_core::Timing,
    ) -> Result<(), Refusal> {
        let (_, old) = self.edited_timing();
        let mut edit = Edit(vec![Op::Timing {
            own,
            old: Box::new(old.clone()),
            new: Box::new(new),
        }]);
        let document = self.edited(self.chart, &edit)?;
        let joins = self.history.can_merge_timing(self.chart, own);
        if !joins {
            self.document = document;
            self.record(edit);
            return Ok(());
        }
        self.history.merge_last_timing(&mut edit);
        self.document = document;
        self.history.touch_revision();
        Ok(())
    }

    /// Puts `timing`, such as one imported from another program, in place of the timing tempo
    /// edits go to, offset included, as one step. The notes keep their ticks, so they move with
    /// the new tempo. Does nothing when it is the timing there already.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song, and when the timing breaks a rule of the format or the game.
    pub fn replace_timing(&mut self, timing: charter_core::Timing) -> Result<(), Refusal> {
        let (own, old) = self.edited_timing();
        if *old == timing {
            return Ok(());
        }
        self.retime(own, timing)
    }

    /// Gives the timing tempo edits go to the events of `new`, as one step.
    pub(crate) fn retime(&mut self, own: bool, new: charter_core::Timing) -> Result<(), Refusal> {
        let (_, old) = self.edited_timing();
        let edit = Edit(vec![Op::Timing {
            own,
            old: Box::new(old.clone()),
            new: Box::new(new),
        }]);
        self.document = self.edited(self.chart, &edit)?;
        self.record(edit);
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
/// Timed ticks and pauses in a stretch of timing, with warps left out of the timed ticks.
pub struct TimedSpan {
    /// Ticks through which the tempo advances, excluding any warp.
    timed_ticks: f64,
    /// Seconds added by stops or delays in the span.
    paused_seconds: f64,
}

impl TimedSpan {
    /// The span between two ticks of a validated timing.
    #[must_use]
    pub fn between(timing: Valid<'_, Timing>, from: Tick, to: Tick) -> Option<Self> {
        span_parts(&timing, from, to)
    }

    /// What share of the timed ticks in `whole` this span holds.
    #[must_use]
    pub fn fraction_of(self, whole: Self) -> Option<f64> {
        (whole.timed_ticks > 0.0).then_some(self.timed_ticks / whole.timed_ticks)
    }

    /// Seconds spent stopped or delayed in this span.
    #[must_use]
    pub const fn paused_seconds(self) -> f64 {
        self.paused_seconds
    }

    /// Beats whose tempo advances through this span.
    #[must_use]
    pub fn timed_beats(self, ticks_per_beat: TicksPerBeat) -> f64 {
        self.timed_ticks / f64::from(u16::from(ticks_per_beat))
    }

    /// Seconds this span takes when its timed ticks run at `bpm`.
    #[must_use]
    pub fn at_bpm(self, bpm: Bpm, ticks_per_beat: TicksPerBeat) -> f64 {
        self.paused_seconds + self.timed_beats(ticks_per_beat) * 60.0 / f64::from(bpm)
    }
}

/// Timed ticks and pause duration from `from` to `to`, or `None` for reversed bounds.
fn span_parts(timing: &Timing, from: Tick, to: Tick) -> Option<TimedSpan> {
    if to < from {
        return None;
    }
    let timed = |cursor: Tick, until: Tick, warp_end: Option<Tick>| {
        let start = warp_end.map_or(cursor, |end| end.max(cursor));
        start
            .ticks_until(until)
            .map_or(0.0, |ticks| f64::from(u32::from(ticks)))
    };
    let mut timed_ticks = 0.0;
    let mut pauses = 0.0;
    let mut cursor = from;
    let mut warp_end = None;
    for event in &timing.events {
        let at = event.tick;
        if at > to {
            break;
        }
        if at > from {
            timed_ticks += timed(cursor, at, warp_end);
            cursor = at;
        }
        match event.kind {
            TimingKind::Stop(pause) if at >= from && at < to => pauses += f64::from(pause),
            TimingKind::Delay(pause) if at > from => pauses += f64::from(pause),
            TimingKind::Warp(ticks) => warp_end = Some(at.checked_add(ticks)?),
            TimingKind::Stop(_)
            | TimingKind::Delay(_)
            | TimingKind::TimeSignature(..)
            | TimingKind::Other { .. } => {}
        }
    }
    timed_ticks += timed(cursor, to, warp_end);
    Some(TimedSpan {
        timed_ticks,
        paused_seconds: pauses,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::song::tests::*;
    use serde_json::{Value, json};

    /// A song whose timing has `events`, and whose second chart has `own` timing if given.
    pub(crate) fn timed(events: &Value, own: Option<&Value>) -> Song {
        let mut value = json!({
            "format_version": 0,
            "ruleset": { "id": "test", "version": "1" },
            "metadata": { "title": "t", "artist": "a", "audio": "song.ogg" },
            "timing": { "offset_seconds": 0.0, "events": events.clone() },
            "charts": [ { "name": "Easy", "lanes": 4 }, { "name": "Hard", "lanes": 4 } ]
        });
        if let Some(own) = own {
            value["charts"][1]["timing"] = json!({ "offset_seconds": 0.0, "events": own.clone() });
        }
        let document = ValidDocument::from_json(&serde_json::to_vec(&value).unwrap()).unwrap();
        Song::new(document, EDITABLE)
    }

    pub(crate) fn bpm_event(tick: i32, bpm: f64) -> Value {
        json!({ "tick": tick, "kind": { "bpm": bpm } })
    }

    pub(crate) fn song_events(song: &Song) -> Value {
        let timing = song.document().timing();
        let mut events: Vec<Value> = timing
            .tempos()
            .into_iter()
            .map(|(tick, bpm)| {
                let rounded = (bpm * 1e6).round() / 1e6;
                bpm_event(i32::try_from(i64::from(tick)).unwrap(), rounded)
            })
            .collect();
        events.extend(
            song.document()
                .document()
                .timing
                .events
                .iter()
                .map(|event| serde_json::to_value(event).unwrap()),
        );
        events.sort_by_key(|event| {
            let tick = event["tick"].as_i64().unwrap();
            let is_bpm = event["kind"].get("bpm").is_some();
            (tick, i32::from(is_bpm && tick != 0))
        });
        Value::Array(events)
    }

    fn tempo(bpm: f64) -> Bpm {
        Bpm::try_from(bpm).unwrap()
    }

    #[test]
    fn a_timed_span_does_not_run_backwards() {
        let song = timed(&json!([bpm_event(0, 120.0)]), None);
        assert_eq!(
            TimedSpan::between(song.timing(), Tick::new(960), Tick::new(0)),
            None
        );
    }

    #[test]
    fn a_bpm_set_between_changes_changes_the_one_in_effect_undo_takes_back() {
        let meter = json!({ "tick": 960, "kind": { "time_signature": [3, 4] } });
        let events = json!([bpm_event(0, 120.0), meter, bpm_event(1920, 90.0)]);
        let mut song = timed(&events, None);
        song.keep_notes(KeepNotes::OnBeats);
        song.step(Step::Later);
        assert_eq!(song.cursor(), Tick::new(240));
        song.set_bpm(tempo(150.0)).unwrap();
        // The 90 BPM point stays a second in, now two and a half beats at 150 BPM from the start.
        let expected = json!([bpm_event(0, 150.0), meter, bpm_event(2400, 90.0)]);
        assert_eq!(song_events(&song), expected);
        assert_eq!(song.bpm(), Some((Tick::new(0), tempo(150.0))));
        song.undo().unwrap();
        assert_eq!(song.bpm(), Some((Tick::new(0), tempo(120.0))));
    }

    fn seconds(song: &Song, tick: i32) -> f64 {
        song.timing().time_at(Tick::new(tick)).seconds()
    }

    #[test]
    fn a_bpm_keeps_every_later_point_and_note_on_its_audio_in_every_chart_it_times() {
        let mut song = timed(&json!([bpm_event(0, 120.0), bpm_event(1920, 90.0)]), None);
        song.keep_notes(KeepNotes::OnBeats);
        for (tick, lane_index) in [(960, 0), (2880, 1), (1440, 2)] {
            song.toggle(Tick::new(tick), lane(lane_index), Kind::Tap)
                .unwrap();
        }
        song.stretch(lane(2), Tick::new(1440), Tick::new(2400))
            .unwrap();
        song.open_chart(1);
        song.toggle(Tick::new(2880), lane(3), Kind::Tap).unwrap();
        song.open_chart(0);
        let (point, after, hold_end) = (
            seconds(&song, 1920),
            seconds(&song, 2880),
            seconds(&song, 2400),
        );

        song.set_bpm(tempo(150.0)).unwrap();

        assert_eq!(song.bpm(), Some((Tick::new(0), tempo(150.0))));
        assert!((seconds(&song, 2400) - point).abs() < 1e-9);
        assert!((seconds(&song, 3360) - after).abs() < 1e-9);
        assert!((seconds(&song, 2880) - hold_end).abs() < 1e-9);
        assert!(song.starts_at(Tick::new(960), lane(0)));
        assert!(song.starts_at(Tick::new(3360), lane(1)));
        let hold = &song.chart().unwrap().objects[1];
        assert_eq!(hold.tick, Tick::new(1440));
        assert_eq!(hold.shape.end(hold.tick), Some(Tick::new(2880)));
        song.open_chart(1);
        assert!(song.starts_at(Tick::new(3360), lane(3)));
        song.open_chart(0);
        song.undo().unwrap();
        assert!(song.starts_at(Tick::new(2880), lane(1)));
        song.open_chart(1);
        assert!(song.starts_at(Tick::new(2880), lane(3)));
    }

    #[test]
    fn a_bpm_keeps_every_note_where_it_sounds_by_default() {
        let events = json!([bpm_event(0, 120.0), bpm_event(1920, 90.0)]);
        let mut song = timed(&events, None);
        let stop = json!({ "tick": 480, "kind": { "stop": 0.25 } });
        song.replace_timing(
            serde_json::from_value(json!({
                "anchors": [ { "tick": 0, "seconds": 0.0 }, { "tick": 1920, "seconds": 1.25 } ],
                "tempo": 90.0,
                "events": [stop]
            }))
            .unwrap(),
        )
        .unwrap();
        for (tick, lane_index) in [(960, 0), (2880, 1)] {
            song.toggle(Tick::new(tick), lane(lane_index), Kind::Tap)
                .unwrap();
        }
        let (inside, after) = (seconds(&song, 960), seconds(&song, 2880));
        let (stop_at, point) = (seconds(&song, 480), seconds(&song, 1920));

        song.set_bpm(tempo(150.0)).unwrap();

        assert_eq!(song.bpm(), Some((Tick::new(0), tempo(150.0))));
        let shifted_events = &song.document().document().timing.events;
        assert_eq!(shifted_events[0].tick, Tick::new(600));
        assert!((seconds(&song, 600) - stop_at).abs() < 1e-9);
        assert!((seconds(&song, 1200) - inside).abs() < 1e-9);
        assert!((seconds(&song, 2400) - point).abs() < 1e-9);
        assert!((seconds(&song, 3360) - after).abs() < 1e-9);
        assert!(song.starts_at(Tick::new(1200), lane(0)));
        assert!(song.starts_at(Tick::new(3360), lane(1)));
    }

    #[test]
    fn the_last_bpm_keeps_the_notes_after_it_in_time_or_on_their_beats() {
        let mut song = timed(&json!([bpm_event(0, 120.0)]), None);
        song.toggle(Tick::new(1920), lane(0), Kind::Tap).unwrap();
        song.stretch(lane(0), Tick::new(1920), Tick::new(2880))
            .unwrap();
        let (start, end) = (seconds(&song, 1920), seconds(&song, 2880));
        song.set_bpm(tempo(180.0)).unwrap();
        let hold = &song.chart().unwrap().objects[0];
        assert_eq!(hold.tick, Tick::new(2880));
        assert_eq!(hold.shape.end(hold.tick), Some(Tick::new(4320)));
        assert!((seconds(&song, 2880) - start).abs() < 1e-9);
        assert!((seconds(&song, 4320) - end).abs() < 1e-9);

        song.undo().unwrap();
        song.keep_notes(KeepNotes::OnBeats);
        song.set_bpm(tempo(180.0)).unwrap();
        assert!(song.starts_at(Tick::new(1920), lane(0)));
    }

    #[test]
    fn setting_the_bpm_already_in_effect_there_changes_nothing() {
        let mut song = timed(&json!([bpm_event(0, 120.0)]), None);
        song.set_bpm(tempo(120.0)).unwrap();
        assert!(!song.can_undo());
    }

    #[test]
    fn a_bpm_change_added_at_the_cursor_keeps_the_tempo_until_it_is_set() {
        let mut song = timed(&json!([bpm_event(0, 120.0)]), None);
        song.step(Step::Later);
        song.add_bpm_change().unwrap();
        let split = json!([bpm_event(0, 120.0), bpm_event(240, 120.0)]);
        assert_eq!(song_events(&song), split);
        song.add_bpm_change().unwrap();
        assert_eq!(song_events(&song), split);
        song.set_bpm(tempo(150.0)).unwrap();
        let set = json!([bpm_event(0, 120.0), bpm_event(240, 150.0)]);
        assert_eq!(song_events(&song), set);
    }

    #[test]
    fn the_bpm_change_in_effect_can_be_removed_but_not_the_first() {
        let mut song = timed(&json!([bpm_event(0, 120.0), bpm_event(480, 150.0)]), None);
        for _ in 0..3 {
            song.step(Step::Later);
        }
        assert_eq!(song.cursor(), Tick::new(720));
        song.remove_bpm().unwrap();
        assert_eq!(song_events(&song), json!([bpm_event(0, 120.0)]));
        assert!(matches!(song.remove_bpm(), Err(Refusal::FirstTempo)));
        assert_eq!(song_events(&song), json!([bpm_event(0, 120.0)]));
    }

    #[test]
    fn a_timing_put_in_place_replaces_the_offset_and_events_in_one_step() {
        let events = json!([bpm_event(0, 120.0)]);
        let mut song = timed(&events, None);
        let new: charter_core::Timing = serde_json::from_value(json!({
            "anchors": [
                { "tick": 0, "seconds": 0.5 },
                { "tick": 3840, "seconds": 1.653_321 }
            ],
            "tempo": 150.0,
            "events": []
        }))
        .unwrap();
        song.replace_timing(new.clone()).unwrap();
        assert_eq!(song.document().document().timing, new);
        song.replace_timing(new).unwrap();
        song.undo().unwrap();
        assert_eq!(song_events(&song), events);
        assert!(!song.can_undo());
        let bad: charter_core::Timing =
            serde_json::from_value(json!({ "anchors": [], "tempo": 120.0, "events": [] })).unwrap();
        assert!(song.replace_timing(bad).is_err());
    }

    #[test]
    fn a_chart_with_its_own_timing_takes_the_bpm_itself() {
        let mut song = timed(
            &json!([bpm_event(0, 120.0)]),
            Some(&json!([bpm_event(0, 100.0)])),
        );
        song.open_chart(1);
        assert_eq!(song.bpm(), Some((Tick::new(0), tempo(100.0))));
        song.set_bpm(tempo(200.0)).unwrap();
        assert_eq!(song_events(&song), json!([bpm_event(0, 120.0)]));
        let own = &song.document().document().charts[1].timing;
        assert_eq!(own.as_ref().unwrap().tempo, tempo(200.0));
    }

    #[test]
    fn a_read_only_song_refuses_tempo_changes() {
        let document = timed(&json!([bpm_event(0, 120.0)]), None)
            .document()
            .clone();
        let mut song = Song::new(document, &[]);
        assert!(matches!(song.set_bpm(tempo(90.0)), Err(Refusal::ReadOnly)));
        assert!(matches!(song.remove_bpm(), Err(Refusal::ReadOnly)));
        song.step(Step::Later);
        assert!(matches!(song.add_bpm_change(), Err(Refusal::ReadOnly)));
    }

    #[test]
    fn before_the_first_bpm_change_that_change_is_in_effect() {
        let song = timed(&json!([bpm_event(480, 90.0)]), None);
        assert_eq!(song.cursor(), Tick::new(0));
        assert_eq!(song.bpm(), Some((Tick::new(480), tempo(90.0))));
    }

    #[test]
    fn a_timing_event_moved_lands_in_order_and_undo_puts_it_back() {
        let meter = json!({ "tick": 960, "kind": { "time_signature": [3, 4] } });
        let mut song = timed(
            &json!([bpm_event(0, 120.0), bpm_event(480, 150.0), meter]),
            None,
        );
        song.move_timing_at(Tick::new(480), Tick::new(1920))
            .unwrap();
        assert_eq!(
            song_events(&song),
            json!([bpm_event(0, 480.0), meter, bpm_event(1920, 150.0)])
        );
        song.undo().unwrap();
        assert_eq!(song_events(&song)[1], bpm_event(480, 150.0));
    }

    #[test]
    fn every_timing_event_at_a_tick_moves_together_as_one_step() {
        let meter = |tick: i32| json!({ "tick": tick, "kind": { "time_signature": [3, 4] } });
        let events = json!([bpm_event(0, 120.0), meter(960), bpm_event(960, 150.0)]);
        let mut song = timed(&events, None);
        song.move_timing_at(Tick::new(960), Tick::new(1920))
            .unwrap();
        assert_eq!(
            song_events(&song),
            json!([bpm_event(0, 240.0), meter(1920), bpm_event(1920, 150.0)])
        );
        song.undo().unwrap();
        assert_eq!(song_events(&song), events);
    }

    #[test]
    fn a_bpm_change_moved_onto_another_is_refused() {
        let mut song = timed(&json!([bpm_event(0, 120.0), bpm_event(480, 150.0)]), None);
        assert!(song.move_timing_at(Tick::new(480), Tick::new(0)).is_err());
        assert_eq!(song_events(&song)[1], bpm_event(480, 150.0));
    }

    #[test]
    fn moving_timing_to_where_it_is_or_from_where_there_is_none_changes_nothing() {
        let mut song = timed(&json!([bpm_event(0, 120.0)]), None);
        song.move_timing_at(Tick::new(0), Tick::new(0)).unwrap();
        song.move_timing_at(Tick::new(240), Tick::new(480)).unwrap();
        assert!(!song.can_undo());
    }

    #[test]
    fn a_chart_with_its_own_timing_moves_its_own_events() {
        let own = json!([bpm_event(0, 100.0), bpm_event(480, 90.0)]);
        let mut song = timed(&json!([bpm_event(0, 120.0)]), Some(&own));
        song.open_chart(1);
        song.move_timing_at(Tick::new(480), Tick::new(960)).unwrap();
        assert_eq!(song_events(&song), json!([bpm_event(0, 120.0)]));
        let chart = &song.document().document().charts[1];
        assert_eq!(
            chart.timing.as_ref().unwrap().anchors[1].tick,
            Tick::new(960)
        );
    }

    fn offset(song: &Song) -> Value {
        serde_json::to_value(song.document().timing().time_at(Tick::new(0)).seconds()).unwrap()
    }

    #[test]
    fn the_offset_moves_tick_zero_in_the_audio_as_one_step() {
        let mut song = timed(&json!([bpm_event(0, 120.0)]), None);
        song.set_offset(charter_core::Origin::try_from(1.25).unwrap())
            .unwrap();
        assert_eq!(offset(&song), json!(1.25));
        song.undo().unwrap();
        assert_eq!(offset(&song), json!(0.0));
    }

    #[test]
    fn a_chart_with_its_own_timing_takes_the_offset_itself() {
        let own = json!([bpm_event(0, 100.0)]);
        let mut song = timed(&json!([bpm_event(0, 120.0)]), Some(&own));
        song.open_chart(1);
        song.set_offset(charter_core::Origin::try_from(-0.5).unwrap())
            .unwrap();
        assert_eq!(offset(&song), json!(0.0));
        let chart = &song.document().document().charts[1];
        let own_seconds =
            serde_json::to_value(chart.timing.as_ref().unwrap().anchors[0].seconds).unwrap();
        assert_eq!(own_seconds, json!(-0.5));
    }
}
