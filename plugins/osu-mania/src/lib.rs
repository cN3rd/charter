//! osu!mania as a charter ruleset.
//!
//! A chart's lane count is its key count. Notes are taps and holds. Timing is BPM, meter in
//! beats per measure, and scroll speed changes; osu! has no stops, delays or warps.
//!
//! [`Song`] is the typed view of a document whose ruleset is [`ID`]. Reading one fails on
//! anything osu!mania cannot represent and skips data other plugins left behind;
//! [`Song::to_document`] writes this ruleset's data alone. A song is checked when built, by
//! reading or by [`Song::new`], so writing one cannot fail. [`Plugin`] hands the editor the same
//! check, so a document is editable only while it reads as a song.

use std::collections::BTreeMap;
use std::num::NonZeroU16;

use charter_core::{
    Anchor, Bpm, Chart as CoreChart, Decimal, Document, Extensions, FORMAT_VERSION, InvalidValue,
    Key, Kind, LaneCount, LaneIndex, Layout, Location, Marker, Metadata, Name, Namespace, NewSong,
    Object, Origin, Position, ResourcePath, Ruleset, RulesetPlugin, SampleId, Settings, Shape,
    Stream, Tick, Ticks, TicksPerBeat, TimeSignature, Timing as CoreTiming,
    TimingEvent as CoreEvent, TimingKind as CoreKind, ValidDocument,
};
use serde::{Deserialize, Serialize, de, de::IntoDeserializer};
use serde_json::Value;

mod import;

#[expect(
    clippy::pub_use,
    clippy::useless_attribute,
    reason = "the plugin exposes its timing import error beside its song and plugin types"
)]
pub use import::ImportError;

/// The ruleset namespace.
pub const ID: &str = "osu_mania";

/// A song, checked against this ruleset and every document rule when built, and unchangeable
/// after.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Song {
    /// Header values shared by all the song's beatmaps.
    metadata: Metadata,
    /// Number of ticks per quarter-note beat.
    ticks_per_beat: TicksPerBeat,
    /// Keysounds available to notes in every beatmap.
    samples: BTreeMap<SampleId, ResourcePath>,
    /// Song-wide timing, used unless a beatmap overrides it.
    timing: Timing,
    /// Song-wide section and bookmark positions.
    markers: Vec<Marker>,
    /// Beatmaps checked against this ruleset.
    charts: Vec<Chart>,
}

/// Song timing with osu!mania's inherited scroll-speed points.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Timing {
    /// Audio positions pinned to chart ticks.
    pub anchors: Vec<Anchor>,
    /// Starting tempo.
    pub tempo: Bpm,
    /// Metre and scroll-speed changes.
    pub events: Vec<TimingEvent>,
}

/// One change to the timing grid or scroll speed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimingEvent {
    /// Position of the change.
    pub tick: Tick,
    /// Effect of the change.
    pub kind: TimingKind,
}

/// Changes osu!mania timing supports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimingKind {
    /// Beats per measure. osu! meters are always in quarter notes.
    Meter(NonZeroU16),
    /// Scroll speed multiplier, osu!'s slider velocity on an inherited timing point.
    ScrollSpeed(ScrollSpeed),
}

/// One difficulty, which osu! calls a beatmap's version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chart {
    /// Beatmap version name.
    pub name: String,
    /// Key count fixing the lane count.
    pub keys: Keys,
    /// HP drain rate.
    pub hp: Difficulty,
    /// Overall difficulty, which sets the timing windows.
    pub od: Difficulty,
    /// Replaces the song's timing when present. osu! stores timing per beatmap.
    pub timing: Option<Timing>,
    /// Placed taps and holds.
    pub notes: Vec<Note>,
}

/// A key count osu!mania plays: 1 to 10 on one stage, or an even count up to 20 split across
/// two; see `MAX_STAGE_KEYS` in osu!lazer's `ManiaRuleset`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keys {
    /// One key.
    K1,
    /// Two keys.
    K2,
    /// Three keys.
    K3,
    /// Four keys.
    K4,
    /// Five keys.
    K5,
    /// Six keys.
    K6,
    /// Seven keys.
    K7,
    /// Eight keys.
    K8,
    /// Nine keys.
    K9,
    /// Ten keys.
    K10,
    /// Twelve keys on two stages.
    K12,
    /// Fourteen keys on two stages.
    K14,
    /// Sixteen keys on two stages.
    K16,
    /// Eighteen keys on two stages.
    K18,
    /// Twenty keys on two stages.
    K20,
}

impl Keys {
    /// Supported key counts, including two-stage even counts.
    pub const ALL: [Self; 15] = [
        Self::K1,
        Self::K2,
        Self::K3,
        Self::K4,
        Self::K5,
        Self::K6,
        Self::K7,
        Self::K8,
        Self::K9,
        Self::K10,
        Self::K12,
        Self::K14,
        Self::K16,
        Self::K18,
        Self::K20,
    ];

    /// Number of lanes for this key count.
    #[must_use]
    pub const fn lanes(self) -> LaneCount {
        lanes(match self {
            Self::K1 => 1,
            Self::K2 => 2,
            Self::K3 => 3,
            Self::K4 => 4,
            Self::K5 => 5,
            Self::K6 => 6,
            Self::K7 => 7,
            Self::K8 => 8,
            Self::K9 => 9,
            Self::K10 => 10,
            Self::K12 => 12,
            Self::K14 => 14,
            Self::K16 => 16,
            Self::K18 => 18,
            Self::K20 => 20,
        })
    }

    /// The key count with `lanes` lanes, if osu!mania plays one.
    #[must_use]
    pub fn of(lanes: LaneCount) -> Option<Self> {
        Self::ALL.into_iter().find(|keys| keys.lanes() == lanes)
    }
}

