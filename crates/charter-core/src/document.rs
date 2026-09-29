use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::InvalidValue;
use crate::ident::{Ease, Key, Kind, LayerName, MarkerKind, Namespace, string_newtype};
use crate::number::Decimal;
use crate::path::ResourcePath;
use crate::unit::{
    AudioTime, Bpm, LaneCount, LaneIndex, LaneSpan, Origin, Seconds, Tick, Ticks, TicksPerBeat,
    TimeSignature,
};

/// The format version this crate reads and writes.
pub const FORMAT_VERSION: u32 = 1;

/// Plugin data keyed by `<namespace>.<name>`, opaque to the core.
pub type Extensions = BTreeMap<Key, Value>;

/// A named point in the song, such as the start of a section.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Marker {
    /// Position of this marker on the song's grid.
    pub tick: Tick,
    /// Shared or plugin-defined marker category.
    pub kind: MarkerKind,
    /// What the editor shows. May be empty.
    pub text: String,
}

/// Ruleset configuration under the ruleset's own unprefixed keys, opaque to the core.
pub type Settings = BTreeMap<String, Value>;

/// A whole chart file: one song, one ruleset, any number of charts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    /// Core schema version, checked before reading the rest of the document.
    pub format_version: u32,
    #[serde(default, skip_serializing_if = "is_default_ticks_per_beat")]
    /// Grid resolution shared by the song and its charts.
    pub ticks_per_beat: TicksPerBeat,
    /// Plugin defining the meaning of the charts.
    pub ruleset: Ruleset,
    /// Ruleset-owned configuration for the song.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub settings: Settings,
    /// Non-ruleset plugins whose data appears in the document.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub plugins: BTreeMap<Namespace, Plugin>,
    /// Song-wide descriptive information and assets.
    pub metadata: Metadata,
    /// File-level bank of sound references.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub samples: BTreeMap<SampleId, ResourcePath>,
    /// Song timing inherited by charts without their own timing.
    pub timing: Timing,
    /// Song-wide navigation points.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<Marker>,
    /// Difficulties in the order they appear in the file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub charts: Vec<Chart>,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if passes a reference"
)]
/// Whether serde should omit the default resolution on writing.
fn is_default_ticks_per_beat(value: &TicksPerBeat) -> bool {
    *value == TicksPerBeat::default()
}

/// The one ruleset that defines what the file's charts mean.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ruleset {
    /// Namespace owning this song's charts.
    pub id: Namespace,
    /// Opaque to the core; the ruleset interprets it.
    pub version: String,
}

/// A non-ruleset plugin that left data in the file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plugin {
    /// Opaque to the core; the plugin interprets it.
    pub version: String,
}

/// Song credits, audio and optional descriptive assets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    /// Name shown for the song.
    pub title: String,
    /// Performer or creator credited for the music.
    pub artist: String,
    /// Audio resource to play with the chart.
    pub audio: ResourcePath,
    /// Excerpt used when previewing the song.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<Preview>,
    /// The song's picture, such as StepMania's jacket, osu!'s background or Tap Tap Revenge's
    /// artwork.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artwork: Option<ResourcePath>,
    /// The kind of music, as a song list sorts it, such as `Dance`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    /// Plugin-owned song metadata.
    pub ext: Extensions,
}

/// An excerpt of the audio, in audio time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preview {
    /// Where the excerpt begins in the audio.
    pub start_seconds: AudioTime,
    /// Excerpt duration; absent when playback runs to the end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length_seconds: Option<Seconds>,
}

/// A tick to time mapping: pinned beats, a tail tempo, and the pauses and meter events that
/// shape the ticks between them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Timing {
    /// Beats pinned to moments of audio time.
    pub anchors: Vec<Anchor>,
    /// Tempo following the last anchor.
    pub tempo: Bpm,
    /// Pauses, warps, meters and plugin timing data.
    pub events: Vec<TimingEvent>,
}

/// A beat pinned to a moment of the audio.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Anchor {
    /// Beat to pin on the grid.
    pub tick: Tick,
    /// Moment at which that beat plays, possibly before the audio starts.
    pub seconds: Origin,
}

/// A change to the timing map at a particular tick.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimingEvent {
    /// Tick at which this change applies.
    pub tick: Tick,
    /// Timing change or plugin-owned data.
    pub kind: TimingKind,
    /// Plugin-owned properties of the event.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ext: Extensions,
}

