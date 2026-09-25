//! An open song: its document, which chart is open, where the cursor is, and the edits made to
//! it.

use std::collections::BTreeSet;
use std::num::NonZeroU16;
use std::ops::RangeInclusive;

use charter_core::{
    Chart, Error, Extensions, Kind, LaneCount, LaneIndex, LaneSpan, Object, Position,
    RulesetPlugin, Shape, Tick, Ticks, Timing, Valid, ValidDocument,
};

use crate::edit::{Edit, Op};

/// Which way the cursor moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Earlier,
    Later,
}

/// What a lane key did at the cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggled {
    Placed,
    Removed,
}

/// A lane note of the open chart, by the tick it starts on and the first lane it covers, which
/// no other note starting on that tick shares.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NoteAt {
    pub tick: Tick,
    pub lane: LaneIndex,
}

/// How notes being selected meet the selection there is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selecting {
    Replace,
    Add,
    Subtract,
}

/// Why an edit did not happen. The song stays as it was.
#[derive(Debug, thiserror::Error)]
pub enum Refusal {
    #[error("the song is read only")]
    ReadOnly,
    #[error("the song has no chart to edit")]
    NoChart,
    /// The edit breaks a rule of the format.
    #[error(transparent)]
    Document(#[from] Error),
    /// The song's game cannot represent the edit.
    #[error(transparent)]
    Game(Box<dyn std::error::Error + Send + Sync>),
}

/// The snaps in ArrowVortex's menu, coarsest first.
const SNAPS: [u16; 10] = [4, 8, 12, 16, 24, 32, 48, 64, 96, 192];

/// The snap a song opens on, when its resolution holds it.
const DEFAULT_SNAP: u16 = 16;

/// A song open in the editor.
#[derive(Debug)]
pub struct Song {
    document: ValidDocument,
    rules: Rules,
    chart: usize,
    cursor: Tick,
    snap: NonZeroU16,
    /// Every edit, oldest first. The first `done` are in effect; the rest were undone.
    history: Vec<Change>,
    done: usize,
    /// Selected notes of the open chart, each one there.
    selection: BTreeSet<NoteAt>,
}

/// Whether the song's game can check edits to it.
#[derive(Debug)]
enum Rules {
    /// The plugin for the song's ruleset, which accepted the song.
    Checked(&'static dyn RulesetPlugin),
    /// Why the song cannot be edited.
    ReadOnly(String),
}

impl Rules {
    /// The plugin among `plugins` for the ruleset of `document`, if there is one and it accepts
    /// the document.
    fn of(document: &ValidDocument, plugins: &[&'static dyn RulesetPlugin]) -> Self {
        let id = &document.document().ruleset.id;
        let Some(plugin) = plugins.iter().find(|plugin| plugin.id() == id.as_str()) else {
            return Self::ReadOnly(format!("charter has no {id} plugin"));
        };
        match plugin.check(document) {
            Ok(()) => Self::Checked(*plugin),
            Err(why) => Self::ReadOnly(format!("the {id} plugin cannot read this song: {why}")),
        }
    }
}

/// An edit and where it was made, which undo and redo return to.
#[derive(Debug)]
struct Change {
    chart: usize,
    cursor: Tick,
    edit: Edit,
}

impl Song {
    /// Reads a song from a document's bytes.
    ///
    /// # Errors
    ///
    /// Returns whatever keeps the bytes from being a valid document.
    pub fn open(bytes: &[u8], plugins: &[&'static dyn RulesetPlugin]) -> Result<Self, Error> {
        Ok(Self::new(ValidDocument::from_json(bytes)?, plugins))
    }

    /// Opens `document` on its first chart, with the cursor on tick 0. It can be edited if one
    /// of `plugins` is for its ruleset and accepts it.
    #[must_use]
    pub fn new(document: ValidDocument, plugins: &[&'static dyn RulesetPlugin]) -> Self {
        let mut song = Self {
            rules: Rules::of(&document, plugins),
            document,
            chart: 0,
            cursor: Tick::new(0),
            snap: NonZeroU16::MIN,
            history: Vec::new(),
            done: 0,
            selection: BTreeSet::new(),
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

    /// Why the song cannot be edited, if it cannot.
    #[must_use]
    pub fn read_only(&self) -> Option<&str> {
        match &self.rules {
            Rules::Checked(_) => None,
            Rules::ReadOnly(why) => Some(why),
        }
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
        if index < self.document.document().charts.len() && index != self.chart {
            self.chart = index;
            self.selection.clear();
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

    /// Places an instant note of `kind` on `lane` at `tick`, or removes the object that starts
    /// there on that lane.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song or one without charts, and when the edit would break a rule of
    /// the format or of the song's game.
    pub fn toggle(&mut self, tick: Tick, lane: LaneIndex, kind: Kind) -> Result<Toggled, Refusal> {
        let chart = self.chart().ok_or(Refusal::NoChart)?;
        let (edit, toggled) = if let Some((index, object)) = self.starting(tick, lane) {
            let object = object.clone();
            (Edit(vec![Op::Remove { index, object }]), Toggled::Removed)
        } else {
            let index = chart.objects.partition_point(|object| object.tick <= tick);
            let object = note(tick, lane, kind);
            (Edit(vec![Op::Insert { index, object }]), Toggled::Placed)
        };
        self.document = self.edited(self.chart, &edit)?;
        self.record(edit);
        Ok(toggled)
    }

    /// Makes the tap or hold that starts on `lane` at `from` end at `to`: a hold when `to` is
    /// later, a tap otherwise. When the edit before placed that note, the two undo as one step.
    /// Does nothing when no tap or hold starts there.
    ///
    /// # Errors
    ///
    /// Refuses when the note's new length would break a rule of the format or of the song's
    /// game.
    pub fn stretch(&mut self, lane: LaneIndex, from: Tick, to: Tick) -> Result<(), Refusal> {
        let Some((index, old)) = self.starting(from, lane) else {
            return Ok(());
        };
        if !matches!(old.kind, Kind::Tap | Kind::Hold) {
            return Ok(());
        }
        let mut new = old.clone();
        (new.kind, new.shape) = match from.ticks_until(to) {
            Some(duration) => (Kind::Hold, Shape::Sustain { duration }),
            None => (Kind::Tap, Shape::Instant),
        };
        if new == *old {
            return Ok(());
        }
        let old = old.clone();
        let edit = Edit(vec![Op::Replace { index, old, new }]);
        self.document = self.edited(self.chart, &edit)?;
        self.record(edit);
        Ok(())
    }

    /// The object of the open chart that starts at `tick` and covers `lane`, and where it is.
    fn starting(&self, tick: Tick, lane: LaneIndex) -> Option<(usize, &Object)> {
        let objects = &self.document.document().charts.get(self.chart)?.objects;
        let from = objects.partition_point(|object| object.tick < tick);
        let to = objects.partition_point(|object| object.tick <= tick);
        let key = LaneSpan::new(lane, LaneCount::ONE);
        let here = objects.get(from..to).unwrap_or_default();
        here.iter().enumerate().find_map(|(offset, object)| {
            let covers = object
                .position
                .lanes()
                .is_some_and(|span| span.overlaps(key));
            covers.then_some((from + offset, object))
        })
    }

    /// Keeps `edit`, made to the open chart, as the newest step of the history, dropping what
    /// could have been redone. An edit that only changes what the step before placed joins that
    /// step.
    fn record(&mut self, edit: Edit) {
        self.history.truncate(self.done);
        if let Some(last) = self.history.last_mut()
            && last.chart == self.chart
            && last.edit.absorb(&edit)
        {
            self.prune_selection();
            return;
        }
        self.history.push(Change {
            chart: self.chart,
            cursor: self.cursor,
            edit,
        });
        self.done = self.history.len();
        self.prune_selection();
    }

    /// The document with `edit` made to chart `chart`, if the format and the game allow it.
    fn edited(&self, chart: usize, edit: &Edit) -> Result<ValidDocument, Refusal> {
        let Rules::Checked(plugin) = self.rules else {
            return Err(Refusal::ReadOnly);
        };
        // ponytail: every edit clones, validates and plugin-checks the whole document, about 3 ms
        // at 10,000 notes in a release build; check only the edited chart once live recording or
        // bulk edits need it faster.
        let mut document = self.document.document().clone();
        let objects = &mut document
            .charts
            .get_mut(chart)
            .ok_or(Refusal::NoChart)?
            .objects;
        edit.apply(objects);
        let edited = ValidDocument::new(document)?;
        plugin.check(&edited).map_err(Refusal::Game)?;
        Ok(edited)
    }

    /// Takes back the newest edit still in effect, returning to the chart and cursor it was made
    /// at. Does nothing when no edit is left.
    ///
    /// # Errors
    ///
    /// Refuses when the song as it was no longer passes its plugin, which cannot happen while
    /// the plugin's rules stay the same.
    pub fn undo(&mut self) -> Result<(), Refusal> {
        let Some(last) = self.done.checked_sub(1) else {
            return Ok(());
        };
        let Some(change) = self.history.get(last) else {
            return Ok(());
        };
        self.document = self.edited(change.chart, &change.edit.inverse())?;
        let (chart, cursor) = (change.chart, change.cursor);
        self.done = last;
        self.return_to(chart, cursor);
        Ok(())
    }

    /// Makes the oldest undone edit again, at the chart and cursor it was made at. Does nothing
    /// when none was undone since the last edit.
    ///
    /// # Errors
    ///
    /// Refuses when the edit no longer passes the song's plugin, which cannot happen while the
    /// plugin's rules stay the same.
    pub fn redo(&mut self) -> Result<(), Refusal> {
        let Some(change) = self.history.get(self.done) else {
            return Ok(());
        };
        self.document = self.edited(change.chart, &change.edit)?;
        let (chart, cursor) = (change.chart, change.cursor);
        self.done += 1;
        self.return_to(chart, cursor);
        Ok(())
    }

    /// The selected notes, earliest first.
    pub fn selection(&self) -> impl Iterator<Item = NoteAt> + '_ {
        self.selection.iter().copied()
    }

    #[must_use]
    pub fn is_selected(&self, object: &Object) -> bool {
        note_at(object).is_some_and(|note| self.selection.contains(&note))
    }

    /// Meets the selection with those of `notes` that the open chart has.
    pub fn select(&mut self, notes: impl IntoIterator<Item = NoteAt>, how: Selecting) {
        let found: Vec<_> = notes.into_iter().filter(|note| self.has(*note)).collect();
        self.meet(found, how);
    }

    /// Meets the selection with the notes of the open chart that start within `ticks` and cover
    /// a lane within `lanes`.
    pub fn select_between(
        &mut self,
        ticks: RangeInclusive<Tick>,
        lanes: RangeInclusive<LaneIndex>,
        how: Selecting,
    ) {
        let (first, last) = (u16::from(*lanes.start()), u16::from(*lanes.end()));
        let width = last
            .checked_sub(first)
            .and_then(|span| LaneCount::new(span + 1));
        let found: Vec<_> = match (width, self.chart()) {
            (Some(width), Some(chart)) => {
                let area = LaneSpan::new(LaneIndex::new(first), width);
                let inside = |object: &&Object| {
                    ticks.contains(&object.tick)
                        && object
                            .position
                            .lanes()
                            .is_some_and(|span| span.overlaps(area))
                };
                chart
                    .objects
                    .iter()
                    .filter(inside)
                    .filter_map(note_at)
                    .collect()
            }
            _ => Vec::new(),
        };
        self.meet(found, how);
    }

    /// Selects every lane note of the open chart.
    pub fn select_all(&mut self) {
        self.selection = self
            .chart()
            .map(|chart| chart.objects.iter().filter_map(note_at).collect())
            .unwrap_or_default();
    }

    pub fn clear_selection(&mut self) {
        self.selection.clear();
    }

    /// Removes every selected note, as one step.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song, and when removing the notes would break a rule of the format
    /// or of the song's game; the notes stay selected.
    pub fn delete_selection(&mut self) -> Result<(), Refusal> {
        let Some(chart) = self.chart() else {
            return Ok(());
        };
        // Removing from the back keeps the index of every note still to remove.
        let removals = chart.objects.iter().enumerate().rev();
        let ops: Vec<_> = removals
            .filter(|(_, object)| self.is_selected(object))
            .map(|(index, object)| Op::Remove {
                index,
                object: object.clone(),
            })
            .collect();
        if ops.is_empty() {
            return Ok(());
        }
        let edit = Edit(ops);
        self.document = self.edited(self.chart, &edit)?;
        self.record(edit);
        Ok(())
    }

    fn meet(&mut self, notes: Vec<NoteAt>, how: Selecting) {
        match how {
            Selecting::Replace => self.selection = notes.into_iter().collect(),
            Selecting::Add => self.selection.extend(notes),
            Selecting::Subtract => {
                for note in &notes {
                    self.selection.remove(note);
                }
            }
        }
    }

    /// Whether the open chart has the note `note` names.
    fn has(&self, note: NoteAt) -> bool {
        self.starting(note.tick, note.lane)
            .is_some_and(|(_, object)| note_at(object) == Some(note))
    }

    /// Drops the selected notes an edit took away.
    fn prune_selection(&mut self) {
        let selection = std::mem::take(&mut self.selection);
        self.selection = selection
            .into_iter()
            .filter(|note| self.has(*note))
            .collect();
    }

    /// Opens `chart` with the cursor on `cursor`, where an undone or redone edit was made.
    fn return_to(&mut self, chart: usize, cursor: Tick) {
        self.open_chart(chart);
        self.cursor = cursor;
        self.prune_selection();
    }

    #[must_use]
    pub fn can_undo(&self) -> bool {
        self.done > 0
    }

    #[must_use]
    pub fn can_redo(&self) -> bool {
        self.done < self.history.len()
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

/// The note an object is, when it sits on lanes.
fn note_at(object: &Object) -> Option<NoteAt> {
    match object.position {
        Position::Lane { index, .. } => Some(NoteAt {
            tick: object.tick,
            lane: index,
        }),
        Position::Plane { .. } => None,
    }
}

/// An instant note of `kind` on `lane` at `tick`.
fn note(tick: Tick, lane: LaneIndex, kind: Kind) -> Object {
    Object {
        tick,
        position: Position::Lane {
            index: lane,
            width: LaneCount::ONE,
        },
        kind,
        shape: Shape::Instant,
        sample: None,
        children: Vec::new(),
        ext: Extensions::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use serde_json::{Value, json};

    use super::*;
    use crate::demo;

    fn note(n: u16) -> NonZeroU16 {
        NonZeroU16::new(n).unwrap()
    }

    fn at(tick: i32) -> Tick {
        Tick::new(tick)
    }

    /// Accepts every document of the `test` ruleset.
    #[derive(Debug)]
    struct Lenient;

    impl RulesetPlugin for Lenient {
        fn id(&self) -> &'static str {
            "test"
        }

        fn check(&self, _: &ValidDocument) -> Result<(), Box<dyn Error + Send + Sync>> {
            Ok(())
        }
    }

    /// Refuses a note on the last of four lanes, which no format rule does.
    #[derive(Debug)]
    struct NoLastLane;

    impl RulesetPlugin for NoLastLane {
        fn id(&self) -> &'static str {
            "test"
        }

        fn check(&self, document: &ValidDocument) -> Result<(), Box<dyn Error + Send + Sync>> {
            let last = Some(LaneSpan::new(LaneIndex::new(3), LaneCount::ONE));
            let on_last = |chart: Valid<'_, Chart>| {
                chart
                    .objects()
                    .any(|object| object.position.lanes() == last)
            };
            if document.charts().any(on_last) {
                return Err("a note on lane 4".into());
            }
            Ok(())
        }
    }

    /// Refuses sustained notes, as a game without holds would.
    #[derive(Debug)]
    struct NoHolds;

    impl RulesetPlugin for NoHolds {
        fn id(&self) -> &'static str {
            "test"
        }

        fn check(&self, document: &ValidDocument) -> Result<(), Box<dyn Error + Send + Sync>> {
            let held = |chart: Valid<'_, Chart>| {
                chart
                    .objects()
                    .any(|object| matches!(object.shape, Shape::Sustain { .. }))
            };
            if document.charts().any(held) {
                return Err("a hold".into());
            }
            Ok(())
        }
    }

    /// A plugin for another game, which has no say over `test` documents.
    #[derive(Debug)]
    struct Elsewhere;

    impl RulesetPlugin for Elsewhere {
        fn id(&self) -> &'static str {
            "sm"
        }

        fn check(&self, _: &ValidDocument) -> Result<(), Box<dyn Error + Send + Sync>> {
            Ok(())
        }
    }

    const EDITABLE: &[&dyn RulesetPlugin] = &[&Elsewhere, &Lenient];

    /// A `test` document with one four-lane chart holding `objects`, then a second, empty one.
    fn document(objects: Value) -> ValidDocument {
        let mut value = json!({
            "format_version": 0,
            "ruleset": { "id": "test", "version": "1" },
            "metadata": { "title": "t", "artist": "a", "audio": "song.ogg" },
            "timing": { "offset_seconds": 0.0, "events": [ { "tick": 0, "kind": { "bpm": 120.0 } } ] },
            "charts": [ { "name": "Easy", "lanes": 4 }, { "name": "Hard", "lanes": 4 } ]
        });
        value["charts"][0]["objects"] = objects;
        ValidDocument::from_json(&serde_json::to_vec(&value).unwrap()).unwrap()
    }

    fn object(tick: i32, lane: u16, kind: &str) -> Value {
        json!({ "tick": tick, "position": { "lane": { "index": lane } }, "kind": kind, "shape": "instant" })
    }

    #[test]
    fn a_song_is_read_only_without_a_plugin_for_its_ruleset() {
        let song = Song::new(document(json!([])), &[&Elsewhere]);
        assert_eq!(song.read_only(), Some("charter has no test plugin"));
    }

    #[test]
    fn a_song_its_plugin_refuses_is_read_only_and_says_why() {
        let song = Song::new(document(json!([object(0, 3, "tap")])), &[&NoLastLane]);
        assert_eq!(
            song.read_only(),
            Some("the test plugin cannot read this song: a note on lane 4")
        );
    }

    #[test]
    fn a_song_its_plugin_accepts_is_editable() {
        let song = Song::new(document(json!([])), EDITABLE);
        assert_eq!(song.read_only(), None);
    }

    fn objects(song: &Song) -> Vec<Value> {
        let chart = song.chart().unwrap();
        chart
            .objects
            .iter()
            .map(|o| serde_json::to_value(o).unwrap())
            .collect()
    }

    fn lane(index: u16) -> LaneIndex {
        LaneIndex::new(index)
    }

    #[test]
    fn a_lane_key_places_a_tap_at_the_cursor_among_the_notes_around_it() {
        let mut song = Song::new(
            document(json!([object(0, 0, "tap"), object(480, 1, "tap")])),
            EDITABLE,
        );
        song.step(Step::Later);
        assert_eq!(
            song.toggle(song.cursor(), lane(2), Kind::Tap).unwrap(),
            Toggled::Placed
        );
        assert_eq!(
            objects(&song),
            [
                object(0, 0, "tap"),
                object(240, 2, "tap"),
                object(480, 1, "tap")
            ]
        );
    }

    #[test]
    fn a_mine_goes_where_a_tap_would() {
        let mut song = Song::new(document(json!([object(0, 0, "tap")])), EDITABLE);
        assert_eq!(
            song.toggle(at(0), lane(1), Kind::Mine).unwrap(),
            Toggled::Placed
        );
        assert_eq!(objects(&song), [object(0, 0, "tap"), object(0, 1, "mine")]);
    }

    #[test]
    fn a_note_placed_away_from_the_cursor_leaves_the_cursor_and_undoes_back_to_it() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        song.toggle(at(480), lane(2), Kind::Tap).unwrap();
        assert_eq!(objects(&song), [object(480, 2, "tap")]);
        assert_eq!(song.cursor(), at(0));
        song.step(Step::Later);
        song.undo().unwrap();
        assert_eq!(song.cursor(), at(0));
    }

    fn hold(tick: i32, lane: u16, duration: u32) -> Value {
        json!({ "tick": tick, "position": { "lane": { "index": lane } }, "kind": "hold",
                "shape": { "sustain": { "duration": duration } } })
    }

    #[test]
    fn stretching_a_tap_past_its_tick_makes_it_a_hold_to_there() {
        let mut song = Song::new(document(json!([object(0, 0, "tap")])), EDITABLE);
        song.stretch(lane(0), at(0), at(480)).unwrap();
        assert_eq!(objects(&song), [hold(0, 0, 480)]);
    }

    #[test]
    fn stretching_a_hold_back_to_its_own_tick_makes_it_a_tap_again() {
        let mut song = Song::new(document(json!([hold(0, 0, 480)])), EDITABLE);
        song.stretch(lane(0), at(0), at(0)).unwrap();
        assert_eq!(objects(&song), [object(0, 0, "tap")]);
    }

    #[test]
    fn a_note_placed_and_stretched_undoes_in_one_step() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        song.toggle(at(0), lane(0), Kind::Tap).unwrap();
        song.stretch(lane(0), at(0), at(240)).unwrap();
        song.stretch(lane(0), at(0), at(480)).unwrap();
        song.undo().unwrap();
        assert_eq!(objects(&song), Vec::<Value>::new());
        assert!(!song.can_undo());
        song.redo().unwrap();
        assert_eq!(objects(&song), [hold(0, 0, 480)]);
    }

