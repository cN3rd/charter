#![expect(
    clippy::mod_module_files,
    reason = "the refactor deliberately uses a song/mod.rs module directory"
)]
//! The editor song model, split by concern: layers, notes, selection, timing, markers,
//! history, cursor movement and metadata.

use std::collections::BTreeSet;
use std::num::NonZeroU16;
use std::ops::RangeInclusive;

use charter_core::{
    Anchor, Bpm, Chart, Error, Extensions, Kind, LaneCount, LaneIndex, LaneOffset, LaneSpan, Layer,
    LayerColour, LayerName, LayerView, Marker, MarkerKind, Metadata, Object, Position,
    ResourcePath, RulesetPlugin, SettingScope, Shape, Tick, TickOffset, Ticks, TicksPerBeat,
    Timing, TimingKind, Valid, ValidDocument,
};

use crate::edit::{Edit, Op};

mod cursor;
mod history;
mod layers;
mod markers;
mod notes;
mod selection;
mod settings;
mod tempo;

#[expect(
    clippy::pub_use,
    clippy::useless_attribute,
    reason = "TimedSpan stays in charter-editor's existing flat public API"
)]
pub use tempo::TimedSpan;

use history::History;

/// Which way the cursor moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Toward an earlier tick.
    Earlier,
    /// Toward a later tick.
    Later,
}

/// What a lane key did at the cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggled {
    /// A note was added.
    Placed,
    /// A note already there was removed.
    Removed,
}

/// A lane note of the open chart, by the tick it starts on and the first lane it covers, which
/// no other note starting on that tick shares.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NoteAt {
    /// Where the note starts.
    pub tick: Tick,
    /// First lane the note covers.
    pub lane: LaneIndex,
}

/// How notes being selected meet the selection there is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selecting {
    /// Discard the previous selection.
    Replace,
    /// Keep the previous selection as well.
    Add,
    /// Remove matching notes from the selection.
    Subtract,
}

/// One row in the open chart's layer list, including its note count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerRow {
    /// Display name.
    pub name: String,
    /// Palette colour.
    pub colour: LayerColour,
    /// Visibility.
    pub view: LayerView,
    /// Lock state.
    pub locked: bool,
    /// Number of notes on this layer.
    pub notes: usize,
}

/// What to do with notes when deleting a layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerNotes {
    /// Delete the layer's notes.
    Delete,
    /// Move its notes into the layer below.
    MergeDown,
}

/// Why an edit did not happen. The song stays as it was.
#[derive(Debug, thiserror::Error)]
pub enum Refusal {
    /// There is no plugin authorised to edit the song.
    #[error("the song is read only")]
    ReadOnly,
    /// The song has no chart open for note edits.
    #[error("the song has no chart to edit")]
    NoChart,
    /// The edit breaks a rule of the format.
    #[error(transparent)]
    Document(#[from] Error),
    /// The song's game cannot represent the edit.
    #[error(transparent)]
    Game(Box<dyn std::error::Error + Send + Sync>),
    /// An anchor would reach or pass the one on this tick, in beats or in the audio.
    #[error("that would cross the anchor beside it")]
    Crosses(Tick),
    /// The edit would leave the song without a BPM change.
    #[error("a song keeps at least one BPM change")]
    OnlyTempo,
    /// The edit would put something between two ticks, or out of their range.
    #[error("that does not come out on whole ticks")]
    NotWhole,
    /// The song's first BPM change starts its tempo, so it cannot go.
    #[error("the first BPM change starts the song's tempo")]
    FirstTempo,
    /// A note would start where another already starts on a lane it covers.
    #[error("another note is already there")]
    Occupied,
    /// A note would move past the first or last lane, or out of the range of ticks.
    #[error("that would move a note off the chart")]
    OffTheChart,
    /// The active layer is hidden.
    #[error("Layer {0} is hidden")]
    LayerHidden(String),
    /// The active layer is ghosted.
    #[error("Layer {0} is ghosted")]
    LayerGhosted(String),
    /// The active layer is locked.
    #[error("Layer {0} is locked")]
    LayerLocked(String),
    /// Another layer has a note occupying the destination.
    #[error("A note on layer {0} is in the way")]
    OnLayer(String),
    /// A layer name is invalid or already used, or a layer row does not exist.
    #[error("Invalid or duplicate layer name: {0}")]
    LayerName(String),
    /// The chart must keep one layer.
    #[error("Cannot remove the last layer")]
    LastLayer,
}

/// What setting a BPM keeps of the notes, events and markers in the stretch it times, from its
/// point to the next or to the end. Everything past the next point keeps its moment either way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KeepNotes {
    /// They stay where they sound, and the beat grid changes under them.
    #[default]
    InTime,
    /// They stay on their beats, and move in the audio with the grid.
    OnBeats,
}