/// Timing changes interpreted by the core, or opaque plugin timing data.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TimingKind {
    /// Seconds of pause after the objects on this tick.
    Stop(Seconds),
    /// Seconds of pause before the objects on this tick.
    Delay(Seconds),
    /// Ticks skipped from this one on, taking no time.
    Warp(Ticks),
    /// The meter from this tick on.
    TimeSignature(TimeSignature),
    /// Plugin timing data the core carries and never interprets.
    Other {
        /// Plugin-qualified name of this timing kind.
        name: Key,
        /// Data interpreted by the owning plugin.
        value: Value,
    },
}

/// A colour token from the editor's layer palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerColour {
    /// Red palette entry.
    Red,
    /// Orange palette entry.
    Orange,
    /// Yellow palette entry.
    Yellow,
    /// Green palette entry.
    Green,
    /// Teal palette entry.
    Teal,
    /// Blue palette entry.
    Blue,
    /// Purple palette entry.
    Purple,
    /// Pink palette entry.
    Pink,
}

impl LayerColour {
    /// Palette in the order new layers receive colours.
    pub const ALL: [Self; 8] = [
        Self::Red,
        Self::Orange,
        Self::Yellow,
        Self::Green,
        Self::Teal,
        Self::Blue,
        Self::Purple,
        Self::Pink,
    ];

    /// First unused palette entry, cycling through the palette when all are used.
    #[must_use]
    pub fn next_unused(used: impl IntoIterator<Item = Self>) -> Self {
        let used: Vec<_> = used.into_iter().collect();
        Self::ALL
            .iter()
            .copied()
            .find(|colour| !used.contains(colour))
            .unwrap_or_else(|| {
                Self::ALL
                    .into_iter()
                    .cycle()
                    .nth(used.len())
                    .unwrap_or(Self::Red)
            })
    }
}

/// A layer's visibility and whether its notes can be edited.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerView {
    /// Full brightness, editable unless locked.
    #[default]
    Shown,
    /// Dim reference notes that cannot be selected or edited.
    Ghosted,
    /// Notes not drawn or editable.
    Hidden,
}

impl LayerView {
    /// Whether this layer is visible at full brightness.
    #[must_use]
    pub fn is_shown(&self) -> bool {
        *self == Self::Shown
    }
}

/// Named chart-local group of notes. Only shown, unlocked layers are editable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    /// Unique name within the chart.
    pub name: LayerName,
    /// Palette colour used by the editor.
    pub colour: LayerColour,
    /// How the editor displays this layer.
    #[serde(default, skip_serializing_if = "LayerView::is_shown")]
    pub view: LayerView,
    /// Whether the editor prevents edits on this layer.
    #[serde(default, skip_serializing_if = "is_false")]
    pub locked: bool,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if passes a reference"
)]
/// Whether serde omits the default unlocked state.
fn is_false(value: &bool) -> bool {
    !value
}

impl Layer {
    /// Whether its notes may be selected and changed.
    #[must_use]
    pub fn is_editable(&self) -> bool {
        self.view.is_shown() && !self.locked
    }
}

/// One difficulty of the file's ruleset.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chart {
    /// Difficulty name, unique within the document.
    pub name: String,
    /// Number of lanes; absent for a plane-only chart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lanes: Option<LaneCount>,
    /// Ruleset-owned configuration for this difficulty.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub settings: Settings,
    /// Replaces the song's timing entirely when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing: Option<Timing>,
    /// Ordered top-to-bottom layers; absent means an implicit Notes layer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<Layer>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    /// Notes in tick order.
    pub objects: Vec<Object>,
    /// Plugin-owned chart data.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ext: Extensions,
}

impl Chart {
    /// The layer holding `object`, defaulting to the first layer if unnamed.
    #[must_use]
    pub fn layer_index(&self, object: &Object) -> Option<usize> {
        if self.layers.is_empty() {
            None
        } else {
            object.layer.as_ref().map_or(Some(0), |name| {
                self.layers.iter().position(|layer| layer.name == *name)
            })
        }
    }
}