    #[test]
    fn stretching_an_older_note_is_a_step_of_its_own() {
        let mut song = Song::new(document(json!([object(0, 0, "tap")])), EDITABLE);
        song.toggle(at(240), lane(1), Kind::Tap).unwrap();
        song.stretch(lane(0), at(0), at(480)).unwrap();
        song.undo().unwrap();
        assert_eq!(objects(&song), [object(0, 0, "tap"), object(240, 1, "tap")]);
    }

    #[test]
    fn a_stretch_the_game_refuses_leaves_the_note_as_it_was() {
        let mut song = Song::new(document(json!([object(0, 0, "tap")])), &[&NoHolds]);
        let refusal = song.stretch(lane(0), at(0), at(480)).unwrap_err();
        assert_eq!(refusal.to_string(), "a hold");
        assert_eq!(objects(&song), [object(0, 0, "tap")]);
    }

    #[test]
    fn only_taps_and_holds_stretch() {
        let mut song = Song::new(document(json!([object(0, 0, "mine")])), EDITABLE);
        song.stretch(lane(0), at(0), at(480)).unwrap();
        assert_eq!(objects(&song), [object(0, 0, "mine")]);
        assert!(!song.can_undo());
    }

    #[test]
    fn a_lane_key_removes_whatever_starts_at_the_cursor_on_its_lane() {
        let wide = json!({ "tick": 0, "position": { "lane": { "index": 2, "width": 2 } },
                           "kind": "hold", "shape": { "sustain": { "duration": 480 } } });
        let mut song = Song::new(document(json!([object(0, 0, "mine"), wide])), EDITABLE);
        assert_eq!(
            song.toggle(song.cursor(), lane(0), Kind::Tap).unwrap(),
            Toggled::Removed
        );
        assert_eq!(
            song.toggle(song.cursor(), lane(3), Kind::Tap).unwrap(),
            Toggled::Removed
        );
        assert_eq!(objects(&song), Vec::<Value>::new());
    }

