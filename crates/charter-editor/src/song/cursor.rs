//! Cursor movement and snap choices.

#[cfg_attr(
    not(test),
    expect(
        clippy::wildcard_imports,
        reason = "song submodules share one private implementation vocabulary"
    )
)]
use super::*;

impl Song {
    /// Tick at which edits and playback begin.
    #[must_use]
    pub const fn cursor(&self) -> Tick {
        self.cursor
    }

    /// Current snap resolution in notes per whole note.
    #[must_use]
    pub const fn snap(&self) -> NonZeroU16 {
        self.snap
    }
}

impl Song {
    /// Puts the cursor on `tick`, such as a marker's.
    pub const fn jump_to(&mut self, tick: Tick) {
        self.cursor = tick;
    }
}

impl Song {
    /// Moves the cursor to the next line of the snap's grid.
    pub fn step(&mut self, step: Step) {
        let Some(length) = self.length_of(self.snap) else {
            return;
        };
        let moved = match step {
            Step::Later => self.cursor.next_on(length),
            Step::Earlier => self.cursor.previous_on(length),
        };
        if let Some(tick) = moved {
            self.cursor = tick;
        }
    }

    /// Moves the cursor to the start of the next measure, or back to the start of this one or
    /// the one before.
    pub fn step_measure(&mut self, step: Step) {
        let moved = {
            let timing = self.timing();
            let mut measures = timing.measures_from(self.cursor);
            let holding = measures.next().map(|measure| measure.start);
            match step {
                Step::Later => measures.next().map(|measure| measure.start),
                Step::Earlier => holding.filter(|start| *start < self.cursor).or_else(|| {
                    let before = self.cursor.previous_on(Ticks::new(1)?)?;
                    timing
                        .measures_from(before)
                        .next()
                        .map(|measure| measure.start)
                }),
            }
        };
        if let Some(tick) = moved {
            self.cursor = tick;
        }
    }

    /// Moves to the next finer snap supported by this song's tick resolution.
    pub fn finer_snap(&mut self) {
        let finer = self.snaps().find(|snap| *snap > self.snap);
        self.snap = finer.unwrap_or(self.snap);
    }

    /// Sets what a BPM set from now on keeps of the notes it times.
    pub const fn keep_notes(&mut self, keep: KeepNotes) {
        self.keep = keep;
    }

    /// Snaps to `snap` if it is one of [`SNAPS`] and a whole number of ticks at the song's
    /// resolution, and otherwise keeps the snap there is.
    pub fn set_snap(&mut self, snap: NonZeroU16) {
        if self.snaps().any(|held| held == snap) {
            self.snap = snap;
        }
    }

    /// Moves to the next coarser snap supported by this song's tick resolution.
    pub fn coarser_snap(&mut self) {
        let coarser = self.snaps().filter(|snap| *snap < self.snap).last();
        self.snap = coarser.unwrap_or(self.snap);
    }