/// `n` lanes, for the key counts this ruleset names.
const fn lanes(n: u16) -> LaneCount {
    #[expect(
        clippy::unreachable,
        reason = "only positive constant key counts call this"
    )]
    match LaneCount::new(n) {
        Some(lanes) => lanes,
        None => unreachable!(),
    }
}

/// A 0 to 10 difficulty setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Decimal")]
pub struct Difficulty(Decimal);

impl TryFrom<Decimal> for Difficulty {
    type Error = String;

    fn try_from(value: Decimal) -> Result<Self, String> {
        Self::new(value).ok_or_else(|| format!("expected a difficulty from 0 to 10, found {value}"))
    }
}

/// A scroll speed multiplier above 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Decimal")]
pub struct ScrollSpeed(Decimal);

impl ScrollSpeed {
    /// A positive scroll multiplier, or `None` for zero or less.
    #[must_use]
    pub fn new(value: Decimal) -> Option<Self> {
        (value > whole(0)).then_some(Self(value))
    }
}

impl TryFrom<Decimal> for ScrollSpeed {
    type Error = String;

    fn try_from(value: Decimal) -> Result<Self, String> {
        Self::new(value).ok_or_else(|| format!("expected a scroll speed above 0, found {value}"))
    }
}

/// `n` as a decimal constant.
const fn whole(n: i64) -> Decimal {
    #[expect(
        clippy::unreachable,
        reason = "only small constant difficulties call this"
    )]
    match Decimal::from_millionths(n.saturating_mul(1_000_000)) {
        Some(value) => value,
        None => unreachable!(),
    }
}

/// Chart settings as stored in the document.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChartSettings {
    /// HP drain rate; defaults to osu!'s standard value.
    #[serde(default)]
    hp: Difficulty,
    /// Overall difficulty used for judgement windows.
    #[serde(default)]
    od: Difficulty,
}

impl Default for Difficulty {
    /// 5, osu!'s value when a beatmap does not set one.
    fn default() -> Self {
        Self(whole(5))
    }
}

impl Difficulty {
    /// A difficulty within osu!'s inclusive 0 to 10 range.
    #[must_use]
    pub fn new(value: Decimal) -> Option<Self> {
        (whole(0)..=whole(10))
            .contains(&value)
            .then_some(Self(value))
    }
}

/// One placed tap or hold.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    /// Start position in chart ticks.
    pub tick: Tick,
    /// Column containing the note.
    pub column: LaneIndex,
    /// Ticks held, for a hold note.
    pub hold: Option<Ticks>,
    /// Optional sample to play with the note.
    pub keysound: Option<SampleId>,
}

/// Ruleset key for an inherited timing point's slider velocity.
const SCROLL_SPEED: &str = "scroll_speed";

/// Quarter-note denominator for osu! meters.
const FOUR: NonZeroU16 = NonZeroU16::MIN.saturating_add(3);

/// Written as the ruleset version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

impl Song {
    /// A song of `charts` with the keysound bank `samples`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnknownKey`] for metadata extension data, which this ruleset does not
    /// keep, or [`Error::Document`] for the first document rule the song breaks, such as a note
    /// in a lane its chart does not have, or a keysound missing from the bank.
    pub fn new(
        metadata: Metadata,
        ticks_per_beat: TicksPerBeat,
        samples: BTreeMap<SampleId, ResourcePath>,
        timing: Timing,
        markers: Vec<Marker>,
        charts: Vec<Chart>,
    ) -> Result<Self, Error> {
        let own = markers
            .iter()
            .enumerate()
            .find_map(|(m, marker)| marker.kind.key_owned_by(ID).map(|key| (m, key)));
        if let Some((m, key)) = own {
            return Err(Error::UnknownKey {
                at: Location::Marker(m),
                key: key.clone(),
            });
        }
        if let Some(key) = metadata.ext.keys().next() {
            return Err(Error::UnknownKey {
                at: Location::MetadataExt,
                key: key.clone(),
            });
        }
        let song = Self {
            metadata,
            ticks_per_beat,
            samples,
            timing,
            markers,
            charts,
        };
        song.build()?;
        Ok(song)
    }

    /// Header values shared by all beatmaps.
    #[must_use]
    pub const fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// Number of ticks per quarter-note beat.
    #[must_use]
    pub const fn ticks_per_beat(&self) -> TicksPerBeat {
        self.ticks_per_beat
    }

    /// Keysounds.
    #[must_use]
    pub const fn samples(&self) -> &BTreeMap<SampleId, ResourcePath> {
        &self.samples
    }

    /// Song-wide timing, before beatmap overrides.
    #[must_use]
    pub const fn timing(&self) -> &Timing {
        &self.timing
    }

    /// Beatmaps in this song.
    #[must_use]
    pub fn charts(&self) -> &[Chart] {
        &self.charts
    }

    /// Song-wide markers of the shared kinds.
    #[must_use]
    pub fn markers(&self) -> &[Marker] {
        &self.markers
    }

    /// Writes the song as a document holding only this ruleset's data.
    ///
    /// # Panics
    ///
    /// Never: [`Song::new`] checked the same document, and a song cannot change.
    #[must_use]
    #[expect(clippy::expect_used, reason = "Song::new checked the immutable song")]
    pub fn to_document(&self) -> ValidDocument {
        self.build().expect("a song was checked when built")
    }

    /// Validates and writes this song using only osu!mania data.
    fn build(&self) -> Result<ValidDocument, Error> {
        let charts = self
            .charts
            .iter()
            .map(Chart::to_core)
            .collect::<Result<_, _>>()?;
        let doc = Document {
            format_version: FORMAT_VERSION,
            ticks_per_beat: self.ticks_per_beat,
            ruleset: Ruleset {
                id: Namespace::new(ID)?,
                version: VERSION.to_owned(),
            },
            settings: Settings::new(),
            plugins: BTreeMap::new(),
            metadata: self.metadata.clone(),
            samples: self.samples.clone(),
            timing: self.timing.to_core()?,
            markers: self.markers.clone(),
            charts,
        };
        Ok(ValidDocument::new(doc)?)
    }
}

