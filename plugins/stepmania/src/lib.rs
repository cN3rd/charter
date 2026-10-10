//! StepMania as a charter ruleset.
//!
//! A chart's [`Style`] fixes its lane count. Timing covers everything StepMania 5 interprets,
//! including its scroll and combo segments, which travel as `sm.*` events. `#LABELS` are the song's
//! section markers.
//!
//! [`Song`] is the typed view of a document whose ruleset is [`ID`]. Reading one fails on
//! anything StepMania cannot represent and skips data other plugins left behind;
//! [`Song::to_document`] writes this ruleset's data alone. A song is checked when built, by
//! reading or by [`Song::new`], so writing one cannot fail. [`Plugin`] hands the editor the same
//! check, so a document is editable only while it reads as a song.

use std::collections::BTreeMap;

use charter_core::{
    Anchor, Bpm, Chart as CoreChart, CurrentFormat, Decimal, Document, Extensions, InvalidValue,
    Key, Kind, LaneCount, LaneIndex, Layout, Location, Marker, Metadata, Name, Namespace, NewSong,
    Object, Origin, Position, ResourcePath, Ruleset, RulesetPlugin, SampleId, Seconds, Settings,
    Shape, Stream, Tick, Ticks, TicksPerBeat, TimeSignature, Timing as CoreTiming,
    TimingEvent as CoreEvent, TimingKind as CoreKind, ValidDocument,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de, de::IntoDeserializer};
use serde_json::Value;

/// The ruleset namespace.
const ID: &str = "sm";

/// A song, checked against this ruleset and every document rule when built, and unchangeable
/// after.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Song {
    /// Header values shared by all charts.
    metadata: Metadata,
    /// Number of ticks per quarter-note beat.
    ticks_per_beat: TicksPerBeat,
    /// Keysounds available to notes across the song.
    samples: BTreeMap<SampleId, ResourcePath>,
    /// Song-wide timing, used unless a chart overrides it.
    timing: Timing,
    /// Song-wide section and bookmark positions.
    markers: Vec<Marker>,
    /// Charts checked against this ruleset.
    charts: Vec<Chart>,
}

/// A tempo map with StepMania's extra timing segments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Timing {
    /// Audio positions pinned to chart ticks.
    pub anchors: Vec<Anchor>,
    /// Tempo after the last anchor.
    pub tempo: Bpm,
    /// Changes to metre and game-specific timing.
    pub events: Vec<TimingEvent>,
}

/// A timing change at a chart tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimingEvent {
    /// Position of the change.
    pub tick: Tick,
    /// Effect of the change.
    pub kind: TimingKind,
}

/// Timing segments supported by StepMania.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TimingKind {
    /// Pause before the tick.
    Stop(Seconds),
    /// Pause after the tick.
    Delay(Seconds),
    /// Ticks skipped without elapsed audio time.
    Warp(Ticks),
    /// Beats and beat unit per measure.
    TimeSignature(TimeSignature),
    /// Hold ticks per beat for scoring.
    TickCount(TickCount),
    /// Hit and miss combo multipliers.
    Combo(Combo),
    /// Scroll speed that eases toward `ratio` over `length`.
    Speed(Speed),
    /// Scroll speed that applies immediately.
    Scroll(Decimal),
    /// Ticks from here whose notes are fake.
    Fake(Ticks),
}

/// Hold ticks per beat, from 0 to 48.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct TickCount(u8);

impl TickCount {
    /// A tick count if it fits StepMania's range.
    #[must_use]
    pub const fn new(count: u8) -> Option<Self> {
        if count <= 48 { Some(Self(count)) } else { None }
    }

    /// The number of hold ticks per beat.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

#[expect(
    clippy::missing_trait_methods,
    reason = "serde's default `deserialize_in_place` is right"
)]
impl<'de> Deserialize<'de> for TickCount {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let count = u8::deserialize(deserializer)?;
        Self::new(count).ok_or_else(|| de::Error::custom("tick count must be from 0 to 48"))
    }
}

/// Multipliers applied to successful and missed notes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Combo {
    /// Multiplier for hits.
    pub hit: u32,
    /// Multiplier for misses.
    pub miss: u32,
}

/// An eased scroll-speed change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Speed {
    /// Target scroll multiplier.
    pub ratio: Decimal,
    /// Duration and unit of the transition.
    pub length: SpeedLength,
}

/// Unit and duration of an eased speed change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum SpeedLength {
    /// The new speed applies at once.
    Instant,
    /// Ease over this many chart ticks.
    Ticks(Ticks),
    /// Ease over this much audio time.
    Seconds(Seconds),
}

/// A chart's steps, settings and optional timing override.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chart {
    /// Display name.
    pub name: String,
    /// Steps type, fixing the lane count.
    pub style: Style,
    /// Named difficulty.
    pub difficulty: Difficulty,
    /// Numeric difficulty rating.
    pub meter: u32,
    /// Free-text description.
    pub description: String,
    /// Replaces the song's timing when present, as in `.ssc`.
    pub timing: Option<Timing>,
    /// Placed notes.
    pub notes: Vec<Note>,
}