/// The snaps in ArrowVortex's menu, coarsest first.
pub const SNAPS: [u16; 10] = [4, 8, 12, 16, 24, 32, 48, 64, 96, 192];

/// The snap a song opens on, when its resolution holds it.
const DEFAULT_SNAP: u16 = 16;

/// A song open in the editor.
#[derive(Debug)]
pub struct Song {
    /// Validated chart data as opened or last edited.
    document: ValidDocument,
    /// Plugin that authorises edits, or the reason this song is read-only.
    rules: Rules,
    /// Index of the chart currently open.
    chart: usize,
    /// Tick where edits and playback start.
    cursor: Tick,
    /// Notes per whole note at the current snap resolution.
    snap: NonZeroU16,
    /// What a BPM set on a timing point keeps of the notes it times.
    keep: KeepNotes,
    /// Undo and redo state for edits made to the document.
    history: History,
    /// Selected notes of the open chart, each one there.
    selection: BTreeSet<NoteAt>,
    /// Per-chart editor state kept beside the document.
    chart_state: Vec<ChartState>,
}

/// Editor state kept per chart beside the document.
#[derive(Clone, Debug, Default)]
struct ChartState {
    /// Active row of the chart, clamped whenever the chart's layers change.
    active_layer: LayerIndex,
    /// Views to restore after soloing, kept outside undo and the document.
    solo: Option<SoloLayer>,
}

/// A row in a chart's layer list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct LayerIndex(usize);

impl LayerIndex {
    /// The first layer row.
    const ZERO: Self = Self(0);

    /// Wraps a raw layer row index at public API boundaries.
    const fn new(index: usize) -> Self {
        Self(index)
    }

    /// Raw index for collection access and public API returns.
    const fn get(self) -> usize {
        self.0
    }

    /// Clamps a caller-provided index to the last row that exists.
    fn clamped(index: usize, len: usize) -> Self {
        Self(index.min(len.saturating_sub(1)))
    }
}