impl Timing {
    /// Writes meters and scroll changes as shared timing events.
    fn to_core(&self) -> Result<CoreTiming, Error> {
        let events = self.events.iter().map(|event| {
            let kind = match event.kind {
                TimingKind::Meter(beats) => CoreKind::TimeSignature(TimeSignature {
                    numerator: beats,
                    denominator: FOUR,
                }),
                TimingKind::ScrollSpeed(ratio) => CoreKind::Other {
                    name: Key::new(Namespace::new(ID)?, Name::new(SCROLL_SPEED)?),
                    value: serde_json::to_value(ratio)?,
                },
            };
            Ok(CoreEvent {
                tick: event.tick,
                kind,
                ext: Extensions::new(),
            })
        });
        Ok(CoreTiming {
            anchors: self.anchors.clone(),
            tempo: self.tempo,
            events: events.collect::<Result<_, Error>>()?,
        })
    }
}

impl Chart {
    /// Writes the beatmap's difficulty settings, key count and notes.
    fn to_core(&self) -> Result<CoreChart, Error> {
        let settings = serde_json::to_value(ChartSettings {
            hp: self.hp,
            od: self.od,
        })?;
        let settings = match settings {
            Value::Object(map) => map.into_iter().collect(),
            Value::Null
            | Value::Bool(_)
            | Value::Number(_)
            | Value::String(_)
            | Value::Array(_) => Settings::new(),
        };
        Ok(CoreChart {
            name: self.name.clone(),
            lanes: Some(self.keys.lanes()),
            settings,
            timing: self.timing.as_ref().map(Timing::to_core).transpose()?,
            // osu!mania has no layers; writing its game model exports the merged chart.
            layers: Vec::new(),
            objects: self.notes.iter().map(Note::to_object).collect(),
            ext: Extensions::new(),
        })
    }
}

impl Note {
    /// Writes a one-lane tap or hold, retaining its optional keysound.
    fn to_object(&self) -> Object {
        let (kind, shape) = match self.hold {
            None => (Kind::Tap, Shape::Instant),
            Some(duration) => (Kind::Hold, Shape::Sustain { duration }),
        };
        Object {
            tick: self.tick,
            position: Position::Lane {
                index: self.column,
                width: LaneCount::ONE,
            },
            kind,
            shape,
            // Layer identity cannot be recovered from an osu!mania hit object.
            layer: None,
            sample: self.keysound.clone(),
            children: Vec::new(),
            ext: Extensions::new(),
        }
    }
}

/// Why a document cannot be represented as an osu!mania song.
#[expect(
    clippy::error_impl_error,
    reason = "public error type is part of the plugin API"
)]
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Document belongs to another ruleset.
    #[error("ruleset is \"{0}\", not {ID:?}")]
    WrongRuleset(Namespace),
    /// Beatmap has an unsupported key count.
    #[error("{at}: osu!mania does not play {} keys", .found.map_or_else(|| "no".to_owned(), |lanes| lanes.to_string()))]
    Keys {
        /// Location of the beatmap.
        at: Location,
        /// Key count, if specified.
        found: Option<LaneCount>,
    },
    /// Unsupported document feature.
    #[error("{at}: {what} are not part of osu!mania")]
    Unsupported {
        /// Location of the feature.
        at: Location,
        /// Name of the feature.
        what: &'static str,
    },
    /// Note shape or position this game cannot play.
    #[error("{at}: not an osu!mania note")]
    UnsupportedObject {
        /// Location of the note.
        at: Location,
    },
    /// Unrecognised plugin key.
    #[error("{at}: \"{key}\" is not an osu!mania key")]
    UnknownKey {
        /// Location of the key.
        at: Location,
        /// Unrecognised key.
        key: Key,
    },
    /// Unrecognised chart or song setting.
    #[error("{at}: \"{key}\" is not an osu!mania setting")]
    UnknownSetting {
        /// Location of the setting.
        at: Location,
        /// Unrecognised setting.
        key: String,
    },
    /// Malformed plugin-specific value.
    #[error("{at}: {source}")]
    Malformed {
        /// Location of the value.
        at: Location,
        /// JSON conversion failure.
        source: serde_json::Error,
    },
    /// Invalid format-level value.
    #[error(transparent)]
    Value(#[from] InvalidValue),
    /// JSON conversion failure.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Invalid chart document.
    #[error(transparent)]
    Document(#[from] charter_core::Error),
}

impl TryFrom<&ValidDocument> for Song {
    type Error = Error;

    fn try_from(valid: &ValidDocument) -> Result<Self, Error> {
        let doc = valid.document();
        if doc.ruleset.id.as_str() != ID {
            return Err(Error::WrongRuleset(doc.ruleset.id.clone()));
        }
        if let Some(key) = doc.settings.keys().next() {
            return Err(Error::UnknownSetting {
                at: Location::Settings,
                key: key.clone(),
            });
        }
        reject_own_keys(&doc.metadata.ext, Location::MetadataExt)?;
        let charts = doc
            .charts
            .iter()
            .enumerate()
            .map(|(c, chart)| read_chart(chart, c))
            .collect::<Result<_, _>>()?;
        let metadata = Metadata {
            ext: Extensions::new(),
            ..doc.metadata.clone()
        };
        let markers = doc
            .markers
            .iter()
            .filter(|marker| marker.kind.is_shared_or_owned_by(ID))
            .cloned()
            .collect();
        let timing = read_timing(&doc.timing, Stream::Song)?;
        Self::new(
            metadata,
            doc.ticks_per_beat,
            doc.samples.clone(),
            timing,
            markers,
            charts,
        )
    }
}

/// This ruleset for the editor, which checks a document by reading it as a [`Song`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Plugin;

#[expect(
    clippy::missing_trait_methods,
    reason = "osu!mania uses the trait's defaults for `note_changes`, `change_note`, `difficulties` and `text_settings`: it offers no note changes, difficulty presets or text settings"
)]
impl RulesetPlugin for Plugin {
    fn id(&self) -> &str {
        ID
    }