/// Defines [`Style`] from one table of variant, StepMania name and lane count.
macro_rules! styles {
    ($($variant:ident = $name:literal, $lanes:expr;)+) => {
        /// A StepMania steps type such as `dance-single`.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Style {
            $(#[doc = concat!("The `", $name, "` steps type.")] $variant,)+
        }

        impl Style {
            /// Every playable steps type in StepMania 5's `GameManager.cpp`.
            pub const ALL: &'static [Self] = &[$(Self::$variant,)+];

            /// The name StepMania writes, such as `dance-single`.
            #[must_use]
            pub const fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $name,)+
                }
            }

            /// Lane count for this steps type, including both players in routine styles.
            #[must_use]
            pub const fn lanes(self) -> LaneCount {
                match self {
                    $(Self::$variant => lanes($lanes),)+
                }
            }
        }
    };
}

styles! {
    DanceSingle = "dance-single", 4;
    DanceDouble = "dance-double", 8;
    DanceCouple = "dance-couple", 8;
    DanceSolo = "dance-solo", 6;
    DanceThreepanel = "dance-threepanel", 3;
    DanceRoutine = "dance-routine", routine(8);
    PumpSingle = "pump-single", 5;
    PumpHalfdouble = "pump-halfdouble", 6;
    PumpDouble = "pump-double", 10;
    PumpCouple = "pump-couple", 10;
    PumpRoutine = "pump-routine", routine(10);
    Kb7Single = "kb7-single", 7;
    Ez2Single = "ez2-single", 5;
    Ez2Double = "ez2-double", 10;
    Ez2Real = "ez2-real", 7;
    ParaSingle = "para-single", 5;
    Ds3ddxSingle = "ds3ddx-single", 8;
    BmSingle5 = "bm-single5", 6;
    BmVersus5 = "bm-versus5", 6;
    BmDouble5 = "bm-double5", 12;
    BmSingle7 = "bm-single7", 8;
    BmVersus7 = "bm-versus7", 8;
    BmDouble7 = "bm-double7", 16;
    ManiaxSingle = "maniax-single", 4;
    ManiaxDouble = "maniax-double", 8;
    TechnoSingle4 = "techno-single4", 4;
    TechnoSingle5 = "techno-single5", 5;
    TechnoSingle8 = "techno-single8", 8;
    TechnoDouble4 = "techno-double4", 8;
    TechnoDouble5 = "techno-double5", 10;
    TechnoDouble8 = "techno-double8", 16;
    PnmFive = "pnm-five", 5;
    PnmNine = "pnm-nine", 9;
    KickboxHuman = "kickbox-human", 4;
    KickboxQuadarm = "kickbox-quadarm", 4;
    KickboxInsect = "kickbox-insect", 6;
    KickboxArachnid = "kickbox-arachnid", 8;
}

/// Routine styles, where two players share one set of panels, get a full set of lanes per
/// player, player 1 first.
const fn routine(columns: u16) -> u16 {
    columns.saturating_mul(2)
}

impl Style {
    /// The steps type with StepMania's `name`, if supported.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|style| style.name() == name)
    }
}

/// `n` lanes, for the styles' own column counts.
const fn lanes(n: u16) -> LaneCount {
    #[expect(
        clippy::unreachable,
        reason = "only positive constant style widths call this"
    )]
    match LaneCount::new(n) {
        Some(lanes) => lanes,
        None => unreachable!(),
    }
}

impl Serialize for Style {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

// Written by hand so the names come from the style table.
#[expect(
    clippy::missing_trait_methods,
    reason = "serde's default `deserialize_in_place` is right"
)]
impl<'de> Deserialize<'de> for Style {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Self::named(&name)
            .ok_or_else(|| de::Error::custom(format_args!("unknown StepMania style {name:?}")))
    }
}

/// Named StepMania chart difficulty.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Difficulty {
    /// Entry-level steps.
    Beginner,
    /// Easy steps.
    Easy,
    /// Medium steps.
    Medium,
    /// Hard steps.
    Hard,
    /// Challenge steps.
    Challenge,
    /// User-created steps.
    Edit,
}

/// Chart settings as stored in the document.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChartSettings {
    /// Steps type determining the chart's lane count.
    style: Style,
    /// Standard difficulty written as a chart setting.
    difficulty: Difficulty,
    /// Numeric difficulty rating.
    meter: u32,
    /// Optional free-text chart description.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    description: String,
}

/// A single placed step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    /// Chart tick of the note.
    pub tick: Tick,
    /// Column containing the note.
    pub lane: LaneIndex,
    /// How the step is judged.
    pub kind: NoteKind,
    /// Optional sample played with the note.
    pub keysound: Option<SampleId>,
}

/// StepMania's supported step types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteKind {
    /// One instant step.
    Tap,
    /// Sustained step.
    Hold(Ticks),
    /// Sustained roll requiring repeated presses.
    Roll(Ticks),
    /// Step penalising a press.
    Mine,
    /// Step judged on release.
    Lift,
    /// Unjudged visible step.
    Fake,
    /// Plays its keysound unjudged.
    AutoKeysound,
}

/// Ruleset key for an unjudged keysound note.
const AUTO_KEYSOUND: &str = "auto_keysound";
/// Ruleset key for hold ticks counted per beat.
const TICKCOUNT: &str = "tickcount";
/// Ruleset key for hit and miss combo multipliers.
const COMBO: &str = "combo";
/// Ruleset key for an eased scroll-speed change.
const SPEED: &str = "speed";
/// Ruleset key for an immediate scroll multiplier.
const SCROLL: &str = "scroll";
/// Ruleset key for a span of fake notes.
const FAKE: &str = "fake";

