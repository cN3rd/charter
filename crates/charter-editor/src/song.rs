//! An open song: its document, which chart is open, and where the cursor is.

use std::num::NonZeroU16;

use charter_core::{Chart, Error, Tick, Ticks, Timing, Valid, ValidDocument};

/// Which way the cursor moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Earlier,
    Later,
}

/// The snaps in ArrowVortex's menu, coarsest first.
const SNAPS: [u16; 10] = [4, 8, 12, 16, 24, 32, 48, 64, 96, 192];

/// The snap a song opens on, when its resolution holds it.
const DEFAULT_SNAP: u16 = 16;

/// A song open in the editor.
#[derive(Debug)]
pub struct Song {
    document: ValidDocument,
    chart: usize,
    cursor: Tick,
    snap: NonZeroU16,
}

impl Song {
    /// Reads a song from a document's bytes.
    ///
    /// # Errors
    ///
    /// Returns whatever keeps the bytes from being a valid document.
    pub fn open(bytes: &[u8]) -> Result<Self, Error> {
        Ok(Self::new(ValidDocument::from_json(bytes)?))
    }

    /// Opens `document` on its first chart, with the cursor on tick 0.
    #[must_use]
    pub fn new(document: ValidDocument) -> Self {
        let mut song = Self {
            document,
            chart: 0,
            cursor: Tick::new(0),
            snap: NonZeroU16::MIN,
        };
        let snaps: Vec<_> = song.snaps().collect();
        song.snap = snaps
            .iter()
            .copied()
            .find(|snap| snap.get() == DEFAULT_SNAP)
            .or_else(|| snaps.first().copied())
            .unwrap_or(NonZeroU16::MIN);
        song
    }

    #[must_use]
    pub fn document(&self) -> &ValidDocument {
        &self.document
    }

    #[must_use]
    pub fn chart_index(&self) -> usize {
        self.chart
    }

    /// The open chart, unless the song has none.
    #[must_use]
    pub fn chart(&self) -> Option<Valid<'_, Chart>> {
        self.document.charts().nth(self.chart)
    }

    /// The timing the open chart resolves through, or the song's when there is no chart.
    #[must_use]
    pub fn timing(&self) -> Valid<'_, Timing> {
        self.chart()
            .map_or_else(|| self.document.timing(), |chart| chart.timing())
    }

    /// Opens another chart, keeping the cursor on its tick so the two compare at one spot.
    pub fn open_chart(&mut self, index: usize) {
        if index < self.document.document().charts.len() {
            self.chart = index;
        }
    }

    #[must_use]
    pub fn cursor(&self) -> Tick {
        self.cursor
    }

    #[must_use]
    pub fn snap(&self) -> NonZeroU16 {
        self.snap
    }

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

    pub fn finer_snap(&mut self) {
        let finer = self.snaps().find(|snap| *snap > self.snap);
        self.snap = finer.unwrap_or(self.snap);
    }

    pub fn coarser_snap(&mut self) {
        let coarser = self.snaps().filter(|snap| *snap < self.snap).last();
        self.snap = coarser.unwrap_or(self.snap);
    }

    /// The snaps whose notes are a whole number of ticks at this song's resolution.
    fn snaps(&self) -> impl Iterator<Item = NonZeroU16> + '_ {
        SNAPS
            .into_iter()
            .filter_map(NonZeroU16::new)
            .filter(|snap| self.length_of(*snap).is_some())
    }

    fn length_of(&self, snap: NonZeroU16) -> Option<Ticks> {
        self.document.document().ticks_per_beat.note_length(snap)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::demo;

    fn note(n: u16) -> NonZeroU16 {
        NonZeroU16::new(n).unwrap()
    }

    fn at(tick: i32) -> Tick {
        Tick::new(tick)
    }

    #[test]
    fn a_song_opens_on_its_first_chart_with_the_cursor_on_tick_zero() {
        let bytes = demo::song().to_json().unwrap();
        let song = Song::open(&bytes).unwrap();
        assert_eq!(song.chart().unwrap().name, "Easy");
        assert_eq!(song.cursor(), at(0));
    }

    #[test]
    fn switching_charts_keeps_the_cursor_on_its_tick() {
        let mut song = Song::new(demo::song());
        song.step(Step::Later);
        song.open_chart(1);
        assert_eq!(song.chart().unwrap().name, "Hard");
        assert_eq!(song.cursor(), at(240));
    }

    #[test]
    fn the_cursor_steps_a_sixteenth_either_way() {
        let mut song = Song::new(demo::song());
        song.step(Step::Later);
        assert_eq!(song.cursor(), at(240));
        song.step(Step::Earlier);
        song.step(Step::Earlier);
        assert_eq!(song.cursor(), at(-240));
    }

    #[test]
    fn a_measure_step_lands_on_the_next_or_previous_measure_line() {
        let mut song = Song::new(demo::song());
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
        let mut song = Song::new(demo::song());
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
    fn a_snap_the_resolution_cannot_hold_is_passed_over() {
        let coarse = json!({
            "format_version": 0,
            "ticks_per_beat": 100,
            "ruleset": { "id": "test", "version": "1" },
            "metadata": { "title": "t", "artist": "a", "audio": "song.ogg" },
            "timing": { "offset_seconds": 0.0, "events": [ { "tick": 0, "kind": { "bpm": 120.0 } } ] }
        });
        let mut song = Song::open(&serde_json::to_vec(&coarse).unwrap()).unwrap();
        assert_eq!(song.snap(), note(16));
        song.coarser_snap();
        assert_eq!(song.snap(), note(8));
        song.finer_snap();
        song.finer_snap();
        assert_eq!(song.snap(), note(16));
    }
}