    #[test]
    fn a_hold_that_started_earlier_leaves_the_lane_free_at_the_cursor() {
        let hold = json!({ "tick": 0, "position": { "lane": { "index": 0 } },
                           "kind": "hold", "shape": { "sustain": { "duration": 960 } } });
        let mut song = Song::new(document(json!([hold.clone()])), EDITABLE);
        song.step(Step::Later);
        assert_eq!(
            song.toggle(song.cursor(), lane(0), Kind::Tap).unwrap(),
            Toggled::Placed
        );
        assert_eq!(objects(&song), [hold, object(240, 0, "tap")]);
    }

    #[test]
    fn an_edit_that_breaks_a_format_rule_is_refused_and_changes_nothing() {
        let mut song = Song::new(document(json!([object(0, 0, "tap")])), EDITABLE);
        let refusal = song.toggle(song.cursor(), lane(4), Kind::Tap).unwrap_err();
        assert!(matches!(refusal, Refusal::Document(_)), "{refusal:?}");
        assert_eq!(
            refusal.to_string(),
            "charts[0].objects[1]: position covers a lane past the last of 4"
        );
        assert_eq!(objects(&song), [object(0, 0, "tap")]);
    }

    #[test]
    fn an_edit_the_game_refuses_is_refused_and_changes_nothing() {
        let mut song = Song::new(document(json!([object(0, 0, "tap")])), &[&NoLastLane]);
        let refusal = song.toggle(song.cursor(), lane(3), Kind::Tap).unwrap_err();
        assert!(matches!(refusal, Refusal::Game(_)), "{refusal:?}");
        assert_eq!(refusal.to_string(), "a note on lane 4");
        assert_eq!(objects(&song), [object(0, 0, "tap")]);
    }