    /// The snaps whose notes are a whole number of ticks at this song's resolution.
    pub(super) fn snaps(&self) -> impl Iterator<Item = NonZeroU16> + '_ {
        SNAPS
            .into_iter()
            .filter_map(NonZeroU16::new)
            .filter(|snap| self.length_of(*snap).is_some())
    }

    /// Whole ticks in a note at `snap`, if the song's resolution supports it.
    pub(super) fn length_of(&self, snap: NonZeroU16) -> Option<Ticks> {
        self.document.document().ticks_per_beat.note_length(snap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo;
    use crate::song::tests::*;
    use serde_json::json;

    #[test]
    fn a_song_opens_on_its_first_chart_with_the_cursor_on_tick_zero() {
        let bytes = demo::song().to_json().unwrap();
        let song = Song::open(&bytes, &[]).unwrap();
        assert_eq!(song.chart().unwrap().name, "Easy");
        assert_eq!(song.cursor(), at(0));
    }

    #[test]
    fn switching_charts_keeps_the_cursor_on_its_tick() {
        let mut song = Song::new(demo::song(), &[]);
        song.step(Step::Later);
        song.open_chart(chart_index(1));
        assert_eq!(song.chart().unwrap().name, "Hard");
        assert_eq!(song.cursor(), at(240));
    }

    #[test]
    fn the_cursor_steps_a_sixteenth_either_way() {
        let mut song = Song::new(demo::song(), &[]);
        song.step(Step::Later);
        assert_eq!(song.cursor(), at(240));
        song.step(Step::Earlier);
        song.step(Step::Earlier);
        assert_eq!(song.cursor(), at(-240));
    }

    #[test]
    fn a_measure_step_lands_on_the_next_or_previous_measure_line() {
        let mut song = Song::new(demo::song(), &[]);
        song.step_measure(Step::Later);
        assert_eq!(song.cursor(), at(3840));
        song.step(Step::Later);
        song.step_measure(Step::Earlier);
        assert_eq!(song.cursor(), at(3840));
        song.step_measure(Step::Earlier);
        assert_eq!(song.cursor(), at(0));
    }

    #[test]
    fn snaps_walk_the_arrowvortex_list_and_stop_at_its_ends() {
        let mut song = Song::new(demo::song(), &[]);
        assert_eq!(song.snap(), note(16));
        song.finer_snap();
        assert_eq!(song.snap(), note(24));
        for _ in 0..20 {
            song.finer_snap();
        }
        assert_eq!(song.snap(), note(192));
        for _ in 0..20 {
            song.coarser_snap();
        }
        assert_eq!(song.snap(), note(4));
    }

    #[test]
    fn a_chosen_snap_stands_only_if_the_menu_has_it_and_the_resolution_holds_it() {
        let mut original_song = Song::new(document(json!([])), &[]);
        original_song.set_snap(note(12));
        assert_eq!(original_song.snap(), note(12));
        original_song.set_snap(note(7));
        assert_eq!(original_song.snap(), note(12));
        let coarse = json!({
            "format_version": 0,
            "ticks_per_beat": 100,
            "ruleset": { "id": "test", "version": "1" },
            "metadata": { "title": "t", "artist": "a", "audio": "song.ogg" },
            "timing": { "offset_seconds": 0.0, "events": [ { "tick": 0, "kind": { "bpm": 120.0 } } ] }
        });
        let mut coarse_song = Song::open(&serde_json::to_vec(&coarse).unwrap(), &[]).unwrap();
        coarse_song.set_snap(note(12));
        assert_eq!(coarse_song.snap(), note(16));
    }

    #[test]
    fn a_snap_the_resolution_cannot_hold_is_passed_over() {
        let coarse = json!({
            "format_version": 0,
            "ticks_per_beat": 100,
            "ruleset": { "id": "test", "version": "1" },
            "metadata": { "title": "t", "artist": "a", "audio": "song.ogg" },
            "timing": { "offset_seconds": 0.0, "events": [ { "tick": 0, "kind": { "bpm": 120.0 } } ] }
        });
        let mut song = Song::open(&serde_json::to_vec(&coarse).unwrap(), &[]).unwrap();
        assert_eq!(song.snap(), note(16));
        song.coarser_snap();
        assert_eq!(song.snap(), note(8));
        song.finer_snap();
        song.finer_snap();
        assert_eq!(song.snap(), note(16));
    }

    #[test]
    fn a_song_whose_resolution_cannot_hold_the_default_snap_opens_on_its_first_whole_snap() {
        let value = json!({
            "format_version": 0,
            "ticks_per_beat": 1,
            "ruleset": { "id": "test", "version": "1" },
            "metadata": { "title": "t", "artist": "a", "audio": "song.ogg" },
            "timing": { "offset_seconds": 0.0, "events": [bpm_event(0, 120.0)] }
        });
        let song = Song::open(&serde_json::to_vec(&value).unwrap(), &[]).unwrap();
        assert_eq!(song.snap(), note(4));
    }
}

#[cfg(kani)]
mod proofs {
    use super::*;

    #[kani::proof]
    fn successive_snap_menu_entries_are_finer_and_whole_steps_never_zero() {
        let index: usize = kani::any();
        let resolution: std::num::NonZeroU16 = kani::any();
        kani::assume(index < SNAPS.len() - 1);
        let first = NonZeroU16::new(SNAPS[index]).expect("snap is nonzero");
        let next = NonZeroU16::new(SNAPS[index + 1]).expect("snap is nonzero");
        assert!(first < next);
        let resolution = TicksPerBeat::new(resolution.get()).expect("nonzero resolution");
        if let Some(length) = resolution.note_length(first) {
            assert!(u32::from(length) > 0);
        }
        if let Some(length) = resolution.note_length(next) {
            assert!(u32::from(length) > 0);
        }
    }
}