/// A note, possibly with a span and nested children.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Object {
    /// Starting tick of the note.
    pub tick: Tick,
    /// Lane span or point on the playfield.
    pub position: Position,
    /// Shared or plugin-defined note kind.
    pub kind: Kind,
    /// Duration or path after the starting tick.
    pub shape: Shape,
    /// Named layer holding the note, or the first layer when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerName>,
    /// Keysound from the file-level bank.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample: Option<SampleId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    /// Notes contained within this one's span.
    pub children: Vec<Child>,
    /// Plugin-owned properties of this note.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ext: Extensions,
}

string_newtype!(
    /// Key into the file-level sample bank: any non-empty text.
    SampleId, "a non-empty sample id", |v| !v.is_empty()
);

/// An object contained by another, with no children of its own.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Child {
    /// Starting tick within the parent's span.
    pub tick: Tick,
    /// Lane span or point on the playfield.
    pub position: Position,
    /// Shared or plugin-defined note kind.
    pub kind: Kind,
    /// Duration or path within the parent's span.
    pub shape: Shape,
    /// Keysound from the file-level bank.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample: Option<SampleId>,
    /// Plugin-owned properties of this child.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ext: Extensions,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if passes a reference"
)]
/// Whether serde should omit a one-lane `width` on writing.
fn is_one(value: &LaneCount) -> bool {
    *value == LaneCount::ONE
}

/// A note's location in lanes or on a continuous playfield.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Position {
    /// `width` lanes starting at `index`, counting from 0.
    Lane {
        /// First lane covered, counting from zero.
        index: LaneIndex,
        /// Consecutive lanes covered, defaulting to one.
        #[serde(default, skip_serializing_if = "is_one")]
        width: LaneCount,
    },
    /// A point on a continuous playfield, in units the ruleset defines.
    Plane {
        /// Horizontal coordinate in the ruleset's units.
        x: Decimal,
        /// Vertical coordinate in the ruleset's units.
        y: Decimal,
    },
}

impl Position {
    /// The lanes a `Lane` position covers; `None` for `Plane`.
    #[must_use]
    pub fn lanes(&self) -> Option<LaneSpan> {
        match *self {
            Self::Lane { index, width } => Some(LaneSpan::new(index, width)),
            Self::Plane { .. } => None,
        }
    }
}

/// Whether a note is instantaneous, sustained or follows a path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Shape {
    /// No duration beyond the starting tick.
    Instant,
    /// Ticks from the object's tick to its end.
    Sustain {
        /// Positive distance from the starting tick.
        duration: Ticks,
    },
    /// A sequence of points following the note's starting position.
    Path {
        /// Subsequent points in strictly increasing tick order.
        points: Points,
    },
}

impl Shape {
    /// The tick where an object starting at `tick` with this shape ends, or `None` past the last
    /// tick.
    #[must_use]
    pub fn end(&self, tick: Tick) -> Option<Tick> {
        match self {
            Self::Instant => Some(tick),
            Self::Sustain { duration } => tick.checked_add(*duration),
            Self::Path { points } => Some(points.last().tick),
        }
    }
}

/// The points of a path after the object's own position. Never empty.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<PathPoint>")]
pub struct Points(Vec<PathPoint>);

impl Points {
    /// Wraps a non-empty list of path points.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidValue`] if `points` is empty.
    pub fn new(points: Vec<PathPoint>) -> Result<Self, InvalidValue> {
        if points.is_empty() {
            Err(InvalidValue::new("at least one path point", "none"))
        } else {
            Ok(Self(points))
        }
    }

    /// The points following the object's starting position.
    #[must_use]
    pub fn as_slice(&self) -> &[PathPoint] {
        &self.0
    }

    /// # Panics
    ///
    /// Never: `new` refuses an empty list, and nothing else builds one.
    #[must_use]
    #[expect(clippy::expect_used, reason = "points are nonempty by construction")]
    pub fn last(&self) -> &PathPoint {
        self.0.last().expect("points are never empty")
    }
}

impl TryFrom<Vec<PathPoint>> for Points {
    type Error = InvalidValue;

    fn try_from(points: Vec<PathPoint>) -> Result<Self, Self::Error> {
        Self::new(points)
    }
}