    fn check(
        &self,
        document: &ValidDocument,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Song::try_from(document)?;
        Ok(())
    }

    fn name(&self) -> &'static str {
        "osu!mania"
    }

    fn layout_kind(&self) -> &'static str {
        "Keys"
    }

    fn timing_format(&self) -> Option<&str> {
        Some("osu! timing points")
    }

    fn import_timing(
        &self,
        text: &str,
        into: &ValidDocument,
    ) -> Result<charter_core::ImportedTiming, Box<dyn std::error::Error + Send + Sync>> {
        Ok(import::import(text, into)?)
    }

    fn layouts(&self) -> Vec<Layout> {
        NEW_SONG_KEYS
            .iter()
            .map(|keys| Layout {
                name: format!("{}K", keys.lanes()),
                lanes: keys.lanes(),
            })
            .collect()
    }

    /// Every chart gets the keys and HP and OD of 5.
    fn new_song(
        &self,
        song: &NewSong,
    ) -> Result<ValidDocument, Box<dyn std::error::Error + Send + Sync>> {
        let keys = *NEW_SONG_KEYS
            .get(song.layout)
            .ok_or("osu!mania has no such key count")?;
        let middling = Difficulty::new(whole(5)).ok_or("5 is a difficulty")?;
        let charts = song
            .charts
            .iter()
            .map(|chart| Chart {
                name: chart.name.clone(),
                keys,
                hp: middling,
                od: middling,
                timing: None,
                notes: Vec::new(),
            })
            .collect();
        let at_zero = |kind| TimingEvent {
            tick: Tick::new(0),
            kind,
        };
        let four = NonZeroU16::new(4).ok_or("4 is not zero")?;
        let timing = Timing {
            anchors: vec![Anchor {
                tick: Tick::new(0),
                seconds: Origin::try_from(0.0)?,
            }],
            tempo: Bpm::try_from(120.0)?,
            events: vec![at_zero(TimingKind::Meter(four))],
        };
        let song = Song::new(
            song.metadata.clone(),
            TicksPerBeat::default(),
            BTreeMap::new(),
            timing,
            Vec::new(),
            charts,
        )?;
        Ok(song.to_document())
    }
}

/// The key counts a new song can have, the usual one first.
const NEW_SONG_KEYS: [Keys; 15] = [
    Keys::K4,
    Keys::K1,
    Keys::K2,
    Keys::K3,
    Keys::K5,
    Keys::K6,
    Keys::K7,
    Keys::K8,
    Keys::K9,
    Keys::K10,
    Keys::K12,
    Keys::K14,
    Keys::K16,
    Keys::K18,
    Keys::K20,
];

/// Reads beatmap `c`, checking its settings and supported key count.
fn read_chart(chart: &CoreChart, c: usize) -> Result<Chart, Error> {
    reject_own_keys(&chart.ext, Location::ChartExt(c))?;
    let keys = chart.lanes.and_then(Keys::of).ok_or(Error::Keys {
        at: Location::Chart(c),
        found: chart.lanes,
    })?;
    let settings = de::value::MapDeserializer::<_, serde_json::Error>::new(
        chart
            .settings
            .iter()
            .map(|(key, value)| (key.as_str().into_deserializer(), value)),
    );
    let settings = ChartSettings::deserialize(settings).map_err(|source| Error::Malformed {
        at: Location::ChartSettings(c),
        source,
    })?;
    let notes = chart
        .objects
        .iter()
        .enumerate()
        .map(|(j, object)| read_note(object, Location::Object { chart: c, index: j }))
        .collect::<Result<_, _>>()?;
    Ok(Chart {
        name: chart.name.clone(),
        keys,
        hp: settings.hp,
        od: settings.od,
        timing: chart
            .timing
            .as_ref()
            .map(|timing| read_timing(timing, Stream::Chart(c)))
            .transpose()?,
        notes,
    })
}

/// A single-lane tap or hold at `at`, rejecting other note shapes.
fn read_note(object: &Object, at: Location) -> Result<Note, Error> {
    if !object.children.is_empty() {
        return Err(Error::Unsupported {
            at,
            what: "child objects",
        });
    }
    reject_own_keys(&object.ext, at)?;
    let unsupported = || Error::UnsupportedObject { at };
    let Position::Lane { index, width } = object.position else {
        return Err(unsupported());
    };
    if width != LaneCount::ONE {
        return Err(unsupported());
    }
    let hold = match (&object.kind, &object.shape) {
        (Kind::Tap, Shape::Instant) => None,
        (Kind::Hold, &Shape::Sustain { duration }) => Some(duration),
        _ => return Err(unsupported()),
    };
    Ok(Note {
        tick: object.tick,
        column: index,
        hold,
        keysound: object.sample.clone(),
    })
}