    #[test]
    fn a_read_only_song_refuses_every_edit() {
        let mut song = Song::new(document(json!([object(0, 0, "tap")])), &[]);
        let refusal = song.toggle(song.cursor(), lane(1), Kind::Tap).unwrap_err();
        assert!(matches!(refusal, Refusal::ReadOnly), "{refusal:?}");
        assert_eq!(objects(&song), [object(0, 0, "tap")]);
    }

    #[test]
    fn a_song_without_charts_has_nothing_to_edit() {
        let empty = json!({
            "format_version": 0,
            "ruleset": { "id": "test", "version": "1" },
            "metadata": { "title": "t", "artist": "a", "audio": "song.ogg" },
            "timing": { "offset_seconds": 0.0, "events": [ { "tick": 0, "kind": { "bpm": 120.0 } } ] }
        });
        let mut song = Song::open(&serde_json::to_vec(&empty).unwrap(), EDITABLE).unwrap();
        assert!(matches!(
            song.toggle(song.cursor(), lane(0), Kind::Tap),
            Err(Refusal::NoChart)
        ));
    }

    /// A song with a tap placed on lane 1 at tick 0 and on lane 2 at tick 240, and the cursor
    /// left three sixteenths on.
    fn two_taps_placed() -> Song {
        let mut song = Song::new(document(json!([])), EDITABLE);
        song.toggle(song.cursor(), lane(0), Kind::Tap).unwrap();
        song.step(Step::Later);
        song.toggle(song.cursor(), lane(1), Kind::Tap).unwrap();
        song.step(Step::Later);
        song.step(Step::Later);
        song
    }