/// One later point of an object's path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathPoint {
    /// Tick after the previous path point.
    pub tick: Tick,
    /// Position reached at this tick.
    pub position: Position,
    /// Shapes the segment arriving at this point.
    pub ease: Ease,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    /// Plugin-owned properties of this point.
    pub ext: Extensions,
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn minimal() -> Value {
        json!({
            "format_version": 1,
            "ruleset": { "id": "test", "version": "1" },
            "metadata": { "title": "t", "artist": "a", "audio": "song.ogg" },
            "timing": {
                "anchors": [ { "tick": 0, "seconds": 0.0 } ],
                "tempo": 120.0,
                "events": [ { "tick": 0, "kind": { "time_signature": [4, 4] } } ]
            }
        })
    }

    fn with(edit: impl FnOnce(&mut Value)) -> Value {
        let mut value = minimal();
        edit(&mut value);
        value
    }

    fn parse(value: &Value) -> Result<Document, serde_json::Error> {
        Document::deserialize(value)
    }

    /// Reads `value` as a document and writes it back.
    fn round_trip(value: &Value) -> Value {
        serde_json::to_value(parse(value).unwrap()).unwrap()
    }

    fn plane(x: f64, y: f64) -> Value {
        json!({ "plane": { "x": x, "y": y } })
    }

    /// A minimal document with one chart holding `objects`.
    fn chart_of(objects: Value) -> Value {
        let mut chart = json!({ "name": "Easy" });
        chart["objects"] = objects;
        with(|v| v["charts"] = json!([chart]))
    }

    #[test]
    fn a_missing_resolution_defaults_to_960_ticks_per_beat() {
        assert_eq!(
            parse(&minimal()).unwrap().ticks_per_beat,
            TicksPerBeat::new(960).unwrap()
        );
    }

    #[test]
    fn writing_omits_defaults_and_keeps_every_value() {
        assert_eq!(round_trip(&minimal()), minimal());
    }

    #[test]
    fn layers_round_trip_and_default_views_locks_and_unnamed_notes_are_omitted() {
        let plain = chart_of(json!([{"tick":0,"position":{"lane":{"index":0}},
            "kind":"tap","shape":"instant"}]));
        assert_eq!(round_trip(&plain), plain);
        let mut layered = plain;
        layered["charts"][0]["layers"] = json!([
            {"name":"Kick","colour":"red"},
            {"name":"Snare","colour":"blue","view":"ghosted","locked":true}
        ]);
        layered["charts"][0]["objects"][0]["layer"] = json!("Kick");
        assert_eq!(round_trip(&layered), layered);
        let chart = &parse(&layered).unwrap().charts[0];
        assert_eq!(chart.layer_index(&chart.objects[0]), Some(0));
        assert!(!chart.layers[1].is_editable());
        assert!(chart.layers[0].is_editable());
        let mut no_name = chart.objects[0].clone();
        no_name.layer = None;
        assert_eq!(chart.layer_index(&no_name), Some(0));
        parse(
            &json!({"format_version":1,"ruleset":{"id":"test","version":"1"},
            "metadata":{"title":"t","artist":"a","audio":"song.ogg"},
            "timing":{"anchors":[{"tick":0,"seconds":0.0}],"tempo":120.0,"events":[]},
            "charts":[{"name":"Easy","layers":[{"name":"X","colour":"red","unexpected":true}]}]}),
        )
        .unwrap_err();
    }

    #[test]
    fn a_new_layer_uses_the_first_unused_palette_colour_then_cycles() {
        assert_eq!(
            LayerColour::next_unused([LayerColour::Red, LayerColour::Yellow]),
            LayerColour::Orange
        );
        assert_eq!(LayerColour::next_unused(LayerColour::ALL), LayerColour::Red);
        assert_eq!(
            LayerColour::next_unused(LayerColour::ALL.into_iter().chain([LayerColour::Red])),
            LayerColour::Orange
        );
        assert_eq!(LayerColour::ALL.len(), 8);
        assert_eq!(
            serde_json::to_value(LayerColour::Teal).unwrap(),
            json!("teal")
        );
    }

    #[test]
    fn unknown_keys_are_rejected_everywhere_in_the_core_schema() {
        let places: [fn(&mut Value) -> &mut Value; 5] = [
            |v| v,
            |v| &mut v["ruleset"],
            |v| &mut v["metadata"],
            |v| &mut v["timing"],
            |v| &mut v["timing"]["events"][0],
        ];
        for place in places {
            let mut value = minimal();
            place(&mut value)["from_the_future"] = json!(true);
            assert!(parse(&value).is_err(), "{value}");
        }
    }

    #[test]
    fn every_timing_kind_survives_a_round_trip() {
        let value = with(|v| {
            let events = v["timing"]["events"].as_array_mut().unwrap();
            for kind in [
                json!({ "stop": 0.5 }),
                json!({ "delay": 0.25 }),
                json!({ "warp": 480 }),
                json!({ "time_signature": [7, 8] }),
                json!({ "other": { "name": "swing.groove", "value": { "amount": 0.6 } } }),
            ] {
                events.push(json!({ "tick": 960, "kind": kind }));
            }
        });
        assert_eq!(round_trip(&value), value);
    }

    #[test]
    fn charts_of_lane_objects_survive_a_round_trip_without_default_widths() {
        let value = chart_of(json!([
            { "tick": 0, "position": { "lane": { "index": 0 } }, "kind": "tap", "shape": "instant" },
            { "tick": 480, "position": { "lane": { "index": 1, "width": 2 } },
              "kind": "hold", "shape": { "sustain": { "duration": 240 } } }
        ]));
        assert_eq!(round_trip(&value), value);
    }

    #[test]
    fn plane_paths_with_eased_points_survive_a_round_trip() {
        let value = chart_of(json!([{
            "tick": 960, "position": plane(0.5, 0.0), "kind": "test.arc",
            "shape": { "path": { "points": [
                { "tick": 1920, "position": plane(0.75, 1.0), "ease": "test.sine" }
            ] } }
        }]));
        assert_eq!(round_trip(&value), value);
    }

    #[test]
    fn a_path_needs_at_least_one_point() {
        let value = chart_of(
            json!([{ "tick": 0, "position": plane(0.0, 0.0), "kind": "tap",
                                      "shape": { "path": { "points": [] } } }]),
        );
        parse(&value).unwrap_err();
    }

    #[test]
    fn children_survive_a_round_trip() {
        let value = chart_of(json!([{
            "tick": 960, "position": plane(0.5, 0.0), "kind": "tap",
            "shape": { "sustain": { "duration": 960 } },
            "children": [ { "tick": 1440, "position": plane(0.6, 0.5), "kind": "tap", "shape": "instant" } ]
        }]));
        assert_eq!(round_trip(&value), value);
    }

    #[test]
    fn children_cannot_have_children() {
        let leaf =
            json!({ "tick": 0, "position": plane(0.0, 0.0), "kind": "tap", "shape": "instant" });
        let mut child = leaf.clone();
        child["children"] = json!([leaf]);
        let mut parent = leaf;
        parent["children"] = json!([child]);
        parse(&chart_of(json!([parent]))).unwrap_err();
    }

    #[test]
    fn keysounds_and_their_bank_survive_a_round_trip() {
        let mut value = chart_of(json!([{ "tick": 0, "position": { "lane": { "index": 0 } },
                                          "kind": "tap", "shape": "instant", "sample": "kick" }]));
        value["samples"] = json!({ "kick": "keysounds/kick.wav" });
        assert_eq!(round_trip(&value), value);
    }

    #[test]
    fn previews_survive_a_round_trip_with_or_without_a_length() {
        for preview in [
            json!({ "start_seconds": 42.5, "length_seconds": 15.0 }),
            json!({ "start_seconds": 0.0 }),
        ] {
            let value = with(|v| v["metadata"]["preview"] = preview);
            assert_eq!(round_trip(&value), value);
        }
    }

    #[test]
    fn artwork_and_genre_survive_a_round_trip() {
        let value = with(|v| {
            v["metadata"]["artwork"] = json!("artwork.png");
            v["metadata"]["genre"] = json!("Dance");
        });
        assert_eq!(round_trip(&value), value);
        parse(&with(|v| v["metadata"]["artwork"] = json!("../art.png"))).unwrap_err();
    }

    #[test]
    fn plugin_declarations_survive_a_round_trip() {
        let value = with(|v| v["plugins"] = json!({ "swing": { "version": "0.2.0" } }));
        assert_eq!(round_trip(&value), value);
    }

    #[test]
    fn ruleset_settings_survive_a_round_trip_on_the_file_and_each_chart() {
        let value = with(|v| {
            v["settings"] = json!({ "sky_input": true });
            v["charts"] =
                json!([{ "name": "Future", "settings": { "rating": 10, "style": "touch" } }]);
        });
        assert_eq!(round_trip(&value), value);
    }

    #[test]
    fn a_charts_own_timing_survives_a_round_trip() {
        let own = json!({
            "anchors": [ { "tick": 0, "seconds": 0.5 } ],
            "tempo": 90.0,
            "events": []
        });
        let value = with(|v| v["charts"] = json!([{ "name": "Beyond", "timing": own }]));
        assert_eq!(round_trip(&value), value);
    }

    #[test]
    fn extension_data_survives_a_round_trip_wherever_the_format_allows_it() {
        let ext = json!({ "test.note": { "any": ["opaque", 1] } });
        let value = with(|v| {
            v["metadata"]["ext"] = ext.clone();
            v["timing"]["events"][0]["ext"] = ext.clone();
            v["samples"] = json!({ "kick": "kick.wav" });
            v["charts"] = json!([{
                "name": "Future",
                "ext": ext,
                "objects": [{
                    "tick": 0, "position": plane(0.0, 0.0), "kind": "tap", "ext": ext,
                    "shape": { "path": { "points": [
                        { "tick": 960, "position": plane(1.0, 1.0), "ease": "linear", "ext": ext }
                    ] } },
                    "children": [ { "tick": 480, "position": plane(0.5, 0.5), "kind": "tap",
                                    "shape": "instant", "sample": "kick", "ext": ext } ]
                }]
            }]);
        });
        assert_eq!(round_trip(&value), value);
    }

    #[test]
    fn a_lane_position_covers_its_width_from_its_index() {
        let lanes =
            |index, width| LaneSpan::new(LaneIndex::new(index), LaneCount::new(width).unwrap());
        let wide = Position::Lane {
            index: LaneIndex::new(2),
            width: LaneCount::new(3).unwrap(),
        };
        assert_eq!(wide.lanes(), Some(lanes(2, 3)));
        let point = Position::Plane {
            x: Decimal::try_from(0.5).unwrap(),
            y: Decimal::try_from(0.5).unwrap(),
        };
        assert_eq!(point.lanes(), None);
    }

    #[test]
    fn a_shape_ends_where_its_span_does_or_nowhere_past_the_last_tick() {
        assert_eq!(Shape::Instant.end(Tick::new(-5)), Some(Tick::new(-5)));
        let longest = Shape::Sustain {
            duration: Ticks::new(u32::MAX).unwrap(),
        };
        assert_eq!(longest.end(Tick::new(i32::MIN)), Some(Tick::new(i32::MAX)));
        assert_eq!(longest.end(Tick::new(i32::MIN + 1)), None);
        let point = |tick| PathPoint {
            tick,
            position: Position::Lane {
                index: LaneIndex::new(0),
                width: LaneCount::ONE,
            },
            ease: Ease::Linear,
            ext: Extensions::new(),
        };
        let path = Shape::Path {
            points: Points::new(vec![point(Tick::new(10)), point(Tick::new(40))]).unwrap(),
        };
        assert_eq!(path.end(Tick::new(0)), Some(Tick::new(40)));
    }

    #[test]
    fn sample_ids_are_non_empty_text_that_shows_as_itself() {
        assert_eq!(
            SampleId::new("kick").map(|id| id.to_string()),
            Ok("kick".to_owned())
        );
        SampleId::new("").unwrap_err();
        let empty = with(|v| v["samples"] = json!({ "": "keysounds/kick.wav" }));
        parse(&empty).unwrap_err();
    }

    #[test]
    fn markers_of_every_kind_survive_a_round_trip() {
        let value = with(|v| {
            v["markers"] = json!([
                { "tick": 0, "kind": "section", "text": "Intro" },
                { "tick": 3840, "kind": "bookmark", "text": "" },
                { "tick": 3840, "kind": "test.fever", "text": "Fever" }
            ]);
        });
        assert_eq!(round_trip(&value), value);
        let kinds: Vec<MarkerKind> = parse(&value)
            .unwrap()
            .markers
            .into_iter()
            .map(|m| m.kind)
            .collect();
        assert_eq!(kinds[..2], [MarkerKind::Section, MarkerKind::Bookmark]);
    }
}