/// Reads quarter-note meters and scroll changes in `stream`, rejecting unsupported timing.
fn read_timing(timing: &CoreTiming, stream: Stream) -> Result<Timing, Error> {
    let mut events = Vec::with_capacity(timing.events.len());
    for (index, event) in timing.events.iter().enumerate() {
        let at = Location::Event { stream, index };
        reject_own_keys(&event.ext, at)?;
        let unsupported = |what| Error::Unsupported { at, what };
        let kind = match &event.kind {
            CoreKind::TimeSignature(TimeSignature {
                numerator,
                denominator: FOUR,
            }) => TimingKind::Meter(*numerator),
            CoreKind::TimeSignature(..) => return Err(unsupported("meters outside quarter notes")),
            CoreKind::Stop(_) => return Err(unsupported("stops")),
            CoreKind::Delay(_) => return Err(unsupported("delays")),
            CoreKind::Warp(_) => return Err(unsupported("warps")),
            CoreKind::Other { name, value }
                if name.is_owned_by(ID) && name.name().as_str() == SCROLL_SPEED =>
            {
                TimingKind::ScrollSpeed(
                    ScrollSpeed::deserialize(value)
                        .map_err(|source| Error::Malformed { at, source })?,
                )
            }
            CoreKind::Other { name, .. } if name.is_owned_by(ID) => {
                return Err(Error::UnknownKey {
                    at,
                    key: name.clone(),
                });
            }
            CoreKind::Other { .. } => continue,
        };
        events.push(TimingEvent {
            tick: event.tick,
            kind,
        });
    }
    Ok(Timing {
        anchors: timing.anchors.clone(),
        tempo: timing.tempo,
        events,
    })
}