    #[test]
    fn undo_takes_edits_back_newest_first_and_returns_the_cursor_to_each() {
        let mut song = two_taps_placed();
        song.undo().unwrap();
        assert_eq!(objects(&song), [object(0, 0, "tap")]);
        assert_eq!(song.cursor(), at(240));
        song.undo().unwrap();
        assert_eq!(objects(&song), Vec::<Value>::new());
        assert_eq!(song.cursor(), at(0));
        assert!(!song.can_undo());
    }

    #[test]
    fn redo_makes_undone_edits_again_oldest_first() {
        let mut song = two_taps_placed();
        song.undo().unwrap();
        song.undo().unwrap();
        song.redo().unwrap();
        assert_eq!(objects(&song), [object(0, 0, "tap")]);
        assert_eq!(song.cursor(), at(0));
        song.redo().unwrap();
        assert_eq!(objects(&song), [object(0, 0, "tap"), object(240, 1, "tap")]);
        assert_eq!(song.cursor(), at(240));
        assert!(!song.can_redo());
    }

    #[test]
    fn a_new_edit_drops_what_could_have_been_redone() {
        let mut song = two_taps_placed();
        song.undo().unwrap();
        song.toggle(song.cursor(), lane(3), Kind::Tap).unwrap();
        assert!(!song.can_redo());
        song.redo().unwrap();
        assert_eq!(objects(&song), [object(0, 0, "tap"), object(240, 3, "tap")]);
    }