/// Views remembered while one layer is soloed.
#[derive(Clone, Debug)]
struct SoloLayer {
    /// The row that is currently soloed.
    layer: LayerIndex,
    /// Views to restore when the same row is unsoloed.
    views: Vec<LayerView>,
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
    /// of `plugins` is for its ruleset and accepts it. Charts without layers get "Layer 0",
    /// holding all their notes, without that counting as an edit.
    #[must_use]
    pub fn new(document: ValidDocument, plugins: &[&'static dyn RulesetPlugin]) -> Self {
        let document = layers::with_layers(document);
        let charts = document.document().charts.len();
        let mut song = Self {
            rules: Rules::of(&document, plugins),
            document,
            chart: 0,
            cursor: Tick::new(0),
            snap: NonZeroU16::MIN,
            keep: KeepNotes::InTime,
            history: History::new(),
            selection: BTreeSet::new(),
            chart_state: vec![ChartState::default(); charts],
        };
        let snap = song
            .snaps()
            .find(|snap| snap.get() == DEFAULT_SNAP)
            .or_else(|| song.snaps().next())
            .unwrap_or(NonZeroU16::MIN);
        song.snap = snap;
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

    /// The validated data held by this song.
    #[must_use]
    pub const fn document(&self) -> &ValidDocument {
        &self.document
    }

    /// Index of the chart currently open, even when the song has no charts.
    #[must_use]
    pub const fn chart_index(&self) -> usize {
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
}

#[cfg(test)]
pub(crate) mod tests {
    use std::error::Error;

    use serde_json::{Value, json};

    use super::*;

    pub(crate) fn note(n: u16) -> NonZeroU16 {
        NonZeroU16::new(n).unwrap()
    }

    pub(crate) fn at(tick: i32) -> Tick {
        Tick::new(tick)
    }

    /// Accepts every document of the `test` ruleset.
    #[derive(Debug)]
    pub(crate) struct Lenient;

    #[expect(
        clippy::missing_trait_methods,
        reason = "a test plugin: the trait's defaults for every optional method"
    )]
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
    pub(crate) struct NoLastLane;

    #[expect(
        clippy::missing_trait_methods,
        reason = "a test plugin: the trait's defaults for every optional method"
    )]
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
    pub(crate) struct NoHolds;

    #[expect(
        clippy::missing_trait_methods,
        reason = "a test plugin: the trait's defaults for every optional method"
    )]
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

    /// Refuses charts with multiple layers, exercising the shared plugin edit gate.
    #[derive(Debug)]
    pub(crate) struct NoExtraLayers;

    #[expect(
        clippy::missing_trait_methods,
        reason = "a test plugin: the trait's defaults for every optional method"
    )]
    impl RulesetPlugin for NoExtraLayers {
        fn id(&self) -> &'static str {
            "test"
        }

        fn check(&self, document: &ValidDocument) -> Result<(), Box<dyn Error + Send + Sync>> {
            if document
                .document()
                .charts
                .iter()
                .any(|chart| chart.layers.len() > 1)
            {
                Err("too many layers".into())
            } else {
                Ok(())
            }
        }
    }

    /// A plugin for another game, which has no say over `test` documents.
    #[derive(Debug)]
    pub(crate) struct Elsewhere;

    #[expect(
        clippy::missing_trait_methods,
        reason = "a test plugin: the trait's defaults for every optional method"
    )]
    impl RulesetPlugin for Elsewhere {
        fn id(&self) -> &'static str {
            "sm"
        }

        fn check(&self, _: &ValidDocument) -> Result<(), Box<dyn Error + Send + Sync>> {
            Ok(())
        }
    }

    pub(crate) const EDITABLE: &[&dyn RulesetPlugin] = &[&Elsewhere, &Lenient];

    /// A `test` document with one four-lane chart holding `objects`, then a second, empty one.
    pub(crate) fn document(objects: Value) -> ValidDocument {
        let mut value = json!({
            "format_version": 1,
            "ruleset": { "id": "test", "version": "1" },
            "metadata": { "title": "t", "artist": "a", "audio": "song.ogg" },
            "timing": { "anchors": [ { "tick": 0, "seconds": 0.0 } ], "tempo": 120.0, "events": [] },
            "charts": [ { "name": "Easy", "lanes": 4, "layers": [{"name":"Layer 0","colour":"blue"}] }, { "name": "Hard", "lanes": 4, "layers": [{"name":"Layer 0","colour":"blue"}] } ]
        });
        value["charts"][0]["objects"] = objects;
        ValidDocument::from_json(&serde_json::to_vec(&value).unwrap()).unwrap()
    }

    pub(crate) fn object(tick: i32, lane: u16, kind: &str) -> Value {
        json!({ "tick": tick, "position": { "lane": { "index": lane } }, "kind": kind, "shape": "instant", "layer": "Layer 0" })
    }

    pub(crate) fn objects(song: &Song) -> Vec<Value> {
        let chart = song.chart().unwrap();
        chart
            .objects
            .iter()
            .map(|object| serde_json::to_value(object).unwrap())
            .collect()
    }

    pub(crate) fn lane(index: u16) -> LaneIndex {
        LaneIndex::new(index)
    }

    pub(crate) fn note_at(tick: i32, index: u16) -> NoteAt {
        NoteAt {
            tick: at(tick),
            lane: lane(index),
        }
    }

    pub(crate) fn hold(tick: i32, lane: u16, duration: u32) -> Value {
        json!({ "tick": tick, "position": { "lane": { "index": lane } }, "kind": "hold", "shape": { "sustain": { "duration": duration } }, "layer": "Layer 0" })
    }

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
}