/// Fails on a key in this ruleset's namespace, which defines none; other plugins' keys are
/// theirs to read.
fn reject_own_keys(ext: &Extensions, at: Location) -> Result<(), Error> {
    match ext.keys().find(|key| key.is_owned_by(ID)) {
        Some(key) => Err(Error::UnknownKey {
            at,
            key: key.clone(),
        }),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use charter_core::ValidDocument;
    use serde_json::{Value, json};

    /// A one-chart 7K document holding `objects`.
    fn document(objects: Value) -> Value {
        let mut value = json!({
            "format_version": 1,
            "ruleset": { "id": "osu_mania", "version": "0" },
            "metadata": { "title": "t", "artist": "a", "audio": "audio.mp3" },
            "timing": { "anchors": [ { "tick": 0, "seconds": 0.042 } ], "tempo": 140.0, "events": [] },
            "charts": [ { "name": "7K Hard", "lanes": 7, "settings": { "hp": 8.0, "od": 8.5 } } ]
        });
        value["charts"][0]["objects"] = objects;
        value
    }

    fn tap(tick: i32, column: u16) -> Value {
        json!({ "tick": tick, "position": { "lane": { "index": column } }, "kind": "tap", "shape": "instant" })
    }

    #[expect(
        clippy::unwrap_in_result,
        reason = "test fixture bytes always form a valid core document"
    )]
    fn read(value: &Value) -> Result<Song, Error> {
        let valid = ValidDocument::from_json(&serde_json::to_vec(value).unwrap()).unwrap();
        Song::try_from(&valid)
    }

    #[test]
    fn reads_a_seven_key_chart_and_its_difficulty() {
        let song = read(&document(json!([tap(0, 3)]))).unwrap();
        let chart = &song.charts()[0];
        assert_eq!(chart.name, "7K Hard");
        assert_eq!(chart.keys, Keys::K7);
        let difficulty = |v| Difficulty::new(Decimal::try_from(v).unwrap()).unwrap();
        assert_eq!((chart.hp, chart.od), (difficulty(8.0), difficulty(8.5)));
        assert_eq!(
            chart.notes,
            [Note {
                tick: Tick::new(0),
                column: LaneIndex::new(3),
                hold: None,
                keysound: None
            }]
        );
    }

    fn written(song: &Song) -> Value {
        serde_json::from_slice(&song.to_document().to_json().unwrap()).unwrap()
    }

    /// What a song read from `document(objects)` writes back.
    fn expected(objects: Value) -> Value {
        let mut value = document(objects);
        value["ruleset"]["version"] = json!(VERSION);
        value
    }

    #[test]
    fn the_plugin_names_its_ruleset_and_checks_documents_as_reading_does() {
        let valid =
            |value: &Value| ValidDocument::from_json(&serde_json::to_vec(value).unwrap()).unwrap();
        assert_eq!(Plugin.id(), ID);
        let song = valid(&document(json!([tap(0, 3)])));
        Plugin.check(&song).unwrap();
        let plane = json!([{ "tick": 0, "position": { "plane": { "x": 0.5, "y": 0.5 } },
                             "kind": "tap", "shape": "instant" }]);
        let not_a_song = valid(&document(plane));
        assert_eq!(
            Plugin.check(&not_a_song).unwrap_err().to_string(),
            Song::try_from(&not_a_song).unwrap_err().to_string()
        );
    }

    #[test]
    fn writes_a_chart_back_with_its_difficulty() {
        let objects = json!([tap(0, 3)]);
        let song = read(&document(objects.clone())).unwrap();
        assert_eq!(written(&song), expected(objects));
    }

    #[test]
    fn key_counts_follow_osu_stages() {
        // osu!lazer plays up to 10 keys on one stage and splits even counts up to 20 across two.
        let playable: Vec<LaneCount> = (0..=22)
            .filter_map(LaneCount::new)
            .filter(|&lanes| Keys::of(lanes).is_some())
            .collect();
        let expected =
            [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 14, 16, 18, 20].map(|n| LaneCount::new(n).unwrap());
        assert_eq!(playable, expected);
        let mut eleven = document(json!([]));
        eleven["charts"][0]["lanes"] = json!(11);
        assert!(matches!(read(&eleven), Err(Error::Keys { .. })));
    }

    #[test]
    fn holds_are_sustained_notes() {
        let hold = json!({ "tick": 0, "position": { "lane": { "index": 2 } }, "kind": "hold",
                           "shape": { "sustain": { "duration": 480 } } });
        let song = read(&document(json!([hold]))).unwrap();
        assert_eq!(song.charts()[0].notes[0].hold, Ticks::new(480));
        assert_eq!(written(&song), expected(json!([hold])));
    }

    #[test]
    fn keysounds_and_their_bank_read_and_write() {
        let mut note = tap(0, 6);
        note["sample"] = json!("kick");
        let objects = json!([note]);
        let mut value = document(objects.clone());
        value["samples"] = json!({ "kick": "kick.wav" });
        let song = read(&value).unwrap();
        assert_eq!(
            song.charts()[0].notes[0].keysound,
            Some(charter_core::SampleId::new("kick").unwrap())
        );
        let mut out = expected(objects);
        out["samples"] = value["samples"].clone();
        assert_eq!(written(&song), out);
    }

    fn with_events(events: &[Value]) -> Value {
        let mut value = document(json!([]));
        value["timing"]["events"]
            .as_array_mut()
            .unwrap()
            .extend_from_slice(events);
        value
    }

    #[test]
    fn meters_scroll_speed_and_bpm_changes_read_and_write() {
        let value = with_events(&[
            json!({ "tick": 0, "kind": { "time_signature": [3, 4] } }),
            json!({ "tick": 2880, "kind": { "other": { "name": "osu_mania.scroll_speed", "value": 0.5 } } }),
        ]);
        let mut chart_value = value;
        chart_value["timing"]["anchors"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "tick": 3840, "seconds": 2.0 }));
        chart_value["timing"]["tempo"] = json!(70.0);
        let song = read(&chart_value).unwrap();
        let kinds: Vec<TimingKind> = song
            .timing()
            .events
            .iter()
            .map(|event| event.kind)
            .collect();
        assert_eq!(
            kinds,
            [
                TimingKind::Meter(std::num::NonZeroU16::new(3).unwrap()),
                TimingKind::ScrollSpeed(ScrollSpeed::new(Decimal::try_from(0.5).unwrap()).unwrap()),
            ]
        );
        assert_eq!(song.timing().tempo, Bpm::try_from(70.0).unwrap());
    }

    #[test]
    fn timing_osu_cannot_express_is_refused() {
        for kind in [
            json!({ "stop": 1.0 }),
            json!({ "delay": 1.0 }),
            json!({ "warp": 960 }),
            json!({ "time_signature": [7, 8] }),
        ] {
            let mut event = json!({ "tick": 3840 });
            event["kind"] = kind;
            let value = with_events(&[event]);
            assert!(
                matches!(read(&value), Err(Error::Unsupported { .. })),
                "{value}"
            );
        }
    }

    #[test]
    fn a_chart_can_carry_its_own_timing() {
        let own = json!({
            "anchors": [ { "tick": 0, "seconds": 0.0 } ],
            "tempo": 70.0,
            "events": []
        });
        let mut value = document(json!([]));
        value["charts"][0]["timing"] = own.clone();
        let song = read(&value).unwrap();
        assert_eq!(
            song.charts()[0].timing.as_ref().map(|t| t.tempo),
            Some(Bpm::try_from(70.0).unwrap())
        );
        assert_eq!(written(&song)["charts"][0]["timing"], own);
    }

    #[test]
    fn difficulty_settings_default_to_five_and_stay_within_zero_to_ten() {
        let mut unset = document(json!([]));
        unset["charts"][0]
            .as_object_mut()
            .unwrap()
            .remove("settings");
        let song = read(&unset).unwrap();
        assert_eq!(
            written(&song)["charts"][0]["settings"],
            json!({ "hp": 5.0, "od": 5.0 })
        );
        for settings in [
            json!({ "hp": 11.0, "od": 5.0 }),
            json!({ "hp": 5.0, "od": -1.0 }),
            json!({ "hp": 5.0, "od": 5.0, "ar": 9.0 }),
        ] {
            let mut value = document(json!([]));
            value["charts"][0]["settings"] = settings;
            assert!(
                matches!(read(&value), Err(Error::Malformed { .. })),
                "{value}"
            );
        }
    }

    #[test]
    fn a_song_of_another_ruleset_is_refused() {
        let mut value = document(json!([]));
        value["ruleset"]["id"] = json!("sm");
        assert!(matches!(read(&value), Err(Error::WrongRuleset(id)) if id.as_str() == "sm"));
    }

    #[test]
    fn objects_osu_mania_cannot_play_are_refused() {
        let mut roll = tap(0, 0);
        roll["kind"] = json!("roll");
        roll["shape"] = json!({ "sustain": { "duration": 240 } });
        let mut mine = tap(0, 0);
        mine["kind"] = json!("mine");
        let mut wide = tap(0, 0);
        wide["position"] = json!({ "lane": { "index": 0, "width": 2 } });
        let mut plane = tap(0, 0);
        plane["position"] = json!({ "plane": { "x": 0.0, "y": 0.0 } });
        let mut path = tap(0, 0);
        path["shape"] = json!({ "path": { "points": [
            { "tick": 240, "position": { "lane": { "index": 1 } }, "ease": "linear" } ] } });
        for object in [roll, mine, wide, plane, path] {
            let value = document(json!([object]));
            assert!(
                matches!(read(&value), Err(Error::UnsupportedObject { .. })),
                "{object}"
            );
        }
        let mut parent = tap(0, 0);
        parent["kind"] = json!("hold");
        parent["shape"] = json!({ "sustain": { "duration": 480 } });
        parent["children"] = json!([tap(240, 1)]);
        assert!(matches!(
            read(&document(json!([parent]))),
            Err(Error::Unsupported { .. })
        ));
    }

    #[test]
    fn keys_in_the_osu_mania_namespace_or_song_settings_that_it_does_not_define_are_refused() {
        type Edit = fn(&mut Value);
        let edits: [Edit; 5] = [
            |v| v["charts"][0]["objects"][0]["ext"] = json!({ "osu_mania.hitsound": 2 }),
            |v| v["metadata"]["ext"] = json!({ "osu_mania.source": "x" }),
            |v| v["charts"][0]["ext"] = json!({ "osu_mania.tags": "x" }),
            |v| {
                let kiai = json!({ "tick": 0, "kind": { "other": { "name": "osu_mania.kiai", "value": true } } });
                v["timing"]["events"].as_array_mut().unwrap().push(kiai);
            },
            |v| v["settings"] = json!({ "special_style": true }),
        ];
        for edit in edits {
            let mut value = document(json!([tap(0, 0)]));
            edit(&mut value);
            assert!(
                matches!(
                    read(&value),
                    Err(Error::UnknownKey { .. } | Error::UnknownSetting { .. })
                ),
                "{value}"
            );
        }
    }

    #[test]
    fn other_plugins_data_is_skipped_and_not_written_back() {
        let plain = document(json!([tap(0, 0)]));
        let mut decorated = plain.clone();
        decorated["plugins"] = json!({ "swing": { "version": "1" } });
        decorated["metadata"]["ext"] = json!({ "swing.feel": "late" });
        decorated["charts"][0]["objects"][0]["ext"] = json!({ "swing.accent": true });
        let groove =
            json!({ "tick": 0, "kind": { "other": { "name": "swing.groove", "value": 0.6 } } });
        decorated["timing"]["events"]
            .as_array_mut()
            .unwrap()
            .push(groove);
        let song = read(&decorated).unwrap();
        assert_eq!(song, read(&plain).unwrap());
        assert_eq!(
            written(&song),
            expected(plain["charts"][0]["objects"].clone())
        );
    }

    #[test]
    fn difficulty_bounds_include_zero_and_ten() {
        let decimal = |v| Decimal::try_from(v).unwrap();
        for edge in [0.0, 10.0] {
            assert!(Difficulty::new(decimal(edge)).is_some(), "{edge}");
        }
        for outside in [-0.01, 10.01] {
            assert!(Difficulty::new(decimal(outside)).is_none(), "{outside}");
        }
    }

    #[test]
    fn difficulties_and_scroll_speeds_refuse_a_seventh_place_and_stopped_scrolling() {
        let mut hp = document(json!([]));
        hp["charts"][0]["settings"]["hp"] = json!(8.123_456_7);
        let speed = |value: f64| {
            let mut kind = json!({ "other": { "name": "osu_mania.scroll_speed" } });
            kind["other"]["value"] = json!(value);
            let mut event = json!({ "tick": 0 });
            event["kind"] = kind;
            with_events(&[event])
        };
        for value in [hp, speed(0.123_456_7), speed(0.0)] {
            assert!(read(&value).is_err(), "{value}");
        }
    }

    #[test]
    fn key_counts_are_variants() {
        assert_eq!(Keys::of(LaneCount::new(7).unwrap()), Some(Keys::K7));
        assert_eq!(Keys::K20.lanes(), LaneCount::new(20).unwrap());
    }

    #[test]
    fn errors_say_where_and_which_key() {
        let mut value = document(json!([]));
        value["charts"][0]["ext"] = json!({ "osu_mania.skin": "argon" });
        let skin: Key = "osu_mania.skin".parse().unwrap();
        assert!(matches!(
            read(&value),
            Err(Error::UnknownKey { at: Location::ChartExt(0), key }) if key == skin
        ));
        let mut settings = document(json!([]));
        settings["settings"] = json!({ "speed": 2 });
        assert!(matches!(
            read(&settings),
            Err(Error::UnknownSetting { at: Location::Settings, key }) if key == "speed"
        ));
    }

    #[test]
    fn a_song_is_checked_when_built() {
        let song = read(&document(json!([tap(0, 1)]))).unwrap();
        let build = |metadata: Metadata, charts: Vec<Chart>| {
            Song::new(
                metadata,
                song.ticks_per_beat(),
                song.samples().clone(),
                song.timing().clone(),
                song.markers().to_vec(),
                charts,
            )
        };
        assert_eq!(
            build(song.metadata().clone(), song.charts().to_vec()).unwrap(),
            song
        );
        let mut outside = song.charts().to_vec();
        outside[0].notes[0].column = LaneIndex::new(6 + 1);
        let mut tagged = song.metadata().clone();
        tagged
            .ext
            .insert("osu_mania.genre".parse().unwrap(), json!("rock"));
        assert!(matches!(
            build(song.metadata().clone(), outside),
            Err(Error::Document(_))
        ));
        assert!(matches!(
            build(tagged, song.charts().to_vec()),
            Err(Error::UnknownKey {
                at: Location::MetadataExt,
                ..
            })
        ));
    }

    #[test]
    fn shared_markers_are_kept_and_other_plugins_markers_skipped() {
        let mut value = document(json!([]));
        value["plugins"] = json!({ "swing": { "version": "1" } });
        value["markers"] = json!([
            { "tick": 0, "kind": "section", "text": "Intro" },
            { "tick": 960, "kind": "bookmark", "text": "fix" },
            { "tick": 960, "kind": "swing.accent", "text": "" }
        ]);
        let song = read(&value).unwrap();
        let mut out = expected(json!([]));
        out["charts"][0].as_object_mut().unwrap().remove("objects");
        out["markers"] = json!(value["markers"].as_array().unwrap()[..2]);
        assert_eq!(written(&song), out);
        let mut own = document(json!([]));
        own["markers"] = json!([{ "tick": 0, "kind": "osu_mania.drop", "text": "" }]);
        assert!(matches!(
            read(&own),
            Err(Error::UnknownKey {
                at: Location::Marker(0),
                ..
            })
        ));
    }

    fn new_song(layout: usize, charts: &[&str]) -> charter_core::NewSong {
        charter_core::NewSong {
            metadata: Metadata {
                title: "t".to_owned(),
                artist: "a".to_owned(),
                audio: charter_core::ResourcePath::new("audio.ogg").unwrap(),
                preview: None,
                artwork: None,
                genre: None,
                ext: Extensions::new(),
            },
            layout,
            charts: charts
                .iter()
                .map(|&name| charter_core::NewChart::named(name))
                .collect(),
        }
    }

    #[test]
    fn a_new_song_offers_every_key_count_with_four_keys_first() {
        let layouts = Plugin.layouts();
        assert_eq!(layouts.len(), Keys::ALL.len());
        assert_eq!(layouts[0].name, "4K");
        assert_eq!(layouts[0].lanes, LaneCount::new(4).unwrap());
        assert_eq!(Plugin.layout_kind(), "Keys");
        assert_eq!(Plugin.name(), "osu!mania");
    }

    #[test]
    fn a_new_song_gives_every_chart_its_keys_and_middling_difficulty() {
        let seven = Plugin
            .layouts()
            .iter()
            .position(|layout| layout.name == "7K")
            .unwrap();
        let document = Plugin
            .new_song(&new_song(seven, &["Normal", "Hard"]))
            .unwrap();
        Plugin.check(&document).unwrap();
        let song = Song::try_from(&document).unwrap();
        assert_eq!(song.charts().len(), 2);
        assert!(
            song.charts()
                .iter()
                .all(|chart| chart.keys == Keys::K7 && chart.notes.is_empty())
        );
    }

    #[test]
    fn a_new_song_refuses_a_key_count_it_does_not_have() {
        Plugin
            .new_song(&new_song(Keys::ALL.len(), &["Easy"]))
            .unwrap_err();
    }

    #[test]
    fn the_plugin_imports_timing_and_reports_bad_pasted_points() {
        let doc =
            ValidDocument::from_json(&serde_json::to_vec(&document(json!([]))).unwrap()).unwrap();
        assert_eq!(Plugin.timing_format(), Some("osu! timing points"));
        let imported = Plugin.import_timing("250,500\n1250,-50", &doc).unwrap();
        assert_eq!(
            imported.timing.anchors[0].seconds,
            Origin::try_from(0.25).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&imported.timing.events).unwrap(),
            json!([{ "tick": 1920, "kind": { "other": {
                "name": "osu_mania.scroll_speed", "value": 2.0
            } } }])
        );
        assert_eq!(
            Plugin
                .import_timing("soon,500", &doc)
                .unwrap_err()
                .to_string(),
            "line 1: `soon` is not a time"
        );
    }

    #[test]
    fn zero_and_twenty_keys_are_checked_against_the_format_and_the_ruleset() {
        let mut missing = document(json!([]));
        missing["charts"][0]
            .as_object_mut()
            .unwrap()
            .remove("lanes");
        assert!(matches!(
            read(&missing),
            Err(Error::Keys {
                at: Location::Chart(0),
                found: None
            })
        ));
        let mut twenty = document(json!([tap(0, 19)]));
        twenty["charts"][0]["lanes"] = json!(20);
        let song = read(&twenty).unwrap();
        assert_eq!(song.charts()[0].keys, Keys::K20);
        let mut out = expected(twenty["charts"][0]["objects"].clone());
        out["charts"][0]["lanes"] = json!(20);
        assert_eq!(written(&song), out);
        twenty["charts"][0]["objects"][0]["position"]["lane"]["index"] = json!(20);
        assert!(matches!(
            ValidDocument::from_json(&serde_json::to_vec(&twenty).unwrap()),
            Err(charter_core::Error::Invalid(_))
        ));
    }

    #[test]
    fn malformed_scroll_speed_reports_its_event_and_wrong_chart_setting_reports_its_chart() {
        let speed = json!({ "tick": 0, "kind": { "other": {
            "name": "osu_mania.scroll_speed", "value": "quick"
        } } });
        assert!(matches!(
            read(&with_events(&[speed])),
            Err(Error::Malformed {
                at: Location::Event {
                    stream: Stream::Song,
                    index: 0
                },
                ..
            })
        ));
        let mut chart = document(json!([]));
        chart["charts"][0]["settings"]["od"] = json!(11);
        assert!(matches!(
            read(&chart),
            Err(Error::Malformed {
                at: Location::ChartSettings(0),
                ..
            })
        ));
    }

    #[test]
    fn a_hold_at_the_last_valid_tick_round_trips_but_an_overrunning_hold_is_refused() {
        let hold = json!({ "tick": 2_147_483_646_i32,
            "position": { "lane": { "index": 6 } }, "kind": "hold",
            "shape": { "sustain": { "duration": 1 } } });
        let value = document(json!([hold]));
        let song = read(&value).unwrap();
        assert_eq!(song.charts()[0].notes[0].hold, Ticks::new(1));
        assert_eq!(
            written(&song),
            expected(value["charts"][0]["objects"].clone())
        );
        let mut invalid = value;
        invalid["charts"][0]["objects"][0]["shape"]["sustain"]["duration"] = json!(2);
        assert!(matches!(
            ValidDocument::from_json(&serde_json::to_vec(&invalid).unwrap()),
            Err(charter_core::Error::Invalid(_))
        ));
    }
}

/// Proofs for Kani, run with `cargo kani -p charter-osu-mania`.
#[cfg(kani)]
mod proofs {
    use super::*;

    #[kani::proof]
    #[kani::unwind(17)]
    fn osu_plays_one_to_ten_keys_or_an_even_count_up_to_twenty() {
        let count: u16 = kani::any();
        let Some(lanes) = LaneCount::new(count) else {
            return;
        };
        let playable = count <= 10 || (count <= 20 && count % 2 == 0);
        assert!(Keys::of(lanes).is_some() == playable);
        if let Some(keys) = Keys::of(lanes) {
            assert!(keys.lanes() == lanes);
        }
    }

    #[kani::proof]
    fn difficulty_and_scroll_speed_accept_exactly_their_decimal_ranges() {
        let millionths: i64 = kani::any();
        let Some(value) = Decimal::from_millionths(millionths) else {
            return;
        };
        assert!(Difficulty::new(value).is_some() == (0..=10_000_000).contains(&millionths));
        assert!(ScrollSpeed::new(value).is_some() == (millionths > 0));
    }
}