    #[test]
    fn undoing_a_removal_puts_the_note_back_where_it_was() {
        let three = json!([
            object(0, 0, "tap"),
            object(0, 1, "mine"),
            object(480, 2, "tap")
        ]);
        let mut song = Song::new(document(three.clone()), EDITABLE);
        song.toggle(song.cursor(), lane(1), Kind::Tap).unwrap();
        song.undo().unwrap();
        assert_eq!(Value::Array(objects(&song)), three);
    }

    #[test]
    fn undo_opens_the_chart_the_edit_was_made_in() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        song.toggle(song.cursor(), lane(0), Kind::Tap).unwrap();
        song.open_chart(1);
        song.undo().unwrap();
        assert_eq!(song.chart_index(), 0);
        assert_eq!(objects(&song), Vec::<Value>::new());
    }

    #[test]
    fn with_nothing_to_undo_or_redo_both_leave_the_song_alone() {
        let mut song = Song::new(document(json!([object(0, 0, "tap")])), EDITABLE);
        assert!(!song.can_undo() && !song.can_redo());
        song.undo().unwrap();
        song.redo().unwrap();
        assert_eq!(objects(&song), [object(0, 0, "tap")]);
    }

    fn four_taps() -> Song {
        let taps = json!([
            object(0, 0, "tap"),
            object(240, 1, "tap"),
            object(480, 2, "tap"),
            object(720, 3, "tap")
        ]);
        Song::new(document(taps), EDITABLE)
    }