/// This ruleset's key for `name`.
fn own(name: &str) -> Result<Key, InvalidValue> {
    Ok(Key::new(Namespace::new(ID)?, Name::new(name)?))
}

/// Written as the ruleset version.
const VERSION: &str = env!("CARGO_PKG_VERSION");

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

    /// Header values shared by the song's charts.
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

    /// Song-wide timing, before any chart override.
    #[must_use]
    pub const fn timing(&self) -> &Timing {
        &self.timing
    }

    /// Charts in this song.
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

    /// Validates and writes the song using only StepMania data.
    fn build(&self) -> Result<ValidDocument, Error> {
        let charts = self
            .charts
            .iter()
            .map(Chart::to_core)
            .collect::<Result<_, _>>()?;
        let doc = Document {
            format_version: CurrentFormat,
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
    /// Writes StepMania's extra segments as namespaced shared timing events.
    fn to_core(&self) -> Result<CoreTiming, Error> {
        let other = |name: &str, value: Value| -> Result<CoreKind, Error> {
            Ok(CoreKind::Other {
                name: own(name)?,
                value,
            })
        };
        let events = self.events.iter().map(|event| {
            let kind = match &event.kind {
                TimingKind::Stop(seconds) => CoreKind::Stop(*seconds),
                TimingKind::Delay(seconds) => CoreKind::Delay(*seconds),
                TimingKind::Warp(ticks) => CoreKind::Warp(*ticks),
                TimingKind::TimeSignature(meter) => CoreKind::TimeSignature(*meter),
                TimingKind::TickCount(count) => other(TICKCOUNT, count.get().into())?,
                TimingKind::Combo(combo) => other(COMBO, serde_json::to_value(combo)?)?,
                TimingKind::Speed(speed) => other(SPEED, serde_json::to_value(speed)?)?,
                TimingKind::Scroll(ratio) => other(SCROLL, serde_json::to_value(ratio)?)?,
                TimingKind::Fake(ticks) => other(FAKE, serde_json::to_value(ticks)?)?,
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
    /// Writes the steps type and difficulty as settings alongside the placed notes.
    fn to_core(&self) -> Result<CoreChart, Error> {
        let settings = serde_json::to_value(ChartSettings {
            style: self.style,
            difficulty: self.difficulty,
            meter: self.meter,
            description: self.description.clone(),
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
            lanes: Some(self.style.lanes()),
            settings,
            timing: self.timing.as_ref().map(Timing::to_core).transpose()?,
            // StepMania has no layers; writing its game model deliberately exports the merged chart.
            layers: Vec::new(),
            objects: self
                .notes
                .iter()
                .map(Note::to_object)
                .collect::<Result<_, _>>()?,
            ext: Extensions::new(),
        })
    }
}

impl Note {
    /// Writes this note as a one-lane object, retaining its optional keysound.
    fn to_object(&self) -> Result<Object, InvalidValue> {
        let (kind, shape) = match self.kind {
            NoteKind::Tap => (Kind::Tap, Shape::Instant),
            NoteKind::Hold(duration) => (Kind::Hold, Shape::Sustain { duration }),
            NoteKind::Roll(duration) => (Kind::Roll, Shape::Sustain { duration }),
            NoteKind::Mine => (Kind::Mine, Shape::Instant),
            NoteKind::Lift => (Kind::Lift, Shape::Instant),
            NoteKind::Fake => (Kind::Fake, Shape::Instant),
            NoteKind::AutoKeysound => (Kind::Other(own(AUTO_KEYSOUND)?), Shape::Instant),
        };
        Ok(Object {
            tick: self.tick,
            position: Position::Lane {
                index: self.lane,
                width: LaneCount::ONE,
            },
            kind,
            shape,
            // A StepMania note has no layer identity in its game model.
            layer: None,
            sample: self.keysound.clone(),
            children: Vec::new(),
            ext: Extensions::new(),
        })
    }
}

/// Why a document cannot be represented as a StepMania song.
#[expect(
    clippy::error_impl_error,
    reason = "public error type is part of the plugin API"
)]
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Document belongs to another ruleset.
    #[error("ruleset is \"{0}\", not {ID:?}")]
    WrongRuleset(Namespace),
    /// Chart columns do not match its steps type.
    #[error("{at}: a {style} chart has {expected} lanes, not {}", .found.map_or_else(|| "none".to_owned(), |lanes| lanes.to_string()))]
    Lanes {
        /// Chart containing the mismatch.
        at: Location,
        /// Steps type's external name.
        style: &'static str,
        /// Required columns.
        expected: LaneCount,
        /// Actual columns, if specified.
        found: Option<LaneCount>,
    },
    /// Unrecognised key in this plugin's namespace.
    #[error("{at}: \"{key}\" is not a StepMania key")]
    UnknownKey {
        /// Location of the key.
        at: Location,
        /// Unrecognised key.
        key: Key,
    },
    /// Unrecognised chart or song setting.
    #[error("{at}: \"{key}\" is not a StepMania setting")]
    UnknownSetting {
        /// Location of the setting.
        at: Location,
        /// Unrecognised setting.
        key: String,
    },
    /// Note shape or position StepMania cannot play.
    #[error("{at}: not a StepMania note")]
    UnsupportedObject {
        /// Location of the unsupported note.
        at: Location,
    },
    /// Unsupported timing or object feature.
    #[error("{at}: {what} are not part of StepMania")]
    Unsupported {
        /// Location of the unsupported feature.
        at: Location,
        /// Name of the feature.
        what: &'static str,
    },
    /// Malformed plugin-specific setting or timing value.
    #[error("{at}: {source}")]
    Malformed {
        /// Location of the malformed value.
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
    reason = "StepMania uses the trait's defaults for `note_changes`, `change_note`, `difficulties`, `text_settings`, `timing_format` and `import_timing`: it offers none of them yet"
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
        "StepMania"
    }

    fn layout_kind(&self) -> &'static str {
        "Style"
    }

    fn layouts(&self) -> Vec<Layout> {
        Style::ALL
            .iter()
            .map(|style| Layout {
                name: style.name().to_owned(),
                lanes: style.lanes(),
            })
            .collect()
    }

    /// Every chart gets the style, meter 1, and the difficulty its name spells, if it spells one,
    /// or `edit`.
    fn new_song(
        &self,
        song: &NewSong,
    ) -> Result<ValidDocument, Box<dyn std::error::Error + Send + Sync>> {
        let style = *Style::ALL
            .get(song.layout)
            .ok_or("StepMania has no such style")?;
        let charts = song
            .charts
            .iter()
            .map(|chart| Chart {
                name: chart.name.clone(),
                style,
                difficulty: serde_json::from_value(Value::String(chart.name.to_lowercase()))
                    .unwrap_or(Difficulty::Edit),
                meter: 1,
                description: String::new(),
                timing: None,
                notes: Vec::new(),
            })
            .collect();
        let at_zero = |kind| TimingEvent {
            tick: Tick::new(0),
            kind,
        };
        let timing = Timing {
            anchors: vec![Anchor {
                tick: Tick::new(0),
                seconds: Origin::try_from(0.0)?,
            }],
            tempo: Bpm::try_from(120.0)?,
            events: vec![at_zero(TimingKind::TimeSignature(TimeSignature::default()))],
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

/// Reads chart `c`, checking that its settings agree with its lane count.
fn read_chart(chart: &CoreChart, c: usize) -> Result<Chart, Error> {
    reject_own_keys(&chart.ext, Location::ChartExt(c))?;
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
    if chart.lanes != Some(settings.style.lanes()) {
        return Err(Error::Lanes {
            at: Location::Chart(c),
            style: settings.style.name(),
            expected: settings.style.lanes(),
            found: chart.lanes,
        });
    }
    let notes = chart
        .objects
        .iter()
        .enumerate()
        .map(|(j, object)| read_note(object, Location::Object { chart: c, index: j }))
        .collect::<Result<_, _>>()?;
    Ok(Chart {
        name: chart.name.clone(),
        style: settings.style,
        difficulty: settings.difficulty,
        meter: settings.meter,
        description: settings.description,
        timing: chart
            .timing
            .as_ref()
            .map(|timing| read_timing(timing, Stream::Chart(c)))
            .transpose()?,
        notes,
    })
}

/// A single-lane StepMania note at `at`, rejecting unsupported shapes.
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
    let kind = match (&object.kind, &object.shape) {
        (Kind::Tap, Shape::Instant) => NoteKind::Tap,
        (Kind::Hold, &Shape::Sustain { duration }) => NoteKind::Hold(duration),
        (Kind::Roll, &Shape::Sustain { duration }) => NoteKind::Roll(duration),
        (Kind::Mine, Shape::Instant) => NoteKind::Mine,
        (Kind::Lift, Shape::Instant) => NoteKind::Lift,
        (Kind::Fake, Shape::Instant) => NoteKind::Fake,
        (Kind::Other(key), Shape::Instant)
            if key.is_owned_by(ID) && key.name().as_str() == AUTO_KEYSOUND =>
        {
            NoteKind::AutoKeysound
        }
        _ => return Err(unsupported()),
    };
    Ok(Note {
        tick: object.tick,
        lane: index,
        kind,
        keysound: object.sample.clone(),
    })
}

/// Converts shared events in `stream` into StepMania timing.
fn read_timing(timing: &CoreTiming, stream: Stream) -> Result<Timing, Error> {
    let mut events = Vec::with_capacity(timing.events.len());
    for (index, event) in timing.events.iter().enumerate() {
        let at = Location::Event { stream, index };
        reject_own_keys(&event.ext, at)?;
        let kind = match &event.kind {
            CoreKind::Stop(seconds) => TimingKind::Stop(*seconds),
            CoreKind::Delay(seconds) => TimingKind::Delay(*seconds),
            CoreKind::Warp(ticks) => TimingKind::Warp(*ticks),
            CoreKind::TimeSignature(meter) => TimingKind::TimeSignature(*meter),
            CoreKind::Other { name, value } => match read_other(name, value) {
                Some(Ok(kind)) => kind,
                Some(Err(source)) => return Err(Error::Malformed { at, source }),
                None if name.is_owned_by(ID) => {
                    return Err(Error::UnknownKey {
                        at,
                        key: name.clone(),
                    });
                }
                None => continue,
            },
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

/// This ruleset's reading of an `other` event, or `None` if the name is not one of its own.
fn read_other(name: &Key, value: &Value) -> Option<Result<TimingKind, serde_json::Error>> {
    if !name.is_owned_by(ID) {
        return None;
    }
    Some(match name.name().as_str() {
        TICKCOUNT => TickCount::deserialize(value).map(TimingKind::TickCount),
        COMBO => Combo::deserialize(value).map(TimingKind::Combo),
        SPEED => Speed::deserialize(value).map(TimingKind::Speed),
        SCROLL => Decimal::deserialize(value).map(TimingKind::Scroll),
        FAKE => Ticks::deserialize(value).map(TimingKind::Fake),
        _ => return None,
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

    /// A one-chart dance-single document holding `objects`.
    fn document(objects: Value) -> Value {
        let mut value = json!({
            "format_version": 1,
            "ruleset": { "id": "sm", "version": "0" },
            "metadata": { "title": "t", "artist": "a", "audio": "song.ogg" },
            "timing": { "anchors": [ { "tick": 0, "seconds": -0.014 } ], "tempo": 174.0, "events": [] },
            "charts": [ {
                "name": "Single Hard",
                "lanes": 4,
                "settings": { "style": "dance-single", "difficulty": "hard", "meter": 9, "description": "someone" }
            } ]
        });
        value["charts"][0]["objects"] = objects;
        value
    }

    fn lane_object(tick: i32, lane: u16, kind: &str) -> Value {
        json!({ "tick": tick, "position": { "lane": { "index": lane } }, "kind": kind, "shape": "instant" })
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
    fn reads_a_dance_single_chart_and_its_settings() {
        let song = read(&document(json!([lane_object(0, 3, "tap")]))).unwrap();
        let chart = &song.charts()[0];
        assert_eq!(chart.name, "Single Hard");
        assert_eq!(chart.style.name(), "dance-single");
        assert_eq!(chart.difficulty, Difficulty::Hard);
        assert_eq!(chart.meter, 9);
        assert_eq!(chart.description, "someone");
        assert_eq!(
            chart.notes,
            [Note {
                tick: Tick::new(0),
                lane: LaneIndex::new(3),
                kind: NoteKind::Tap,
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
        let song = valid(&document(json!([lane_object(0, 3, "tap")])));
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
    fn writes_a_chart_back_with_its_settings() {
        let objects = json!([lane_object(0, 3, "tap")]);
        let song = read(&document(objects.clone())).unwrap();
        assert_eq!(written(&song), expected(objects));
    }

    #[test]
    fn every_playable_stepmania_style_knows_its_lanes() {
        // Column counts from StepMania 5's GameManager.cpp; routine styles get a set per player.
        let styles = [
            ("dance-single", 4),
            ("dance-double", 8),
            ("dance-couple", 8),
            ("dance-solo", 6),
            ("dance-threepanel", 3),
            ("dance-routine", 16),
            ("pump-single", 5),
            ("pump-halfdouble", 6),
            ("pump-double", 10),
            ("pump-couple", 10),
            ("pump-routine", 20),
            ("kb7-single", 7),
            ("ez2-single", 5),
            ("ez2-double", 10),
            ("ez2-real", 7),
            ("para-single", 5),
            ("ds3ddx-single", 8),
            ("bm-single5", 6),
            ("bm-versus5", 6),
            ("bm-double5", 12),
            ("bm-single7", 8),
            ("bm-versus7", 8),
            ("bm-double7", 16),
            ("maniax-single", 4),
            ("maniax-double", 8),
            ("techno-single4", 4),
            ("techno-single5", 5),
            ("techno-single8", 8),
            ("techno-double4", 8),
            ("techno-double5", 10),
            ("techno-double8", 16),
            ("pnm-five", 5),
            ("pnm-nine", 9),
            ("kickbox-human", 4),
            ("kickbox-quadarm", 4),
            ("kickbox-insect", 6),
            ("kickbox-arachnid", 8),
        ];
        for (name, lanes) in styles {
            assert_eq!(
                Style::named(name).map(Style::lanes),
                LaneCount::new(lanes),
                "{name}"
            );
        }
        assert_eq!(Style::ALL.len(), styles.len());
    }

    #[test]
    fn a_chart_has_exactly_its_styles_lanes() {
        let mut eight = document(json!([]));
        eight["charts"][0]["lanes"] = json!(8);
        assert!(matches!(read(&eight), Err(Error::Lanes { .. })));
        let mut unknown = document(json!([]));
        unknown["charts"][0]["settings"]["style"] = json!("dance-quad");
        assert!(matches!(read(&unknown), Err(Error::Malformed { .. })));
    }

    #[test]
    fn every_stepmania_difficulty_reads_and_writes() {
        use Difficulty::{Beginner, Challenge, Easy, Edit, Hard, Medium};
        let names = [
            ("beginner", Beginner),
            ("easy", Easy),
            ("medium", Medium),
            ("hard", Hard),
            ("challenge", Challenge),
            ("edit", Edit),
        ];
        for (name, difficulty) in names {
            let mut value = document(json!([]));
            value["charts"][0]["settings"]["difficulty"] = json!(name);
            let song = read(&value).unwrap();
            assert_eq!(song.charts()[0].difficulty, difficulty);
            assert_eq!(
                written(&song)["charts"][0]["settings"]["difficulty"],
                json!(name)
            );
        }
    }

    #[test]
    fn every_stepmania_note_kind_reads_and_writes() {
        let held = Ticks::new(240).unwrap();
        let sustain = json!({ "sustain": { "duration": 240 } });
        let kinds = [
            ("tap", json!("instant"), NoteKind::Tap),
            ("hold", sustain.clone(), NoteKind::Hold(held)),
            ("roll", sustain, NoteKind::Roll(held)),
            ("mine", json!("instant"), NoteKind::Mine),
            ("lift", json!("instant"), NoteKind::Lift),
            ("fake", json!("instant"), NoteKind::Fake),
            ("sm.auto_keysound", json!("instant"), NoteKind::AutoKeysound),
        ];
        for (kind, shape, expected_kind) in kinds {
            let mut object = lane_object(0, 1, kind);
            object["shape"] = shape;
            let objects = json!([object]);
            let song = read(&document(objects.clone())).unwrap();
            assert_eq!(song.charts()[0].notes[0].kind, expected_kind, "{kind}");
            assert_eq!(written(&song), expected(objects), "{kind}");
        }
    }

    #[test]
    fn keysounds_and_their_bank_read_and_write() {
        let mut object = lane_object(0, 1, "sm.auto_keysound");
        object["sample"] = json!("clap");
        let objects = json!([object]);
        let mut value = document(objects.clone());
        value["samples"] = json!({ "clap": "keysounds/clap.wav" });
        let song = read(&value).unwrap();
        assert_eq!(
            song.charts()[0].notes[0].keysound,
            Some(charter_core::SampleId::new("clap").unwrap())
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

    fn event(tick: i32, kind: Value) -> Value {
        let mut event = json!({ "tick": tick });
        event["kind"] = kind;
        event
    }

    #[test]
    fn stops_delays_warps_and_meters_read_and_write() {
        let value = with_events(&[
            event(960, json!({ "stop": 0.5 })),
            event(1920, json!({ "delay": 0.25 })),
            event(2880, json!({ "warp": 480 })),
            event(3840, json!({ "time_signature": [7, 8] })),
        ]);
        let song = read(&value).unwrap();
        let kinds: Vec<&TimingKind> = song
            .timing()
            .events
            .iter()
            .map(|event| &event.kind)
            .collect();
        let nz = |n| std::num::NonZeroU16::new(n).unwrap();
        assert_eq!(
            kinds[..],
            [
                &TimingKind::Stop(Seconds::try_from(0.5).unwrap()),
                &TimingKind::Delay(Seconds::try_from(0.25).unwrap()),
                &TimingKind::Warp(Ticks::new(480).unwrap()),
                &TimingKind::TimeSignature(TimeSignature {
                    numerator: nz(7),
                    denominator: nz(8)
                }),
            ]
        );
        assert_eq!(written(&song)["timing"], value["timing"]);
    }

    fn other(tick: i32, name: &str, value: Value) -> Value {
        let mut kind = json!({ "other": { "name": name } });
        kind["other"]["value"] = value;
        event(tick, kind)
    }

    #[test]
    fn stepmania_timing_segments_read_and_write() {
        let value = with_events(&[
            other(0, "sm.tickcount", json!(4)),
            other(0, "sm.combo", json!({ "hit": 2, "miss": 1 })),
            other(
                960,
                "sm.speed",
                json!({ "ratio": 1.5, "length": { "ticks": 960 } }),
            ),
            other(
                1920,
                "sm.speed",
                json!({ "ratio": 1.0, "length": { "seconds": 0.5 } }),
            ),
            other(1920, "sm.scroll", json!(-1.0)),
            other(2880, "sm.fake", json!(480)),
        ]);
        let song = read(&value).unwrap();
        let kinds: Vec<&TimingKind> = song
            .timing()
            .events
            .iter()
            .map(|event| &event.kind)
            .collect();
        let decimal = |v| Decimal::try_from(v).unwrap();
        assert_eq!(
            kinds[..],
            [
                &TimingKind::TickCount(TickCount::new(4).unwrap()),
                &TimingKind::Combo(Combo { hit: 2, miss: 1 }),
                &TimingKind::Speed(Speed {
                    ratio: decimal(1.5),
                    length: SpeedLength::Ticks(Ticks::new(960).unwrap())
                }),
                &TimingKind::Speed(Speed {
                    ratio: decimal(1.0),
                    length: SpeedLength::Seconds(Seconds::try_from(0.5).unwrap())
                }),
                &TimingKind::Scroll(decimal(-1.0)),
                &TimingKind::Fake(Ticks::new(480).unwrap()),
            ]
        );
        assert_eq!(written(&song)["timing"], value["timing"]);
    }

    #[test]
    fn tick_counts_above_48_are_refused_and_boundary_counts_round_trip() {
        for count in [0, 48] {
            let value = with_events(&[other(0, "sm.tickcount", json!(count))]);
            let song = read(&value).unwrap();
            assert_eq!(written(&song)["timing"], value["timing"]);
        }
        for count in [49, 256, 1024] {
            let value = with_events(&[other(0, "sm.tickcount", json!(count))]);
            assert!(
                matches!(
                    read(&value),
                    Err(Error::Malformed {
                        at: Location::Event {
                            stream: Stream::Song,
                            index: 0
                        },
                        ..
                    })
                ),
                "{count}"
            );
        }
    }

    #[test]
    fn stepmania_segments_must_be_well_formed_and_known() {
        let malformed = with_events(&[other(0, "sm.combo", json!({ "hit": -1, "miss": 1 }))]);
        assert!(matches!(read(&malformed), Err(Error::Malformed { .. })));
        let unknown = with_events(&[other(0, "sm.attacks", json!("x"))]);
        assert!(matches!(read(&unknown), Err(Error::UnknownKey { .. })));
    }

    #[test]
    fn a_chart_can_carry_its_own_timing() {
        let own = json!({
            "anchors": [ { "tick": 0, "seconds": 0.0 } ],
            "tempo": 87.0,
            "events": [
            { "tick": 0, "kind": { "other": { "name": "sm.scroll", "value": 2.0 } } }
        ] });
        let mut value = document(json!([]));
        value["charts"][0]["timing"] = own.clone();
        let song = read(&value).unwrap();
        assert_eq!(
            song.charts()[0].timing.as_ref().map(|t| t.events.len()),
            Some(1)
        );
        assert_eq!(written(&song)["charts"][0]["timing"], own);
    }

    #[test]
    fn a_song_of_another_ruleset_is_refused() {
        let mut value = document(json!([]));
        value["ruleset"]["id"] = json!("ttr");
        assert!(matches!(read(&value), Err(Error::WrongRuleset(id)) if id.as_str() == "ttr"));
    }

    #[test]
    fn objects_stepmania_cannot_play_are_refused() {
        let mut wide = lane_object(0, 0, "tap");
        wide["position"] = json!({ "lane": { "index": 0, "width": 2 } });
        let mut plane = lane_object(0, 0, "tap");
        plane["position"] = json!({ "plane": { "x": 0.0, "y": 0.0 } });
        let mut path = lane_object(0, 0, "tap");
        path["shape"] = json!({ "path": { "points": [
            { "tick": 240, "position": { "lane": { "index": 1 } }, "ease": "linear" } ] } });
        let mut held_mine = lane_object(0, 0, "mine");
        held_mine["shape"] = json!({ "sustain": { "duration": 240 } });
        for object in [wide, plane, path, held_mine, lane_object(0, 0, "sm.attack")] {
            let value = document(json!([object]));
            assert!(
                matches!(read(&value), Err(Error::UnsupportedObject { .. })),
                "{object}"
            );
        }
        let mut parent = lane_object(0, 0, "hold");
        parent["shape"] = json!({ "sustain": { "duration": 480 } });
        parent["children"] = json!([lane_object(240, 1, "tap")]);
        assert!(matches!(
            read(&document(json!([parent]))),
            Err(Error::Unsupported { .. })
        ));
    }

    #[test]
    fn keys_in_the_sm_namespace_or_song_settings_that_it_does_not_define_are_refused() {
        type Edit = fn(&mut Value);
        let edits: [Edit; 4] = [
            |v| v["charts"][0]["objects"][0]["ext"] = json!({ "sm.color": 1 }),
            |v| v["metadata"]["ext"] = json!({ "sm.banner": "b.png" }),
            |v| v["charts"][0]["ext"] = json!({ "sm.radar": [0.5] }),
            |v| v["settings"] = json!({ "selectable": true }),
        ];
        for edit in edits {
            let mut value = document(json!([lane_object(0, 0, "tap")]));
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
        let plain = document(json!([lane_object(0, 0, "tap")]));
        let mut decorated = plain.clone();
        decorated["plugins"] = json!({ "swing": { "version": "1" } });
        decorated["metadata"]["ext"] = json!({ "swing.feel": "late" });
        decorated["charts"][0]["objects"][0]["ext"] = json!({ "swing.accent": true });
        decorated["timing"]["events"]
            .as_array_mut()
            .unwrap()
            .push(other(0, "swing.groove", json!(0.6)));
        let song = read(&decorated).unwrap();
        assert_eq!(song, read(&plain).unwrap());
        assert_eq!(
            written(&song),
            expected(plain["charts"][0]["objects"].clone())
        );
    }

    #[test]
    fn chart_settings_are_closed_and_the_description_optional() {
        let mut radar = document(json!([]));
        radar["charts"][0]["settings"]["radar"] = json!([0.5]);
        assert!(matches!(read(&radar), Err(Error::Malformed { .. })));
        let mut anonymous = document(json!([]));
        anonymous["charts"][0]["settings"]
            .as_object_mut()
            .unwrap()
            .remove("description");
        let song = read(&anonymous).unwrap();
        assert_eq!(song.charts()[0].description, "");
        let mut out = anonymous.clone();
        out["ruleset"]["version"] = json!(VERSION);
        out["charts"][0].as_object_mut().unwrap().remove("objects");
        assert_eq!(written(&song), out);
    }

    #[test]
    fn a_speed_change_takes_no_time_only_when_marked_instant() {
        let instant = json!({ "ratio": 2.0, "length": "instant" });
        let value = with_events(&[other(960, "sm.speed", instant)]);
        assert_eq!(written(&read(&value).unwrap())["timing"], value["timing"]);
        for zero in [json!({ "ticks": 0 }), json!({ "seconds": 0.0 })] {
            let mut speed = json!({ "ratio": 2.0 });
            speed["length"] = zero;
            read(&with_events(&[other(960, "sm.speed", speed)])).unwrap_err();
        }
    }

    #[test]
    fn styles_are_variants_named_as_stepmania_names_them() {
        assert_eq!(Style::named("dance-single"), Some(Style::DanceSingle));
        assert_eq!(Style::PumpRoutine.name(), "pump-routine");
        assert_eq!(Style::PumpRoutine.lanes(), LaneCount::new(20).unwrap());
    }

    #[test]
    fn errors_say_where_and_which_key() {
        let mut value = document(json!([]));
        value["timing"]["events"] = json!([{ "tick": 0, "kind": { "time_signature": [4, 4] }, "ext": { "sm.color": "red" } }]);
        let color: Key = "sm.color".parse().unwrap();
        assert!(matches!(
            read(&value),
            Err(Error::UnknownKey { at: Location::Event { stream: Stream::Song, index: 0 }, key })
                if key == color
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
        let song = read(&document(json!([lane_object(0, 1, "tap")]))).unwrap();
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
        outside[0].notes[0].lane = LaneIndex::new(3 + 1);
        let mut tagged = song.metadata().clone();
        tagged
            .ext
            .insert("sm.genre".parse().unwrap(), json!("rock"));
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
        own["markers"] = json!([{ "tick": 0, "kind": "sm.drop", "text": "" }]);
        assert!(matches!(
            read(&own),
            Err(Error::UnknownKey {
                at: Location::Marker(0),
                ..
            })
        ));
    }

    #[test]
    fn labels_travel_as_section_markers_rather_than_timing_segments() {
        let label = with_events(&[other(0, "sm.label", json!("drop"))]);
        assert!(matches!(read(&label), Err(Error::UnknownKey { .. })));
    }

    fn new_song(layout: usize, charts: &[&str]) -> charter_core::NewSong {
        charter_core::NewSong {
            metadata: Metadata {
                title: "t".to_owned(),
                artist: "a".to_owned(),
                audio: ResourcePath::new("audio.ogg").unwrap(),
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
    fn a_new_song_offers_every_style_with_dance_single_first() {
        let layouts = Plugin.layouts();
        assert_eq!(layouts.len(), Style::ALL.len());
        assert_eq!(layouts[0].name, "dance-single");
        assert_eq!(layouts[0].lanes, LaneCount::new(4).unwrap());
        assert_eq!(Plugin.layout_kind(), "Style");
        assert_eq!(Plugin.name(), "StepMania");
    }

    #[test]
    fn a_new_song_gives_every_chart_its_style_and_a_difficulty_named_like_the_chart() {
        let pump = Style::ALL
            .iter()
            .position(|style| *style == Style::PumpSingle)
            .unwrap();
        let document = Plugin.new_song(&new_song(pump, &["Hard", "Wild"])).unwrap();
        Plugin.check(&document).unwrap();
        let song = Song::try_from(&document).unwrap();
        let charts = song.charts();
        assert_eq!(charts.len(), 2);
        assert_eq!(
            (charts[0].name.as_str(), charts[0].style),
            ("Hard", Style::PumpSingle)
        );
        assert_eq!(charts[0].difficulty, Difficulty::Hard);
        assert_eq!(charts[1].difficulty, Difficulty::Edit);
        assert!(charts.iter().all(|chart| chart.notes.is_empty()));
        assert_eq!(document.document().charts[0].lanes, LaneCount::new(5));
    }

    #[test]
    fn a_new_song_starts_at_120_bpm_in_four_four() {
        let document = Plugin.new_song(&new_song(0, &["Easy"])).unwrap();
        let events = &document.document().timing.events;
        assert_eq!(
            document.document().timing.tempo,
            Bpm::try_from(120.0).unwrap()
        );
        assert_eq!(events.len(), 1);
        assert!(events.iter().all(|event| event.tick == Tick::new(0)));
    }

    #[test]
    fn a_new_song_refuses_a_style_it_does_not_have() {
        Plugin
            .new_song(&new_song(Style::ALL.len(), &["Easy"]))
            .unwrap_err();
    }
}

/// Proofs for Kani, run with `cargo kani -p charter-stepmania`.
#[cfg(kani)]
mod proofs {
    use super::*;

    #[kani::proof]
    #[kani::unwind(40)]
    fn every_steps_type_reads_back_by_name_with_a_playable_lane_count() {
        let style = Style::ALL[kani::any_where(|index: &usize| *index < Style::ALL.len())];
        assert!(Style::named(style.name()) == Some(style));
        let width = u16::from(style.lanes());
        assert!((1..=20).contains(&width));
    }

    #[kani::proof]
    fn routine_columns_double_without_overflow() {
        let columns: u16 = kani::any();
        if columns <= u16::MAX / 2 {
            assert!(u32::from(routine(columns)) == u32::from(columns) * 2);
        } else {
            assert!(routine(columns) == u16::MAX);
        }
        assert!(routine(columns) >= columns);
    }
}
