//! An open song: its document, which chart is open, where the cursor is, and the edits made to
//! it.

use std::collections::BTreeSet;
use std::num::NonZeroU16;
use std::ops::RangeInclusive;

use charter_core::{
    Bpm, Chart, Error, Extensions, Kind, LaneCount, LaneIndex, LaneSpan, Marker, MarkerKind,
    Metadata, Object, Position, ResourcePath, RulesetPlugin, Shape, Tick, Ticks, Timing,
    TimingEvent, TimingKind, Valid, ValidDocument,
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
    /// A beat cannot reach the time asked without passing a neighbouring BPM change.
    #[error("that beat cannot move past the BPM change beside it")]
    Unreachable,
    /// The song's first BPM change starts its tempo, so it cannot go.
    #[error("the first BPM change starts the song's tempo")]
    FirstTempo,
}

/// The snaps in ArrowVortex's menu, coarsest first.
pub const SNAPS: [u16; 10] = [4, 8, 12, 16, 24, 32, 48, 64, 96, 192];

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
    /// How many edits were in effect when the song was opened or last saved, unless no undo or
    /// redo can bring that state back.
    saved: Option<usize>,
    /// Selected notes of the open chart, each one there.
    selection: BTreeSet<NoteAt>,
    /// How many times the notes have changed since the song opened.
    revision: u64,
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
            saved: Some(0),
            selection: BTreeSet::new(),
            revision: 0,
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

    /// Makes the song's audio the file at `audio`, as one step.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song.
    pub fn set_audio(&mut self, audio: ResourcePath) -> Result<(), Refusal> {
        let old = &self.document.document().metadata;
        if old.audio == audio {
            return Ok(());
        }
        let new = Metadata {
            audio,
            ..old.clone()
        };
        let edit = Edit(vec![Op::Metadata {
            old: Box::new(old.clone()),
            new: Box::new(new),
        }]);
        self.document = self.edited(self.chart, &edit)?;
        self.record(edit);
        Ok(())
    }

    /// The timing tempo edits go to: the open chart's own when it has one, and whether it is.
    pub(crate) fn edited_timing(&self) -> (bool, &charter_core::Timing) {
        let document = self.document.document();
        let own = document
            .charts
            .get(self.chart)
            .and_then(|chart| chart.timing.as_ref());
        own.map_or((false, &document.timing), |own| (true, own))
    }

    /// The BPM change in effect at the cursor, and its tick: the latest at or before the
    /// cursor, or the first when the cursor is before every change.
    #[must_use]
    pub fn bpm(&self) -> Option<(Tick, Bpm)> {
        let (_, timing) = self.edited_timing();
        let changes = timing.events.iter().filter_map(|event| match event.kind {
            TimingKind::Bpm(bpm) => Some((event.tick, bpm)),
            _ => None,
        });
        let mut first = None;
        let mut latest = None;
        for change in changes {
            first.get_or_insert(change);
            if change.0 <= self.cursor {
                latest = Some(change);
            }
        }
        latest.or(first)
    }

    /// Makes the tempo from the cursor on `bpm`: the BPM change at the cursor takes it, or a new
    /// change there does. Does nothing when `bpm` is already in effect there.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song, or when the game refuses the tempo.
    pub fn set_bpm(&mut self, bpm: Bpm) -> Result<(), Refusal> {
        let at = self.cursor;
        if self.bpm().is_some_and(|(_, held)| held == bpm) {
            return Ok(());
        }
        let (own, timing) = self.edited_timing();
        let mut new = timing.clone();
        let here = new
            .events
            .iter_mut()
            .find(|event| event.tick == at && matches!(event.kind, TimingKind::Bpm(_)));
        if let Some(change) = here {
            change.kind = TimingKind::Bpm(bpm);
        } else {
            let index = new.events.partition_point(|event| event.tick <= at);
            let change = TimingEvent {
                tick: at,
                kind: TimingKind::Bpm(bpm),
                ext: Extensions::new(),
            };
            new.events.insert(index, change);
        }
        self.retime(own, new)
    }

    /// Removes the BPM change at the cursor, if there is one.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song, and when the change is the timing's only one.
    pub fn remove_bpm(&mut self) -> Result<(), Refusal> {
        let at = self.cursor;
        let (own, timing) = self.edited_timing();
        let mut new = timing.clone();
        let before = new.events.len();
        new.events
            .retain(|event| !(event.tick == at && matches!(event.kind, TimingKind::Bpm(_))));
        if new.events.len() == before {
            return Ok(());
        }
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
        if from == to || !timing.events.iter().any(|event| event.tick == from) {
            return Ok(());
        }
        let mut new = timing.clone();
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

    /// Moves the song's marker at `index` onto `to`, among the markers there, as one step. Does
    /// nothing when there is no such marker or it is already there.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song, and when a marker of its kind is already at `to`.
    pub fn move_marker(&mut self, index: usize, to: Tick) -> Result<(), Refusal> {
        let mut new = self.document.document().markers.clone();
        if new.get(index).is_none_or(|marker| marker.tick == to) {
            return Ok(());
        }
        let mut marker = new.remove(index);
        marker.tick = to;
        let at = new.partition_point(|marker| marker.tick <= to);
        new.insert(at, marker);
        self.remark(new)
    }

    /// Names the song `title` by `artist`, as one step. Does nothing when it already is.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song.
    pub fn set_song_facts(&mut self, title: &str, artist: &str) -> Result<(), Refusal> {
        let old = &self.document.document().metadata;
        if old.title == title && old.artist == artist {
            return Ok(());
        }
        let new = Metadata {
            title: title.to_owned(),
            artist: artist.to_owned(),
            ..old.clone()
        };
        let edit = Edit(vec![Op::Metadata {
            old: Box::new(old.clone()),
            new: Box::new(new),
        }]);
        self.document = self.edited(self.chart, &edit)?;
        self.record(edit);
        Ok(())
    }

    /// Puts tick 0 of the timing tempo edits go to `offset` seconds into the audio, as one step,
    /// so a song that starts with silence starts its grid where the music does.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song.
    pub fn set_offset(&mut self, offset: charter_core::Origin) -> Result<(), Refusal> {
        let (own, timing) = self.edited_timing();
        if timing.offset_seconds == offset {
            return Ok(());
        }
        let mut new = timing.clone();
        new.offset_seconds = offset;
        self.retime(own, new)
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

    /// Puts the cursor on `tick`, such as a marker's.
    pub fn jump_to(&mut self, tick: Tick) {
        self.cursor = tick;
    }

    /// The name of the section that starts at the cursor, if one does.
    #[must_use]
    pub fn section(&self) -> Option<&str> {
        let markers = &self.document.document().markers;
        markers
            .iter()
            .find(|marker| marker.tick == self.cursor && marker.kind == MarkerKind::Section)
            .map(|marker| marker.text.as_str())
    }

    /// Names the section that starts at the cursor `text`, starting one there if none does, as
    /// one step. Does nothing when it already has that name.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song.
    pub fn set_section(&mut self, text: &str) -> Result<(), Refusal> {
        if self.section() == Some(text) {
            return Ok(());
        }
        let at = self.cursor;
        let mut new = self.document.document().markers.clone();
        let here = new
            .iter_mut()
            .find(|marker| marker.tick == at && marker.kind == MarkerKind::Section);
        if let Some(section) = here {
            text.clone_into(&mut section.text);
        } else {
            let index = new.partition_point(|marker| marker.tick <= at);
            let section = Marker {
                tick: at,
                kind: MarkerKind::Section,
                text: text.to_owned(),
            };
            new.insert(index, section);
        }
        self.remark(new)
    }

    /// Removes the song's marker at `index` in its markers, of whatever kind, as one step.
    /// Does nothing when there is none there.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song.
    pub fn remove_marker(&mut self, index: usize) -> Result<(), Refusal> {
        if self.read_only().is_some() {
            return Err(Refusal::ReadOnly);
        }
        let mut new = self.document.document().markers.clone();
        if index >= new.len() {
            return Ok(());
        }
        new.remove(index);
        self.remark(new)
    }

    /// Gives the song the markers `new`, as one step.
    fn remark(&mut self, new: Vec<Marker>) -> Result<(), Refusal> {
        let old = self.document.document().markers.clone();
        let edit = Edit(vec![Op::Markers { old, new }]);
        self.document = self.edited(self.chart, &edit)?;
        self.record(edit);
        Ok(())
    }

    /// Where the open chart's last note ends, or tick 0 when it has none.
    #[must_use]
    pub fn end(&self) -> Tick {
        self.chart()
            .and_then(|chart| chart.objects().map(|object| object.end()).max())
            .unwrap_or(Tick::new(0))
    }

    /// The tick of the nearest section past the cursor that way, if there is one.
    #[must_use]
    pub fn section_from(&self, step: Step) -> Option<Tick> {
        let sections = self
            .document
            .document()
            .markers
            .iter()
            .filter(|marker| marker.kind == MarkerKind::Section)
            .map(|marker| marker.tick);
        match step {
            Step::Later => sections.filter(|tick| *tick > self.cursor).min(),
            Step::Earlier => sections.filter(|tick| *tick < self.cursor).max(),
        }
    }

    /// Adds a bookmark at the cursor, or removes the one there, as one step.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song.
    pub fn toggle_bookmark(&mut self) -> Result<(), Refusal> {
        let at = self.cursor;
        let mut new = self.document.document().markers.clone();
        let here = |marker: &Marker| marker.tick == at && marker.kind == MarkerKind::Bookmark;
        if let Some(index) = new.iter().position(here) {
            new.remove(index);
        } else {
            let index = new.partition_point(|marker| marker.tick <= at);
            let bookmark = Marker {
                tick: at,
                kind: MarkerKind::Bookmark,
                text: String::new(),
            };
            new.insert(index, bookmark);
        }
        self.remark(new)
    }

    /// Whether a note of the open chart starts at `tick` on `lane`.
    #[must_use]
    pub fn starts_at(&self, tick: Tick, lane: LaneIndex) -> bool {
        self.starting(tick, lane).is_some()
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
        self.revision += 1;
        self.history.truncate(self.done);
        if self.saved.is_some_and(|saved| saved > self.done) {
            self.saved = None;
        }
        if let Some(last) = self.history.last_mut()
            && last.chart == self.chart
            && last.edit.absorb(&edit)
        {
            if self.saved == Some(self.done) {
                self.saved = None;
            }
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
        if !edit.apply(&mut document, chart) {
            return Err(Refusal::NoChart);
        }
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

    /// Whether the song differs from when it was opened or last saved.
    #[must_use]
    pub fn is_modified(&self) -> bool {
        self.saved != Some(self.done)
    }

    /// Takes the song as it stands to be what its file holds.
    pub fn mark_saved(&mut self) {
        self.saved = Some(self.done);
    }

    /// Takes the song to differ from what its file holds, as a song restored from an autosave
    /// does.
    pub fn mark_unsaved(&mut self) {
        self.saved = None;
    }

    /// A count of the changes made to the song's notes, edits, undos and redos alike, so two
    /// equal revisions of one song hold the same notes.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
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
        self.revision += 1;
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

    /// Snaps to `snap` if it is one of [`SNAPS`] and a whole number of ticks at the song's
    /// resolution, and otherwise keeps the snap there is.
    pub fn set_snap(&mut self, snap: NonZeroU16) {
        if self.snaps().any(|held| held == snap) {
            self.snap = snap;
        }
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
pub(crate) mod tests {
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

    #[test]
    fn a_song_is_unmodified_until_edited_and_again_once_undone() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        assert!(!song.is_modified());
        song.toggle(at(0), lane(0), Kind::Tap).unwrap();
        assert!(song.is_modified());
        song.undo().unwrap();
        assert!(!song.is_modified());
        song.redo().unwrap();
        assert!(song.is_modified());
    }

    #[test]
    fn saving_makes_the_song_as_it_stands_unmodified() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        song.toggle(at(0), lane(0), Kind::Tap).unwrap();
        song.mark_saved();
        assert!(!song.is_modified());
        song.undo().unwrap();
        assert!(song.is_modified());
    }

    #[test]
    fn a_saved_state_undone_and_edited_over_is_gone_for_good() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        song.toggle(at(0), lane(0), Kind::Tap).unwrap();
        song.mark_saved();
        song.undo().unwrap();
        song.toggle(at(0), lane(1), Kind::Tap).unwrap();
        song.undo().unwrap();
        assert!(song.is_modified());
    }

    #[test]
    fn stretching_a_note_placed_before_saving_modifies_the_song() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        song.toggle(at(0), lane(0), Kind::Tap).unwrap();
        song.mark_saved();
        song.stretch(lane(0), at(0), at(480)).unwrap();
        assert!(song.is_modified());
        song.undo().unwrap();
        assert!(song.is_modified());
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

    fn audio(song: &Song) -> &str {
        song.document().document().metadata.audio.as_str()
    }

    #[test]
    fn setting_the_audio_is_a_step_undo_takes_back() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        song.set_audio(ResourcePath::new("track.ogg").unwrap())
            .unwrap();
        assert_eq!(audio(&song), "track.ogg");
        assert!(song.is_modified());
        song.undo().unwrap();
        assert_eq!(audio(&song), "song.ogg");
        assert!(!song.is_modified());
        song.redo().unwrap();
        assert_eq!(audio(&song), "track.ogg");
    }

    #[test]
    fn setting_the_audio_it_already_has_changes_nothing() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        song.set_audio(ResourcePath::new("song.ogg").unwrap())
            .unwrap();
        assert!(!song.can_undo());
    }

    #[test]
    fn a_read_only_song_refuses_new_audio() {
        let mut song = Song::new(document(json!([])), &[]);
        let refused = song.set_audio(ResourcePath::new("track.ogg").unwrap());
        assert!(matches!(refused, Err(Refusal::ReadOnly)));
        assert_eq!(audio(&song), "song.ogg");
    }

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
        serde_json::to_value(&song.document().document().timing.events).unwrap()
    }

    fn tempo(bpm: f64) -> Bpm {
        Bpm::try_from(bpm).unwrap()
    }

    #[test]
    fn a_bpm_set_between_changes_is_a_new_change_undo_takes_back() {
        let meter = json!({ "tick": 960, "kind": { "time_signature": [3, 4] } });
        let mut song = timed(&json!([bpm_event(0, 120.0), meter.clone()]), None);
        song.step(Step::Later);
        let at = song.cursor();
        assert_eq!(at, Tick::new(240));
        song.set_bpm(tempo(150.0)).unwrap();
        let expected = json!([bpm_event(0, 120.0), bpm_event(240, 150.0), meter]);
        assert_eq!(song_events(&song), expected);
        assert_eq!(song.bpm(), Some((at, tempo(150.0))));
        song.undo().unwrap();
        assert_eq!(song.bpm(), Some((Tick::new(0), tempo(120.0))));
    }

    #[test]
    fn a_bpm_set_on_a_change_changes_it() {
        let mut song = timed(&json!([bpm_event(0, 120.0)]), None);
        song.set_bpm(tempo(90.5)).unwrap();
        assert_eq!(song_events(&song), json!([bpm_event(0, 90.5)]));
    }

    #[test]
    fn setting_the_bpm_already_in_effect_there_changes_nothing() {
        let mut song = timed(&json!([bpm_event(0, 120.0)]), None);
        song.set_bpm(tempo(120.0)).unwrap();
        assert!(!song.can_undo());
    }

    #[test]
    fn a_bpm_change_removed_at_the_cursor_goes_and_the_last_one_stays() {
        let mut song = timed(&json!([bpm_event(0, 120.0), bpm_event(480, 150.0)]), None);
        song.step(Step::Later);
        song.step(Step::Later);
        assert_eq!(song.cursor(), Tick::new(480));
        song.remove_bpm().unwrap();
        assert_eq!(song_events(&song), json!([bpm_event(0, 120.0)]));
        song.undo().unwrap();
        song.step(Step::Earlier);
        song.step(Step::Earlier);
        song.remove_bpm().unwrap();
        assert_eq!(song_events(&song), json!([bpm_event(480, 150.0)]));
        song.step(Step::Later);
        song.step(Step::Later);
        assert!(song.remove_bpm().is_err());
        assert_eq!(song_events(&song), json!([bpm_event(480, 150.0)]));
    }

    #[test]
    fn removing_where_no_bpm_changes_changes_nothing() {
        let mut song = timed(&json!([bpm_event(0, 120.0)]), None);
        song.step(Step::Later);
        song.remove_bpm().unwrap();
        assert!(!song.can_undo());
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
        let own = serde_json::to_value(&own.as_ref().unwrap().events).unwrap();
        assert_eq!(own, json!([bpm_event(0, 200.0)]));
    }

    #[test]
    fn a_read_only_song_refuses_tempo_changes() {
        let document = timed(&json!([bpm_event(0, 120.0)]), None)
            .document()
            .clone();
        let mut song = Song::new(document, &[]);
        assert!(matches!(song.set_bpm(tempo(90.0)), Err(Refusal::ReadOnly)));
        assert!(matches!(song.remove_bpm(), Ok(()) | Err(Refusal::ReadOnly)));
    }

    /// A song with `markers`.
    fn marked(markers: &Value) -> Song {
        let value = json!({
            "format_version": 0,
            "ruleset": { "id": "test", "version": "1" },
            "metadata": { "title": "t", "artist": "a", "audio": "song.ogg" },
            "timing": { "offset_seconds": 0.0, "events": [ bpm_event(0, 120.0) ] },
            "markers": markers.clone(),
            "charts": [ { "name": "Easy", "lanes": 4 } ]
        });
        let document = ValidDocument::from_json(&serde_json::to_vec(&value).unwrap()).unwrap();
        Song::new(document, EDITABLE)
    }

    fn marker(tick: i32, kind: &str, text: &str) -> Value {
        json!({ "tick": tick, "kind": kind, "text": text })
    }

    fn markers(song: &Song) -> Value {
        serde_json::to_value(&song.document().document().markers).unwrap()
    }

    #[test]
    fn a_section_named_where_there_is_none_is_a_new_one_undo_takes_back() {
        let mut song = marked(&json!([
            marker(0, "section", "Intro"),
            marker(480, "bookmark", "b")
        ]));
        song.jump_to(Tick::new(480));
        assert_eq!(song.section(), None);
        song.set_section("Verse").unwrap();
        let expected = json!([
            marker(0, "section", "Intro"),
            marker(480, "bookmark", "b"),
            marker(480, "section", "Verse")
        ]);
        assert_eq!(markers(&song), expected);
        assert_eq!(song.section(), Some("Verse"));
        song.undo().unwrap();
        assert_eq!(song.section(), None);
    }

    #[test]
    fn a_section_named_where_one_is_renames_it() {
        let mut song = marked(&json!([marker(0, "section", "Intro")]));
        song.set_section("Start").unwrap();
        assert_eq!(markers(&song), json!([marker(0, "section", "Start")]));
        song.set_section("Start").unwrap();
        assert!(song.can_undo());
        song.undo().unwrap();
        assert!(!song.can_undo());
    }

    #[test]
    fn a_marker_removed_goes_as_one_step() {
        let mut song = marked(&json!([
            marker(0, "section", "Intro"),
            marker(480, "bookmark", "b")
        ]));
        song.remove_marker(0).unwrap();
        assert_eq!(markers(&song), json!([marker(480, "bookmark", "b")]));
        song.remove_marker(5).unwrap();
        song.undo().unwrap();
        assert_eq!(markers(&song).as_array().unwrap().len(), 2);
    }

    #[test]
    fn jumping_puts_the_cursor_on_the_tick() {
        let mut song = marked(&json!([]));
        song.jump_to(Tick::new(1234));
        assert_eq!(song.cursor(), Tick::new(1234));
    }

    #[test]
    fn a_read_only_song_refuses_marker_edits() {
        let document = marked(&json!([marker(0, "section", "Intro")]))
            .document()
            .clone();
        let mut song = Song::new(document, &[]);
        assert!(matches!(song.set_section("x"), Err(Refusal::ReadOnly)));
        assert!(matches!(song.remove_marker(0), Err(Refusal::ReadOnly)));
    }

    #[test]
    fn the_end_is_where_the_open_charts_last_note_ends() {
        let taps = json!([
            object(0, 0, "tap"),
            hold(480, 1, 960),
            object(960, 2, "tap")
        ]);
        let song = Song::new(document(taps), EDITABLE);
        assert_eq!(song.end(), Tick::new(1440));
        let empty = Song::new(document(json!([])), EDITABLE);
        assert_eq!(empty.end(), Tick::new(0));
    }

    #[test]
    fn the_next_and_previous_sections_are_found_from_the_cursor() {
        let mut song = marked(&json!([
            marker(0, "section", "A"),
            marker(480, "bookmark", "b"),
            marker(960, "section", "B")
        ]));
        assert_eq!(song.section_from(Step::Later), Some(Tick::new(960)));
        assert_eq!(song.section_from(Step::Earlier), None);
        song.jump_to(Tick::new(600));
        assert_eq!(song.section_from(Step::Earlier), Some(Tick::new(0)));
        song.jump_to(Tick::new(960));
        assert_eq!(song.section_from(Step::Later), None);
    }

    #[test]
    fn a_bookmark_toggled_at_the_cursor_comes_and_goes() {
        let mut song = marked(&json!([marker(0, "section", "A")]));
        song.toggle_bookmark().unwrap();
        assert_eq!(
            markers(&song),
            json!([marker(0, "section", "A"), marker(0, "bookmark", "")])
        );
        song.toggle_bookmark().unwrap();
        assert_eq!(markers(&song), json!([marker(0, "section", "A")]));
    }

    #[test]
    fn before_the_first_bpm_change_that_change_is_in_effect() {
        let song = timed(&json!([bpm_event(480, 90.0)]), None);
        assert_eq!(song.cursor(), Tick::new(0));
        assert_eq!(song.bpm(), Some((Tick::new(480), tempo(90.0))));
    }

    #[test]
    fn a_note_starts_where_its_head_is_on_any_lane_it_covers() {
        let wide = json!({ "tick": 240, "position": { "lane": { "index": 1, "width": 2 } },
                           "kind": "tap", "shape": "instant" });
        let song = Song::new(document(json!([wide])), EDITABLE);
        assert!(song.starts_at(at(240), lane(1)));
        assert!(song.starts_at(at(240), lane(2)));
        assert!(!song.starts_at(at(240), lane(3)));
        assert!(!song.starts_at(at(0), lane(1)));
    }

    #[test]
    fn removing_a_marker_past_the_end_changes_nothing() {
        let mut song = marked(&json!([marker(0, "section", "A")]));
        song.remove_marker(usize::MAX).unwrap();
        assert!(!song.can_undo());
        assert!(!song.is_modified());
    }

    #[test]
    fn a_timing_event_moved_lands_in_order_and_undo_puts_it_back() {
        let meter = json!({ "tick": 960, "kind": { "time_signature": [3, 4] } });
        let mut song = timed(
            &json!([bpm_event(0, 120.0), bpm_event(480, 150.0), meter.clone()]),
            None,
        );
        song.move_timing_at(Tick::new(480), Tick::new(1920))
            .unwrap();
        assert_eq!(
            song_events(&song),
            json!([bpm_event(0, 120.0), meter, bpm_event(1920, 150.0)])
        );
        song.undo().unwrap();
        assert_eq!(song_events(&song)[1], bpm_event(480, 150.0));
    }

    #[test]
    fn every_timing_event_at_a_tick_moves_together_as_one_step() {
        let meter = |tick: i32| json!({ "tick": tick, "kind": { "time_signature": [3, 4] } });
        let events = json!([bpm_event(0, 120.0), bpm_event(960, 150.0), meter(960)]);
        let mut song = timed(&events, None);
        song.move_timing_at(Tick::new(960), Tick::new(1920))
            .unwrap();
        assert_eq!(
            song_events(&song),
            json!([bpm_event(0, 120.0), bpm_event(1920, 150.0), meter(1920)])
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
        let events = serde_json::to_value(&chart.timing.as_ref().unwrap().events).unwrap();
        assert_eq!(events[1], bpm_event(960, 90.0));
    }

    #[test]
    fn a_marker_moved_lands_in_order_and_undo_puts_it_back() {
        let mut song = marked(&json!([
            marker(0, "section", "A"),
            marker(480, "section", "B")
        ]));
        song.move_marker(0, Tick::new(960)).unwrap();
        assert_eq!(
            markers(&song),
            json!([marker(480, "section", "B"), marker(960, "section", "A")])
        );
        song.undo().unwrap();
        assert_eq!(markers(&song)[0], marker(0, "section", "A"));
    }

    #[test]
    fn a_read_only_song_refuses_to_move_handles() {
        let document = marked(&json!([marker(0, "section", "A")]))
            .document()
            .clone();
        let mut song = Song::new(document, &[]);
        assert!(matches!(
            song.move_marker(0, Tick::new(480)),
            Err(Refusal::ReadOnly)
        ));
        assert!(matches!(
            song.move_timing_at(Tick::new(0), Tick::new(480)),
            Err(Refusal::ReadOnly)
        ));
    }

    fn facts(song: &Song) -> (String, String) {
        let metadata = &song.document().document().metadata;
        (metadata.title.clone(), metadata.artist.clone())
    }

    #[test]
    fn the_title_and_artist_change_as_one_step() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        song.set_song_facts("Starlight", "The Lanes").unwrap();
        assert_eq!(
            facts(&song),
            ("Starlight".to_owned(), "The Lanes".to_owned())
        );
        song.undo().unwrap();
        assert_eq!(facts(&song), ("t".to_owned(), "a".to_owned()));
        assert!(!song.can_undo());
    }

    #[test]
    fn setting_the_title_and_artist_the_song_has_changes_nothing() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        song.set_song_facts("t", "a").unwrap();
        assert!(!song.can_undo());
    }

    fn offset(song: &Song) -> Value {
        serde_json::to_value(song.document().document().timing.offset_seconds).unwrap()
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
        let own = serde_json::to_value(chart.timing.as_ref().unwrap().offset_seconds).unwrap();
        assert_eq!(own, json!(-0.5));
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
    fn the_revision_moves_with_every_change_to_the_notes_and_nothing_else() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        let mut seen = vec![song.revision()];
        let mut changed = |song: &Song| {
            let new = !seen.contains(&song.revision());
            seen.push(song.revision());
            new
        };
        song.toggle(at(0), lane(0), Kind::Tap).unwrap();
        assert!(changed(&song));
        song.stretch(lane(0), at(0), at(480)).unwrap();
        assert!(changed(&song));
        song.undo().unwrap();
        assert!(changed(&song));
        song.redo().unwrap();
        assert!(changed(&song));
        let before = song.revision();
        song.step(Step::Later);
        song.select_all();
        song.open_chart(1);
        assert_eq!(song.revision(), before);
    }

    #[test]
    fn a_song_marked_unsaved_is_modified_until_saved() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        song.mark_unsaved();
        assert!(song.is_modified());
        song.mark_saved();
        assert!(!song.is_modified());
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
    fn a_chosen_snap_stands_only_if_the_menu_has_it_and_the_resolution_holds_it() {
        let mut song = Song::new(document(json!([])), &[]);
        song.set_snap(note(12));
        assert_eq!(song.snap(), note(12));
        song.set_snap(note(7));
        assert_eq!(song.snap(), note(12));
        let coarse = json!({
            "format_version": 0,
            "ticks_per_beat": 100,
            "ruleset": { "id": "test", "version": "1" },
            "metadata": { "title": "t", "artist": "a", "audio": "song.ogg" },
            "timing": { "offset_seconds": 0.0, "events": [ { "tick": 0, "kind": { "bpm": 120.0 } } ] }
        });
        let mut song = Song::open(&serde_json::to_vec(&coarse).unwrap(), &[]).unwrap();
        song.set_snap(note(12));
        assert_eq!(song.snap(), note(16));
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