    fn note_at(tick: i32, index: u16) -> NoteAt {
        NoteAt {
            tick: at(tick),
            lane: lane(index),
        }
    }

    fn selection(song: &Song) -> Vec<NoteAt> {
        song.selection().collect()
    }

    #[test]
    fn a_marquee_selects_the_notes_that_start_inside_it() {
        let mut song = four_taps();
        song.select_between(at(240)..=at(480), lane(1)..=lane(2), Selecting::Replace);
        assert_eq!(selection(&song), [note_at(240, 1), note_at(480, 2)]);
    }

    #[test]
    fn adding_and_subtracting_meet_the_selection_there_is() {
        let mut song = four_taps();
        song.select_between(at(0)..=at(240), lane(0)..=lane(3), Selecting::Replace);
        song.select([note_at(480, 2)], Selecting::Add);
        song.select_between(at(0)..=at(0), lane(0)..=lane(3), Selecting::Subtract);
        assert_eq!(selection(&song), [note_at(240, 1), note_at(480, 2)]);
        song.select([note_at(720, 3)], Selecting::Replace);
        assert_eq!(selection(&song), [note_at(720, 3)]);
    }

    #[test]
    fn a_wide_note_is_selected_through_any_lane_it_covers() {
        let wide = json!({ "tick": 0, "position": { "lane": { "index": 2, "width": 2 } },
                           "kind": "hold", "shape": { "sustain": { "duration": 480 } } });
        let mut song = Song::new(document(json!([wide])), EDITABLE);
        song.select_between(at(0)..=at(0), lane(3)..=lane(3), Selecting::Replace);
        assert_eq!(selection(&song), [note_at(0, 2)]);
        let chart = song.chart().unwrap();
        assert!(song.is_selected(&chart.objects[0]));
    }

    #[test]
    fn selecting_a_note_that_is_not_there_selects_nothing() {
        let mut song = four_taps();
        song.select([note_at(960, 0), note_at(240, 0)], Selecting::Replace);
        assert_eq!(selection(&song), []);
    }

    #[test]
    fn select_all_takes_the_open_chart_and_opening_another_clears_it() {
        let mut song = four_taps();
        song.select_all();
        assert_eq!(selection(&song).len(), 4);
        song.open_chart(1);
        assert_eq!(selection(&song), []);
    }

    #[test]
    fn deleting_the_selection_is_one_step_that_undo_takes_back() {
        let mut song = four_taps();
        song.select([note_at(0, 0), note_at(480, 2)], Selecting::Replace);
        song.delete_selection().unwrap();
        assert_eq!(
            objects(&song),
            [object(240, 1, "tap"), object(720, 3, "tap")]
        );
        assert_eq!(selection(&song), []);
        song.undo().unwrap();
        assert_eq!(objects(&song).len(), 4);
        assert!(!song.can_undo());
    }

    #[test]
    fn deleting_an_empty_selection_changes_nothing() {
        let mut song = four_taps();
        song.delete_selection().unwrap();
        assert_eq!(objects(&song).len(), 4);
        assert!(!song.can_undo());
    }

    #[test]
    fn a_read_only_song_selects_but_refuses_to_delete() {
        let taps = json!([object(0, 0, "tap"), object(240, 1, "tap")]);
        let mut song = Song::new(document(taps), &[]);
        song.select_all();
        assert!(matches!(song.delete_selection(), Err(Refusal::ReadOnly)));
        assert_eq!(selection(&song).len(), 2);
    }

    #[test]
    fn a_selected_note_an_edit_removes_leaves_the_selection() {
        let mut song = four_taps();
        song.select_all();
        song.toggle(at(0), lane(0), Kind::Tap).unwrap();
        assert_eq!(selection(&song).len(), 3);
    }

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
        song.open_chart(1);
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
}
