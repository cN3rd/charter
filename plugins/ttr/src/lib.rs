//! Tap Tap Revenge as a charter ruleset.
//!
//! A chart has 3, 4 or 5 lanes, and a song's charts may differ. Notes scroll at a constant speed in
//! time, so a tempo change only moves the beat grid and never speeds play up or slows it down the
//! way StepMania's does. Stops, delays and warps have no meaning here.
//!
//! [`Song`] is the typed view of a document whose ruleset is [`ID`]. Reading one fails on
//! anything Tap Tap Revenge cannot represent and skips data other plugins left behind;
//! [`Song::to_document`] writes this ruleset's data alone. A song is checked when built, by
//! reading or by [`Song::new`], so writing one cannot fail. [`Plugin`] hands the editor the same
//! check, so a document is editable only while it reads as a song.
//!
//! [`Plugin`] also imports and exports songs as the game's `.ttr2_track` files and as the MIDI
//! files Editor on Fire charts them in, both through the shared model in `exchange`.

mod exchange;
mod midi;
mod track;

use std::collections::BTreeMap;

use serde_json::Value;

use charter_core::{
    Anchor, Bpm, Chart as CoreChart, Difficulties, DifficultyPreset, Document, ExportedSong,
    Extensions, FORMAT_VERSION, ImportedSong, InvalidValue, Key, Kind, LaneCount, LaneIndex,
    Layout, Location, Marker, Metadata, Name, Namespace, NewSong, NoteChange, Object, Origin,
    Position, Ruleset, RulesetPlugin, SettingScope, Settings, Shape, SongFormat, SongResource,
    Stream, TextSetting, Tick, Ticks, TicksPerBeat, TimeSignature, Timing, TimingEvent, TimingKind,
    ValidDocument,
};

/// The ruleset namespace.
const ID: &str = "ttr";

/// Property tests run from a fixed seed and keep no regression files, so they repeat exactly
/// and touch no files.
#[cfg(test)]
fn proptest_config() -> proptest::test_runner::Config {
    proptest::test_runner::Config {
        failure_persistence: None,
        rng_seed: proptest::test_runner::RngSeed::Fixed(0),
        ..proptest::test_runner::Config::default()
    }
}

/// A song, checked against this ruleset and every document rule when built, and unchangeable
/// after.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Song {
    /// Header values shared by all the song's charts.
    metadata: Metadata,
    /// Number of ticks in one quarter-note beat.
    ticks_per_beat: TicksPerBeat,
    /// Tempo used by charts without their own map.
    tempo: Tempo,
    /// Song-wide section and bookmark positions.
    markers: Vec<Marker>,
    /// Levels checked against this ruleset.
    charts: Vec<Chart>,
    /// The theme every level plays in unless it names its own.
    theme: Option<Name>,
}

/// Supported chart column counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lanes {
    /// Three-column chart.
    Three,
    /// Four-column chart.
    Four,
    /// Five-column chart.
    Five,
}

/// A song's tempo map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tempo {
    /// Audio positions pinned to chart ticks.
    pub anchors: Vec<Anchor>,
    /// Tempo after the last anchor.
    pub tempo: Bpm,
    /// Metre changes.
    pub changes: Vec<TempoChange>,
}

/// A change to the song's timing grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TempoChange {
    /// Position of the change.
    pub tick: Tick,
    /// Effect of the change.
    pub kind: TempoKind,
}

/// Timing changes supported by Tap Tap Revenge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TempoKind {
    /// Beats and beat unit per measure.
    TimeSignature(TimeSignature),
}

/// One difficulty.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chart {
    /// How many lanes the chart has, which the game calls its columns.
    pub lanes: Lanes,
    /// What the game calls the chart, which Tap Tap Reloaded and Tap Tap Hardcore name their own
    /// way.
    pub name: String,
    /// The standard level the chart is, whatever it is called, when it is one.
    pub difficulty: Option<Difficulty>,
    /// The theme this level plays in, over the song's.
    ///
    /// ```
    /// # use charter_core::{Bpm, Name};
    /// # use charter_ttr::{Chart, Lanes, Tempo};
    /// let theme = Name::new("TTRDJ").unwrap();
    /// let _ = Chart {
    ///     lanes: Lanes::Four,
    ///     name: "Easy".to_owned(),
    ///     difficulty: None,
    ///     theme: Some(theme),
    ///     tempo: Some(Tempo { anchors: Vec::new(), tempo: Bpm::try_from(120.0).unwrap(), changes: Vec::new() }),
    ///     notes: Vec::new(),
    /// };
    /// ```
    ///
    /// A theme is a validated name, not raw text:
    ///
    /// ```compile_fail
    /// # use charter_ttr::{Chart, Lanes};
    /// let _ = Chart {
    ///     lanes: Lanes::Four,
    ///     name: "Easy".to_owned(),
    ///     difficulty: None,
    ///     theme: Some("TTRDJ".to_owned()),
    ///     tempo: None,
    ///     notes: Vec::new(),
    /// };
    /// ```
    pub theme: Option<Name>,
    /// Replaces the song's tempo map when present.
    pub tempo: Option<Tempo>,
    /// Notes in this level's lanes.
    pub notes: Vec<Note>,
}

/// A chart's standard level, easiest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Difficulty {
    /// Entry-level chart.
    Easy,
    /// Intermediate chart.
    Medium,
    /// Advanced chart.
    Hard,
    /// Highest standard level.
    Extreme,
}

impl Difficulty {
    /// Standard levels in ascending order.
    pub const ALL: [Self; 4] = [Self::Easy, Self::Medium, Self::Hard, Self::Extreme];

    /// Zero-based place in the standard level order.
    const fn index(self) -> usize {
        match self {
            Self::Easy => 0,
            Self::Medium => 1,
            Self::Hard => 2,
            Self::Extreme => 3,
        }
    }

    /// The level's standard name, as Tap Tap Revenge shows it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Easy => "Easy",
            Self::Medium => "Medium",
            Self::Hard => "Hard",
            Self::Extreme => "Extreme",
        }
    }

    /// The level as the `difficulty` chart setting stores it.
    const fn key(self) -> &'static str {
        match self {
            Self::Easy => "easy",
            Self::Medium => "medium",
            Self::Hard => "hard",
            Self::Extreme => "extreme",
        }
    }

    /// The standard level stored under `key`, if recognised.
    fn keyed(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|level| level.key() == key)
    }
}

/// The chart setting holding a chart's [`Difficulty`].
const DIFFICULTY: &str = "difficulty";

/// The song and chart setting naming the theme a level plays in: the song's for every level
/// that names none, a chart's for that level alone.
const THEME: &str = "theme";

/// The theme a new song and each of its levels start in: the game's own.
pub const DEFAULT_THEME: &str = "TTRDJ";

/// The sets of levels a new song can start with, and what each game calls their charts: custom
/// charts come in standard Extreme alone or every level, Tap Tap Reloaded has no Easy, and Tap
/// Tap Hardcore renames all four.
const PRESETS: [(&str, &[(Difficulty, &str)]); 4] = [
    ("Custom, Extreme only", &[(Difficulty::Extreme, "Extreme")]),
    (
        "Custom, all levels",
        &[
            (Difficulty::Easy, "Easy"),
            (Difficulty::Medium, "Medium"),
            (Difficulty::Hard, "Hard"),
            (Difficulty::Extreme, "Extreme"),
        ],
    ),
    (
        "Tap Tap Reloaded",
        &[
            (Difficulty::Medium, "Metallica"),
            (Difficulty::Hard, "Downloads"),
            (Difficulty::Extreme, "Customs"),
        ],
    ),
    (
        "Tap Tap Hardcore",
        &[
            (Difficulty::Easy, "Extreme"),
            (Difficulty::Medium, "Intense"),
            (Difficulty::Hard, "Severe"),
            (Difficulty::Extreme, "Customs"),
        ],
    ),
];

/// One note on a chart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Note {
    /// Tick at which the note starts.
    pub tick: Tick,
    /// What the player does at that tick.
    pub kind: NoteKind,
}

/// Notes Tap Tap Revenge can play.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteKind {
    /// Single-column tap.
    Tap {
        /// Column to press.
        lane: LaneIndex,
    },
    /// Sustained single-column press.
    Hold {
        /// Column to hold.
        lane: LaneIndex,
        /// Length of the hold.
        duration: Ticks,
    },
    /// Shake the device in this direction.
    Shake(Shake),
    /// Slice across the note.
    Slice(Side),
    /// Tapped anywhere across the whole tap area.
    MegaTap,
}

/// A horizontal direction in the tap area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// Left side.
    Left,
    /// Right side.
    Right,
}

/// Direction in which to shake the device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shake {
    /// Shake left.
    Left,
    /// Shake right.
    Right,
    /// Shake down.
    Down,
}

/// Written as the ruleset version.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Why a document cannot be represented as a Tap Tap Revenge song.
#[expect(
    clippy::error_impl_error,
    reason = "public error type is part of the plugin API"
)]
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Document belongs to another ruleset.
    #[error("ruleset is \"{0}\", not {ID:?}")]
    WrongRuleset(Namespace),
    /// Songs must have a chart.
    #[error("a tap tap revenge song needs at least one chart")]
    NoCharts,
    /// Note shape or position this game cannot play.
    #[error("{at}: not a tap tap revenge note")]
    UnsupportedObject {
        /// Location of the note.
        at: Location,
    },
    /// Unsupported document feature.
    #[error("{at}: {what} are not part of tap tap revenge")]
    Unsupported {
        /// Location of the feature.
        at: Location,
        /// Name of the feature.
        what: &'static str,
    },
    /// Unrecognised plugin key.
    #[error("{at}: \"{key}\" is not a tap tap revenge key")]
    UnknownKey {
        /// Location of the key.
        at: Location,
        /// Unrecognised key.
        key: Key,
    },
    /// Unrecognised chart or song setting.
    #[error("{at}: \"{key}\" is not a tap tap revenge setting")]
    UnknownSetting {
        /// Location of the setting.
        at: Location,
        /// Unrecognised setting.
        key: String,
    },
    /// Difficulty other than the four standard levels.
    #[error("{at}: the difficulty {found} is not easy, medium, hard or extreme")]
    Difficulty {
        /// Location of the difficulty.
        at: Location,
        /// Unsupported value.
        found: String,
    },
    /// Invalid theme name.
    #[error("{at}: a theme is a name, such as DeltaHeavyTheme, not {found}")]
    Theme {
        /// Location of the theme.
        at: Location,
        /// Invalid value.
        found: String,
    },
    /// Two shakes, slices or mega taps cover one lane on one tick.
    #[error("{at}: two shakes, slices or mega taps on one lane at once")]
    Crowded {
        /// Location of the second.
        at: Location,
    },
    /// Chart with an unsupported column count.
    #[error("{at} has {} lanes; a chart has 3, 4 or 5", .lanes.map_or_else(|| "no".to_owned(), |lanes| lanes.to_string()))]
    Lanes {
        /// Location of the chart.
        at: Location,
        /// Column count, if specified.
        lanes: Option<LaneCount>,
    },
    /// Invalid format-level value.
    #[error(transparent)]
    Value(#[from] InvalidValue),
    /// Invalid chart document.
    #[error(transparent)]
    Document(#[from] charter_core::Error),
}

impl Song {
    /// A song of `charts`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCharts`] without a chart, [`Error::UnknownKey`] for metadata extension
    /// data, which this ruleset does not keep, or [`Error::Document`] for the first document rule
    /// the song breaks, such as a tap in a lane its chart does not have.
    pub fn new(
        metadata: Metadata,
        ticks_per_beat: TicksPerBeat,
        tempo: Tempo,
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
        if charts.is_empty() {
            return Err(Error::NoCharts);
        }
        for (c, chart) in charts.iter().enumerate() {
            uncrowded(c, chart)?;
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
            tempo,
            markers,
            charts,
            theme: None,
        };
        song.build()?;
        Ok(song)
    }

    /// Header values shared by all charts.
    #[must_use]
    pub const fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// Number of ticks in one quarter-note beat.
    #[must_use]
    pub const fn ticks_per_beat(&self) -> TicksPerBeat {
        self.ticks_per_beat
    }

    /// Song-wide tempo map, before chart overrides.
    #[must_use]
    pub const fn tempo(&self) -> &Tempo {
        &self.tempo
    }

    /// Levels in this song.
    #[must_use]
    pub fn charts(&self) -> &[Chart] {
        &self.charts
    }

    /// The theme every level plays in unless it names its own.
    #[must_use]
    pub fn theme(&self) -> Option<&str> {
        self.theme.as_ref().map(Name::as_str)
    }

    /// The song with every level playing in `theme` unless it names its own.
    #[must_use]
    pub fn with_theme(mut self, theme: Option<Name>) -> Self {
        self.theme = theme;
        self
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

    /// Validates and writes this song using only Tap Tap Revenge data.
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
            settings: themed(self.theme.as_ref()),
            plugins: BTreeMap::new(),
            metadata: self.metadata.clone(),
            samples: BTreeMap::new(),
            timing: self.tempo.to_timing(),
            markers: self.markers.clone(),
            charts,
        };
        Ok(ValidDocument::new(doc)?)
    }
}

impl Lanes {
    /// How many lanes a chart of this count has.
    #[must_use]
    pub const fn width(self) -> LaneCount {
        match self {
            Self::Three => lanes(3),
            Self::Four => lanes(4),
            Self::Five => lanes(5),
        }
    }

    /// The supported lane count matching `width`, if any.
    fn of(width: LaneCount) -> Option<Self> {
        LANES.into_iter().find(|lanes| lanes.width() == width)
    }

    /// Where a note of `kind` sits. A shake or slice takes the lane of its direction; a down
    /// shake takes the middle lane, or both middle lanes of four. A mega tap covers every lane.
    #[must_use]
    pub const fn position(self, kind: NoteKind) -> Position {
        let width = self.width();
        match kind {
            NoteKind::Tap { lane } | NoteKind::Hold { lane, .. } => one_lane(lane),
            NoteKind::Shake(Shake::Left) | NoteKind::Slice(Side::Left) => {
                one_lane(LaneIndex::new(0))
            }
            NoteKind::Shake(Shake::Right) | NoteKind::Slice(Side::Right) => one_lane(width.last()),
            NoteKind::Shake(Shake::Down) => match self {
                Self::Four => Position::Lane {
                    index: width.middle(),
                    width: lanes(2),
                },
                Self::Three | Self::Five => one_lane(width.middle()),
            },
            NoteKind::MegaTap => Position::Lane {
                index: LaneIndex::new(0),
                width,
            },
        }
    }
}

/// `n` lanes, for the counts this ruleset names.
const fn lanes(n: u16) -> LaneCount {
    #[expect(
        clippy::unreachable,
        reason = "only positive constant lane counts call this"
    )]
    match LaneCount::new(n) {
        Some(lanes) => lanes,
        None => unreachable!(),
    }
}

/// Ruleset key for a directional shake.
const SHAKE: &str = "shake";
/// Ruleset key for a directional slice.
const SLICE: &str = "slice";
/// Ruleset key for a tap spanning every lane.
const MEGA_TAP: &str = "mega_tap";

/// This ruleset's kind called `name`.
fn own(name: &str) -> Result<Kind, InvalidValue> {
    Ok(Kind::Other(Key::new(Namespace::new(ID)?, Name::new(name)?)))
}

/// One-lane position starting at `index`.
const fn one_lane(index: LaneIndex) -> Position {
    Position::Lane {
        index,
        width: LaneCount::ONE,
    }
}

impl Tempo {
    /// Converts the supported meter changes into the shared timing map.
    fn to_timing(&self) -> Timing {
        let events = self.changes.iter().map(|change| TimingEvent {
            tick: change.tick,
            kind: match change.kind {
                TempoKind::TimeSignature(meter) => TimingKind::TimeSignature(meter),
            },
            ext: Extensions::new(),
        });
        Timing {
            anchors: self.anchors.clone(),
            tempo: self.tempo,
            events: events.collect(),
        }
    }
}

impl Chart {
    /// Writes the chart's settings, optional tempo override and placed notes.
    fn to_core(&self) -> Result<CoreChart, InvalidValue> {
        let lanes = self.lanes;
        Ok(CoreChart {
            name: self.name.clone(),
            lanes: Some(lanes.width()),
            settings: {
                let mut settings = themed(self.theme.as_ref());
                if let Some(level) = self.difficulty {
                    settings.insert(DIFFICULTY.to_owned(), Value::from(level.key()));
                }
                settings
            },
            timing: self.tempo.as_ref().map(Tempo::to_timing),
            // TTR has no layers; writing its game model exports the merged chart.
            layers: Vec::new(),
            objects: self
                .notes
                .iter()
                .map(|note| note.to_object(lanes))
                .collect::<Result<_, _>>()?,
            ext: Extensions::new(),
        })
    }
}

impl Note {
    /// Writes a note with its position derived from `lanes`.
    fn to_object(self, lanes: Lanes) -> Result<Object, InvalidValue> {
        let (kind, shape) = match self.kind {
            NoteKind::Tap { .. } => (Kind::Tap, Shape::Instant),
            NoteKind::Hold { duration, .. } => (Kind::Hold, Shape::Sustain { duration }),
            NoteKind::Shake(_) => (own(SHAKE)?, Shape::Instant),
            NoteKind::Slice(_) => (own(SLICE)?, Shape::Instant),
            NoteKind::MegaTap => (own(MEGA_TAP)?, Shape::Instant),
        };
        Ok(Object {
            tick: self.tick,
            position: lanes.position(self.kind),
            kind,
            shape,
            // Layer identity cannot be recovered from a TTR note.
            layer: None,
            sample: None,
            children: Vec::new(),
            ext: Extensions::new(),
        })
    }
}

impl TryFrom<&ValidDocument> for Song {
    type Error = Error;

    fn try_from(valid: &ValidDocument) -> Result<Self, Error> {
        let doc = valid.document();
        if doc.ruleset.id.as_str() != ID {
            return Err(Error::WrongRuleset(doc.ruleset.id.clone()));
        }
        reject_keys(
            doc.settings.keys().filter(|key| *key != THEME),
            Location::Settings,
        )?;
        let theme = named(doc.settings.get(THEME), Location::Settings)?;
        reject_own_keys(&doc.metadata.ext, Location::MetadataExt)?;
        if !doc.samples.is_empty() {
            return Err(Error::Unsupported {
                at: Location::Samples,
                what: "keysounds",
            });
        }
        if doc.charts.is_empty() {
            return Err(Error::NoCharts);
        }
        let charts = doc
            .charts
            .iter()
            .enumerate()
            .map(|(c, chart)| read_chart(c, chart))
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
        let tempo = read_tempo(&doc.timing, Stream::Song)?;
        Ok(Self::new(metadata, doc.ticks_per_beat, tempo, markers, charts)?.with_theme(theme))
    }
}

/// This ruleset for the editor, which checks a document by reading it as a [`Song`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Plugin;

/// The song formats [`Plugin`] reads and writes, in the order it lists them: a file, and how its
/// keys are played.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SongFile {
    /// The game's own `.ttr2_track`, its keys played as the preset's game plays them.
    Track(exchange::Preset),
    /// A MIDI file as Editor on Fire charts the game, its keys laid out as given.
    Midi(midi::Layout),
}

/// Every song format, the game's own first.
const SONG_FILES: [SongFile; 5] = [
    SongFile::Track(exchange::Preset::Revenge),
    SongFile::Track(exchange::Preset::Tour),
    SongFile::Track(exchange::Preset::TapThemes),
    SongFile::Midi(midi::Layout::Standard),
    SongFile::Midi(midi::Layout::Reloaded),
];

impl SongFile {
    /// The format as the editor lists it.
    fn format(self) -> SongFormat {
        use exchange::Preset;
        let name = match self {
            Self::Track(Preset::Revenge) => "Tap Tap Revenge track",
            Self::Track(Preset::Tour) => "Tap Tap Revenge: Tour track",
            Self::Track(Preset::TapThemes) => "Tap Tap Revenge track, custom tap theme",
            Self::Midi(midi::Layout::Standard) => "Editor on Fire MIDI",
            Self::Midi(midi::Layout::Reloaded) => "Tap Tap Reloaded MIDI",
        };
        let (extensions, companions): (&[&str], &[&str]) = match self {
            Self::Track(_) => (&["ttr2_track"], &["info.plist"]),
            Self::Midi(_) => (&["mid", "midi"], &[]),
        };
        let owned = |names: &[&str]| names.iter().map(|&each| each.to_owned()).collect();
        SongFormat {
            name: name.to_owned(),
            extensions: owned(extensions),
            companions: owned(companions),
        }
    }

    /// How this format's keys are played. Tap Tap Reloaded's middle note is the big note
    /// Tour's mega tap is.
    const fn preset(self) -> exchange::Preset {
        match self {
            Self::Track(preset) => preset,
            Self::Midi(midi::Layout::Standard) => exchange::Preset::Revenge,
            Self::Midi(midi::Layout::Reloaded) => exchange::Preset::Tour,
        }
    }

    /// The audio file a song of this format plays, where the game or EOF keeps it beside the
    /// chart.
    const fn audio(self) -> &'static str {
        match self {
            Self::Track(_) => "audio.m4a",
            Self::Midi(_) => "guitar.ogg",
        }
    }

    /// The format at `index` among [`SONG_FILES`].
    fn at(index: usize) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        SONG_FILES
            .get(index)
            .copied()
            .ok_or_else(|| "Tap Tap Revenge has no such song format".into())
    }
}

#[expect(
    clippy::missing_trait_methods,
    reason = "Tap Tap Revenge uses the trait's defaults for `layout_kind`, Lanes, and `timing_format` and `import_timing`: it reads no timing format alone"
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
        "Tap Tap Revenge"
    }

    fn layouts(&self) -> Vec<Layout> {
        LANES
            .iter()
            .map(|lanes| Layout {
                name: format!("{} lanes", lanes.width()),
                lanes: lanes.width(),
            })
            .collect()
    }

    fn note_changes(&self) -> Vec<NoteChange> {
        let change =
            |name: &str, about: &str, apply: fn(&Object, LaneCount) -> Option<Object>| NoteChange {
                name: name.to_owned(),
                about: about.to_owned(),
                apply,
            };
        vec![
            change(
                "Arrow",
                "A shake: left on the first lane, right on the last, down between",
                to_arrow,
            ),
            change(
                "Slice",
                "A slice: left on the left half of the lanes, right on the right",
                to_slice,
            ),
            change("Big tap", "A mega tap across every lane", to_mega_tap),
            change("Tap", "A tap on the note's first lane", to_tap),
        ]
    }

    fn text_settings(&self) -> Vec<TextSetting> {
        let theme = |scope, about: &str| TextSetting {
            key: THEME.to_owned(),
            name: "Theme".to_owned(),
            about: about.to_owned(),
            scope,
        };
        vec![
            theme(
                SettingScope::Song,
                "The theme every level plays in unless it names its own; new songs start in TTRDJ",
            ),
            theme(
                SettingScope::Chart,
                "The theme this level plays in, over the song's; new levels start in TTRDJ",
            ),
        ]
    }

    fn difficulties(&self) -> Option<Difficulties> {
        let presets = PRESETS
            .iter()
            .map(|(name, charts)| DifficultyPreset {
                name: (*name).to_owned(),
                charts: charts
                    .iter()
                    .map(|&(level, chart)| (level.index(), chart.to_owned()))
                    .collect(),
            })
            .collect();
        Some(Difficulties {
            levels: Difficulty::ALL
                .iter()
                .map(|level| level.name().to_owned())
                .collect(),
            presets,
        })
    }

    fn song_formats(&self) -> Vec<SongFormat> {
        SONG_FILES.iter().map(|file| file.format()).collect()
    }

    fn import_song(
        &self,
        format: usize,
        bytes: &[u8],
        companions: &BTreeMap<String, Vec<u8>>,
    ) -> Result<ImportedSong, Box<dyn std::error::Error + Send + Sync>> {
        let file = SongFile::at(format)?;
        let mut report = exchange::Report::default();
        let track = match file {
            SongFile::Track(_) => {
                let info = companions.get("info.plist").map(Vec::as_slice);
                track::read(bytes, info, &mut report)?
            }
            SongFile::Midi(layout) => midi::read(bytes, layout, &mut report)?,
        };
        let document = exchange::to_song(track, file.audio(), file.preset(), &mut report)?;
        Ok(ImportedSong {
            document,
            left_out: report.into_lines(),
        })
    }

    fn export_song(
        &self,
        format: usize,
        song: &ValidDocument,
        audio_seconds: Option<f64>,
    ) -> Result<ExportedSong, Box<dyn std::error::Error + Send + Sync>> {
        let file = SongFile::at(format)?;
        let parsed = Song::try_from(song)?;
        let mut report = exchange::Report::default();
        let track = exchange::from_song(song, file.preset(), &mut report)?;
        let metadata = parsed.metadata();
        if metadata.preview.is_some() {
            report.add("the preview, which the game has no place for");
        }
        let exported = match file {
            SongFile::Track(_) => track_folder(&parsed, &track, audio_seconds, &mut report),
            SongFile::Midi(layout) => {
                let themed = parsed.theme().is_some()
                    || parsed.charts().iter().any(|chart| chart.theme.is_some());
                let elsewhere = [
                    ("the artist", !metadata.artist.is_empty()),
                    ("the genre", metadata.genre.is_some()),
                    ("the artwork", metadata.artwork.is_some()),
                    ("the theme", themed),
                ];
                for (what, set) in elsewhere {
                    if set {
                        report.add(format!("{what}, which a MIDI file has no place for"));
                    }
                }
                ExportedSong {
                    bytes: midi::write(&track, layout, &mut report),
                    file_name: None,
                    beside: BTreeMap::new(),
                    copies: Vec::new(),
                    left_out: Vec::new(),
                }
            }
        };
        Ok(ExportedSong {
            left_out: report.into_lines(),
            ..exported
        })
    }

    fn new_song(
        &self,
        song: &NewSong,
    ) -> Result<ValidDocument, Box<dyn std::error::Error + Send + Sync>> {
        let lanes = *LANES
            .get(song.layout)
            .ok_or("Tap Tap Revenge has no such lane count")?;
        let default_theme = Name::new(DEFAULT_THEME)?;
        let charts = song
            .charts
            .iter()
            .map(|chart| {
                let difficulty = match chart.level {
                    None => None,
                    Some(level) => Some(
                        *Difficulty::ALL
                            .get(level)
                            .ok_or("Tap Tap Revenge has no such difficulty")?,
                    ),
                };
                Ok::<_, &str>(Chart {
                    lanes,
                    name: chart.name.clone(),
                    difficulty,
                    theme: Some(default_theme.clone()),
                    tempo: None,
                    notes: Vec::new(),
                })
            })
            .collect::<Result<_, _>>()?;
        let at_zero = |kind| TempoChange {
            tick: Tick::new(0),
            kind,
        };
        let tempo = Tempo {
            anchors: vec![Anchor {
                tick: Tick::new(0),
                seconds: Origin::try_from(0.0)?,
            }],
            tempo: Bpm::try_from(120.0)?,
            changes: vec![at_zero(TempoKind::TimeSignature(TimeSignature::default()))],
        };
        let song = Song::new(
            song.metadata.clone(),
            TicksPerBeat::default(),
            tempo,
            Vec::new(),
            charts,
        )?
        .with_theme(Some(default_theme));
        Ok(song.to_document())
    }
}

/// `track`, of `song`, as the folder the game keeps a track in: `taptrack.ttr2_track`, its
/// `info.plist`, and the song's audio and artwork copied in as the game names them, when the game
/// can play them; what it cannot is said in `report`.
fn track_folder(
    song: &Song,
    track: &exchange::Track,
    audio_seconds: Option<f64>,
    report: &mut exchange::Report,
) -> ExportedSong {
    let metadata = song.metadata();
    let extension = |path: &str| {
        std::path::Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_lowercase)
            .unwrap_or_default()
    };
    let mut copies = vec![("audio.m4a".to_owned(), SongResource::AacAudio)];
    if let Some(artwork) = &metadata.artwork {
        if extension(artwork.as_str()) == "png" {
            for name in ["artwork.png", "artwork@2x.png"] {
                copies.push((name.to_owned(), SongResource::Artwork));
            }
        } else {
            report.add("the artwork, which the game reads as a PNG");
        }
    }
    if audio_seconds.is_none() {
        report.add("how long the audio lasts, which info.plist holds, as the audio has not loaded");
    }
    let beside = BTreeMap::from([(
        "info.plist".to_owned(),
        track::info(song, track, audio_seconds),
    )]);
    ExportedSong {
        bytes: track::write(track),
        file_name: Some("taptrack.ttr2_track".to_owned()),
        beside,
        copies,
        left_out: Vec::new(),
    }
}

/// Every lane count a song can have.
const LANES: [Lanes; 3] = [Lanes::Three, Lanes::Four, Lanes::Five];

/// Reads chart `c`, rejecting settings and objects this ruleset cannot represent.
fn read_chart(c: usize, chart: &CoreChart) -> Result<Chart, Error> {
    let lanes = chart.lanes.and_then(Lanes::of).ok_or(Error::Lanes {
        at: Location::Chart(c),
        lanes: chart.lanes,
    })?;
    let at = Location::ChartSettings(c);
    reject_keys(
        chart
            .settings
            .keys()
            .filter(|key| *key != DIFFICULTY && *key != THEME),
        at,
    )?;
    let theme = named(chart.settings.get(THEME), at)?;
    let difficulty =
        match chart.settings.get(DIFFICULTY) {
            None => None,
            Some(value) => Some(value.as_str().and_then(Difficulty::keyed).ok_or_else(|| {
                Error::Difficulty {
                    at,
                    found: value.to_string(),
                }
            })?),
        };
    reject_own_keys(&chart.ext, Location::ChartExt(c))?;
    let notes: Vec<Note> = chart
        .objects
        .iter()
        .enumerate()
        .map(|(j, object)| {
            let object_at = Location::Object { chart: c, index: j };
            reject_own_keys(&object.ext, object_at)?;
            if !object.children.is_empty() {
                return Err(Error::Unsupported {
                    at: object_at,
                    what: "child objects",
                });
            }
            let kind =
                read_note(object, lanes).ok_or(Error::UnsupportedObject { at: object_at })?;
            Ok(Note {
                tick: object.tick,
                kind,
            })
        })
        .collect::<Result<_, Error>>()?;
    let tempo = match &chart.timing {
        Some(timing) => Some(read_tempo(timing, Stream::Chart(c))?),
        None => None,
    };
    Ok(Chart {
        lanes,
        name: chart.name.clone(),
        difficulty,
        theme,
        tempo,
        notes,
    })
}

/// Fails on two shakes, slices or mega taps of chart `c` covering one lane on one tick. The
/// format leaves this ruleset's own kinds to share lanes with taps and holds, as a shake shares
/// the column of its direction and the game plays both; two of them on one lane are a mistake.
fn uncrowded(c: usize, chart: &Chart) -> Result<(), Error> {
    let mut gestures: std::collections::HashSet<(i64, u16)> = std::collections::HashSet::new();
    for (j, note) in chart.notes.iter().enumerate() {
        if matches!(note.kind, NoteKind::Tap { .. } | NoteKind::Hold { .. }) {
            continue;
        }
        let Position::Lane { index, width } = chart.lanes.position(note.kind) else {
            continue;
        };
        let first = u16::from(index);
        for lane in first..first.saturating_add(u16::from(width)) {
            if !gestures.insert((i64::from(note.tick), lane)) {
                return Err(Error::Crowded {
                    at: Location::Object { chart: c, index: j },
                });
            }
        }
    }
    Ok(())
}

/// The note type at its expected position for `lanes`, if representable.
fn read_note(object: &Object, lanes: Lanes) -> Option<NoteKind> {
    let placed = |candidates: &[NoteKind]| {
        candidates
            .iter()
            .copied()
            .find(|&kind| lanes.position(kind) == object.position)
    };
    let single_lane = match object.position {
        Position::Lane { index, width } if width == LaneCount::ONE => Some(index),
        Position::Lane { .. } | Position::Plane { .. } => None,
    };
    match (&object.kind, &object.shape) {
        (Kind::Tap, Shape::Instant) => single_lane.map(|lane| NoteKind::Tap { lane }),
        (Kind::Hold, &Shape::Sustain { duration }) => {
            single_lane.map(|lane| NoteKind::Hold { lane, duration })
        }
        (Kind::Other(key), Shape::Instant) if key.is_owned_by(ID) => match key.name().as_str() {
            SHAKE => placed(&[
                NoteKind::Shake(Shake::Left),
                NoteKind::Shake(Shake::Right),
                NoteKind::Shake(Shake::Down),
            ]),
            SLICE => placed(&[NoteKind::Slice(Side::Left), NoteKind::Slice(Side::Right)]),
            MEGA_TAP => placed(&[NoteKind::MegaTap]),
            _ => None,
        },
        _ => None,
    }
}

/// Reads supported meter changes in `stream` and rejects unsupported timing.
fn read_tempo(timing: &Timing, stream: Stream) -> Result<Tempo, Error> {
    let mut changes = Vec::with_capacity(timing.events.len());
    for (index, event) in timing.events.iter().enumerate() {
        let at = Location::Event { stream, index };
        reject_own_keys(&event.ext, at)?;
        let unsupported = |what| Error::Unsupported { at, what };
        let kind = match &event.kind {
            TimingKind::TimeSignature(meter) => TempoKind::TimeSignature(*meter),
            TimingKind::Stop(_) => return Err(unsupported("stops")),
            TimingKind::Delay(_) => return Err(unsupported("delays")),
            TimingKind::Warp(_) => return Err(unsupported("warps")),
            TimingKind::Other { name, .. } if name.is_owned_by(ID) => {
                return Err(Error::UnknownKey {
                    at,
                    key: name.clone(),
                });
            }
            TimingKind::Other { .. } => continue,
        };
        changes.push(TempoChange {
            tick: event.tick,
            kind,
        });
    }
    Ok(Tempo {
        anchors: timing.anchors.clone(),
        tempo: timing.tempo,
        changes,
    })
}

/// Settings holding `theme` as the theme, if there is one.
fn themed(theme: Option<&Name>) -> Settings {
    theme
        .map(|theme| (THEME.to_owned(), Value::from(theme.as_str())))
        .into_iter()
        .collect()
}

/// The theme a setting names, if it is there, refused unless it is a name.
fn named(value: Option<&Value>, at: Location) -> Result<Option<Name>, Error> {
    match value {
        None => Ok(None),
        Some(Value::String(theme)) => {
            Name::new(theme.clone())
                .map(Some)
                .map_err(|_invalid| Error::Theme {
                    at,
                    found: Value::String(theme.clone()).to_string(),
                })
        }
        Some(other) => Err(Error::Theme {
            at,
            found: other.to_string(),
        }),
    }
}

/// Fails at `at` for the first unrecognised chart setting.
fn reject_keys<'a>(mut keys: impl Iterator<Item = &'a String>, at: Location) -> Result<(), Error> {
    match keys.next() {
        Some(key) => Err(Error::UnknownSetting {
            at,
            key: key.clone(),
        }),
        None => Ok(()),
    }
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

/// `object` remade as the note `pick` names, keeping whatever else it carries. None when the
/// object is not a note of this game, or not on a lane.
fn changed_to(
    object: &Object,
    lanes: LaneCount,
    pick: fn(u16, u16, LaneIndex) -> NoteKind,
) -> Option<Object> {
    let lanes = Lanes::of(lanes)?;
    read_note(object, lanes)?;
    let Position::Lane { index, .. } = object.position else {
        return None;
    };
    let (lane, width) = (u16::from(index), u16::from(lanes.width()));
    let note = Note {
        tick: object.tick,
        kind: pick(lane, width, index),
    };
    // What else the note carries, such as another plugin's data, stays with it.
    let changed = note.to_object(lanes).ok()?;
    Some(Object {
        layer: object.layer.clone(),
        sample: object.sample.clone(),
        ext: object.ext.clone(),
        ..changed
    })
}

/// A shake: left on the first lane, right on the last, down between.
fn to_arrow(object: &Object, lanes: LaneCount) -> Option<Object> {
    changed_to(object, lanes, |lane, width, _| {
        if lane == 0 {
            NoteKind::Shake(Shake::Left)
        } else if lane.saturating_add(1) == width {
            NoteKind::Shake(Shake::Right)
        } else {
            NoteKind::Shake(Shake::Down)
        }
    })
}

/// A slice, taking the side the note's lane falls on.
fn to_slice(object: &Object, lanes: LaneCount) -> Option<Object> {
    changed_to(object, lanes, |lane, width, _| {
        let left = lane.saturating_mul(2) < width;
        NoteKind::Slice(if left { Side::Left } else { Side::Right })
    })
}

/// A mega tap across every lane.
fn to_mega_tap(object: &Object, lanes: LaneCount) -> Option<Object> {
    changed_to(object, lanes, |_, _, _| NoteKind::MegaTap)
}

/// A tap on the note's first lane.
fn to_tap(object: &Object, lanes: LaneCount) -> Option<Object> {
    changed_to(object, lanes, |_, _, index| NoteKind::Tap { lane: index })
}

#[cfg(test)]
mod tests {
    use super::*;
    use charter_core::ValidDocument;
    use serde_json::{Value, json};

    /// A one-chart, three-lane ttr document holding `objects`.
    fn document(objects: Value) -> Value {
        let mut value = json!({
            "format_version": 1,
            "ruleset": { "id": "ttr", "version": "0" },
            "metadata": { "title": "t", "artist": "a", "audio": "song.ogg" },
            "timing": { "anchors": [ { "tick": 0, "seconds": -0.25 } ], "tempo": 150.0, "events": [] },
            "charts": [ { "name": "Easy", "lanes": 3 } ]
        });
        value["charts"][0]["objects"] = objects;
        value
    }

    fn lane_object(tick: i32, index: u16, width: u16, kind: &str) -> Value {
        json!({ "tick": tick, "position": { "lane": { "index": index, "width": width } },
                "kind": kind, "shape": "instant" })
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
    fn reads_taps_in_a_three_lane_song() {
        let song = read(&document(json!([lane_object(0, 1, 1, "tap")]))).unwrap();
        assert_eq!(song.charts()[0].lanes, Lanes::Three);
        assert_eq!(song.tempo().tempo, Bpm::try_from(150.0).unwrap());
        assert_eq!(
            song.charts(),
            [Chart {
                lanes: Lanes::Three,
                name: "Easy".into(),
                difficulty: None,
                theme: None,
                tempo: None,
                notes: vec![Note {
                    tick: Tick::new(0),
                    kind: NoteKind::Tap {
                        lane: LaneIndex::new(1)
                    }
                }],
            }]
        );
    }

    fn written(song: &Song) -> Value {
        serde_json::from_slice(&song.to_document().to_json().unwrap()).unwrap()
    }

    /// What `song` writes: `document` stamped with this plugin's version.
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
        let song = valid(&document(json!([lane_object(0, 1, 1, "tap")])));
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
    fn writes_taps_back_as_single_lane_objects() {
        let song = read(&document(json!([lane_object(0, 1, 1, "tap")]))).unwrap();
        assert_eq!(
            written(&song),
            expected(json!([{ "tick": 0, "position": { "lane": { "index": 1 } },
                                                      "kind": "tap", "shape": "instant" }]))
        );
    }

    fn with_lanes(lanes: u16) -> Value {
        let mut value = document(json!([lane_object(0, 0, 1, "tap")]));
        value["charts"][0]["lanes"] = json!(lanes);
        value
    }

    #[test]
    fn a_song_has_three_four_or_five_lanes() {
        for (count, lanes) in [(3, Lanes::Three), (4, Lanes::Four), (5, Lanes::Five)] {
            let song = read(&with_lanes(count)).unwrap();
            assert_eq!(song.charts()[0].lanes, lanes);
            assert_eq!(written(&song)["charts"][0]["lanes"], json!(count));
        }
    }

    #[test]
    fn each_chart_has_three_four_or_five_lanes_of_its_own() {
        let mut mixed = with_lanes(4);
        mixed["charts"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "name": "Hard", "lanes": 5 }));
        let song = read(&mixed).unwrap();
        let lanes: Vec<_> = song.charts().iter().map(|chart| chart.lanes).collect();
        assert_eq!(lanes, [Lanes::Four, Lanes::Five]);
        for value in [with_lanes(2), with_lanes(6)] {
            assert!(matches!(read(&value), Err(Error::Lanes { .. })), "{value}");
        }
    }

    #[test]
    fn a_song_needs_a_chart_to_know_its_lanes() {
        let mut value = document(json!([]));
        value.as_object_mut().unwrap().remove("charts");
        assert!(matches!(read(&value), Err(Error::NoCharts)));
    }

    #[test]
    fn a_song_of_another_ruleset_is_refused() {
        let mut value = document(json!([]));
        value["ruleset"]["id"] = json!("sm");
        assert!(matches!(read(&value), Err(Error::WrongRuleset(id)) if id.as_str() == "sm"));
    }

    #[test]
    fn holds_are_sustained_notes_in_one_lane() {
        let hold = json!({ "tick": 0, "position": { "lane": { "index": 2 } }, "kind": "hold",
                           "shape": { "sustain": { "duration": 480 } } });
        let song = read(&document(json!([hold]))).unwrap();
        let duration = Ticks::new(480).unwrap();
        assert_eq!(
            song.charts()[0].notes,
            [Note {
                tick: Tick::new(0),
                kind: NoteKind::Hold {
                    lane: LaneIndex::new(2),
                    duration
                }
            }]
        );
        assert_eq!(written(&song), expected(json!([hold])));
    }

    #[test]
    fn a_shake_sits_in_the_lane_of_its_direction() {
        use Shake::{Down, Left, Right};
        // (lanes, left lane, right lane, down lane and width), derived by hand from the rules.
        for (lanes, right, (down, down_width)) in [(3, 2, (1, 1)), (4, 3, (1, 2)), (5, 4, (2, 1))] {
            let shakes = json!([
                lane_object(0, 0, 1, "ttr.shake"),
                lane_object(960, right, 1, "ttr.shake"),
                lane_object(1920, down, down_width, "ttr.shake"),
            ]);
            let mut value = document(shakes.clone());
            value["charts"][0]["lanes"] = json!(lanes);
            let song = read(&value).unwrap();
            let kinds: Vec<NoteKind> = song.charts()[0]
                .notes
                .iter()
                .map(|note| note.kind)
                .collect();
            assert_eq!(
                kinds,
                [
                    NoteKind::Shake(Left),
                    NoteKind::Shake(Right),
                    NoteKind::Shake(Down)
                ]
            );
            let mut out = expected(written_objects(&shakes));
            out["charts"][0]["lanes"] = json!(lanes);
            assert_eq!(written(&song), out);
        }
    }

    /// `objects` as a writer emits them: single-lane widths omitted.
    fn written_objects(objects: &Value) -> Value {
        let mut objects = objects.clone();
        for object in objects.as_array_mut().unwrap() {
            if object["position"]["lane"]["width"] == json!(1) {
                object["position"]["lane"]
                    .as_object_mut()
                    .unwrap()
                    .remove("width");
            }
        }
        objects
    }

    #[test]
    fn a_slice_sits_in_the_left_or_right_lane() {
        use Side::{Left, Right};
        let slices = json!([
            lane_object(0, 0, 1, "ttr.slice"),
            lane_object(960, 3, 1, "ttr.slice")
        ]);
        let mut value = document(slices.clone());
        value["charts"][0]["lanes"] = json!(4);
        let song = read(&value).unwrap();
        let kinds: Vec<NoteKind> = song.charts()[0]
            .notes
            .iter()
            .map(|note| note.kind)
            .collect();
        assert_eq!(kinds, [NoteKind::Slice(Left), NoteKind::Slice(Right)]);
        let mut out = expected(written_objects(&slices));
        out["charts"][0]["lanes"] = json!(4);
        assert_eq!(written(&song), out);
    }

    #[test]
    fn a_mega_tap_covers_every_lane_of_its_tick_beside_taps_but_not_shakes() {
        let mega = json!([lane_object(0, 0, 5, "ttr.mega_tap")]);
        let mut value = document(mega.clone());
        value["charts"][0]["lanes"] = json!(5);
        let song = read(&value).unwrap();
        assert_eq!(
            song.charts()[0].notes,
            [Note {
                tick: Tick::new(0),
                kind: NoteKind::MegaTap
            }]
        );
        let mut out = expected(mega);
        out["charts"][0]["lanes"] = json!(5);
        assert_eq!(written(&song), out);

        let mut charts = song.charts().to_vec();
        charts[0].notes.push(Note {
            tick: Tick::new(0),
            kind: NoteKind::Tap {
                lane: LaneIndex::new(4),
            },
        });
        let with = |levels: Vec<Chart>| {
            Song::new(
                song.metadata().clone(),
                song.ticks_per_beat(),
                song.tempo().clone(),
                song.markers().to_vec(),
                levels,
            )
        };
        // A tap shares the instant, as the game plays both.
        with(charts.clone()).unwrap();
        let mut shaken = charts;
        shaken[0].notes.push(Note {
            tick: Tick::new(0),
            kind: NoteKind::Shake(Shake::Left),
        });
        let crowded = with(shaken).unwrap_err();
        assert_eq!(
            crowded.to_string(),
            "charts[0].objects[2]: two shakes, slices or mega taps on one lane at once"
        );
    }

    #[test]
    fn objects_the_game_cannot_play_are_refused() {
        let mut held_shake = lane_object(0, 0, 1, "ttr.shake");
        held_shake["shape"] = json!({ "sustain": { "duration": 240 } });
        let mut plane_tap = lane_object(0, 0, 1, "tap");
        plane_tap["position"] = json!({ "plane": { "x": 0.0, "y": 0.0 } });
        let mut path_tap = lane_object(0, 0, 1, "tap");
        path_tap["shape"] = json!({ "path": { "points": [
            { "tick": 240, "position": { "lane": { "index": 1 } }, "ease": "linear" } ] } });
        for object in [
            lane_object(0, 1, 1, "ttr.slice"),
            held_shake,
            lane_object(0, 0, 2, "tap"),
            plane_tap,
            lane_object(0, 0, 1, "ttr.spin"),
            path_tap,
        ] {
            let value = document(json!([object]));
            assert!(
                matches!(read(&value), Err(Error::UnsupportedObject { .. })),
                "{object} was accepted"
            );
        }
    }

    #[test]
    fn keysounds_and_children_have_no_place_in_the_game() {
        let mut keysounded = document(json!([lane_object(0, 0, 1, "tap")]));
        keysounded["samples"] = json!({ "kick": "kick.wav" });
        keysounded["charts"][0]["objects"][0]["sample"] = json!("kick");
        let mut bank_only = document(json!([]));
        bank_only["samples"] = json!({ "kick": "kick.wav" });
        let hold = json!({ "tick": 0, "position": { "lane": { "index": 0 } }, "kind": "hold",
                           "shape": { "sustain": { "duration": 480 } },
                           "children": [lane_object(240, 1, 1, "tap")] });
        for value in [keysounded, bank_only, document(json!([hold]))] {
            assert!(
                matches!(read(&value), Err(Error::Unsupported { .. })),
                "{value}"
            );
        }
    }

    fn event(tick: i32, kind: Value) -> Value {
        let mut event = json!({ "tick": tick });
        event["kind"] = kind;
        event
    }

    #[test]
    fn tempo_changes_and_meters_are_kept_but_gimmicks_refused() {
        let mut value = document(json!([]));
        let events = value["timing"]["events"].as_array_mut().unwrap();
        events.push(event(0, json!({ "time_signature": [3, 4] })));
        value["timing"]["anchors"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "tick": 3840, "seconds": 3.0 }));
        value["timing"]["tempo"] = json!(75.0);
        let song = read(&value).unwrap();
        let meter = |n| std::num::NonZeroU16::new(n).unwrap();
        assert_eq!(
            song.tempo().changes,
            [TempoChange {
                tick: Tick::new(0),
                kind: TempoKind::TimeSignature(TimeSignature {
                    numerator: meter(3),
                    denominator: meter(4)
                })
            },]
        );
        assert_eq!(song.tempo().tempo, Bpm::try_from(75.0).unwrap());

        for gimmick in [
            json!({ "stop": 1.0 }),
            json!({ "delay": 1.0 }),
            json!({ "warp": 960 }),
        ] {
            let mut gimmick_value = document(json!([]));
            gimmick_value["timing"]["events"]
                .as_array_mut()
                .unwrap()
                .push(event(960, gimmick));
            assert!(
                matches!(read(&gimmick_value), Err(Error::Unsupported { .. })),
                "{gimmick_value}"
            );
        }
    }

    #[test]
    fn a_chart_can_carry_its_own_tempo() {
        let own = json!({
            "anchors": [ { "tick": 0, "seconds": 0.5 } ],
            "tempo": 200.0,
            "events": []
        });
        let mut value = document(json!([]));
        value["charts"][0]["timing"] = own;
        let song = read(&value).unwrap();
        let tempo = song.charts()[0].tempo.as_ref().unwrap();
        assert_eq!(tempo.anchors[0].seconds, Origin::try_from(0.5).unwrap());
        assert_eq!(tempo.tempo, Bpm::try_from(200.0).unwrap());
    }

    #[test]
    fn keys_in_the_ttr_namespace_or_settings_that_it_does_not_define_are_refused() {
        type Edit = fn(&mut Value);
        let edits: [Edit; 6] = [
            |v| v["charts"][0]["objects"][0]["ext"] = json!({ "ttr.color": 1 }),
            |v| v["metadata"]["ext"] = json!({ "ttr.banner": "b.png" }),
            |v| v["charts"][0]["ext"] = json!({ "ttr.rating": 3 }),
            |v| {
                let other = event(0, json!({ "other": { "name": "ttr.scroll", "value": 2 } }));
                v["timing"]["events"].as_array_mut().unwrap().push(other);
            },
            |v| v["settings"] = json!({ "rating": 3 }),
            |v| v["charts"][0]["settings"] = json!({ "rating": 3 }),
        ];
        for edit in edits {
            let mut value = document(json!([lane_object(0, 0, 1, "tap")]));
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
        let plain = document(json!([lane_object(0, 0, 1, "tap")]));
        let mut decorated = plain.clone();
        decorated["plugins"] = json!({ "swing": { "version": "1" } });
        decorated["metadata"]["ext"] = json!({ "swing.feel": "late" });
        decorated["charts"][0]["objects"][0]["ext"] = json!({ "swing.accent": true });
        let groove = event(
            0,
            json!({ "other": { "name": "swing.groove", "value": 0.6 } }),
        );
        decorated["timing"]["events"]
            .as_array_mut()
            .unwrap()
            .push(groove);
        let song = read(&decorated).unwrap();
        assert_eq!(song, read(&plain).unwrap());
        assert_eq!(
            written(&song),
            expected(written_objects(&plain["charts"][0]["objects"]))
        );
    }

    #[test]
    fn errors_say_where_and_which_key() {
        let mut value = document(json!([lane_object(0, 0, 1, "tap")]));
        value["charts"][0]["objects"][0]["ext"] = json!({ "ttr.speed": 2 });
        let speed: Key = "ttr.speed".parse().unwrap();
        assert!(matches!(
            read(&value),
            Err(Error::UnknownKey { at: Location::Object { chart: 0, index: 0 }, key }) if key == speed
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
        let song = read(&document(json!([lane_object(0, 1, 1, "tap")]))).unwrap();
        let build = |metadata: Metadata, charts: Vec<Chart>| {
            Song::new(
                metadata,
                song.ticks_per_beat(),
                song.tempo().clone(),
                song.markers().to_vec(),
                charts,
            )
        };
        assert_eq!(
            build(song.metadata().clone(), song.charts().to_vec()).unwrap(),
            song
        );
        let outside = Chart {
            lanes: Lanes::Three,
            name: "Hard".into(),
            difficulty: None,
            theme: None,
            tempo: None,
            notes: vec![Note {
                tick: Tick::new(0),
                kind: NoteKind::Tap {
                    lane: LaneIndex::new(3),
                },
            }],
        };
        let mut tagged = song.metadata().clone();
        tagged
            .ext
            .insert("ttr.genre".parse().unwrap(), json!("rock"));
        assert!(matches!(
            build(song.metadata().clone(), vec![outside]),
            Err(Error::Document(_))
        ));
        assert!(matches!(
            build(song.metadata().clone(), Vec::new()),
            Err(Error::NoCharts)
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
        own["markers"] = json!([{ "tick": 0, "kind": "ttr.drop", "text": "" }]);
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

    fn levelled(charts: &[(&str, usize)]) -> charter_core::NewSong {
        charter_core::NewSong {
            charts: charts
                .iter()
                .map(|&(name, level)| charter_core::NewChart {
                    name: name.to_owned(),
                    level: Some(level),
                })
                .collect(),
            ..new_song(0, &[])
        }
    }

    fn preset(name: &str) -> Vec<(usize, String)> {
        let difficulties = Plugin.difficulties().unwrap();
        difficulties
            .presets
            .into_iter()
            .find(|preset| preset.name == name)
            .unwrap()
            .charts
    }

    fn named(charts: &[(usize, &str)]) -> Vec<(usize, String)> {
        charts
            .iter()
            .map(|&(level, name)| (level, name.to_owned()))
            .collect()
    }

    #[test]
    fn charts_come_in_four_standard_levels_that_each_game_names_its_own_way() {
        let difficulties = Plugin.difficulties().unwrap();
        assert_eq!(difficulties.levels, ["Easy", "Medium", "Hard", "Extreme"]);
        assert_eq!(difficulties.presets[0].name, "Custom, Extreme only");
        assert_eq!(preset("Custom, Extreme only"), named(&[(3, "Extreme")]));
        assert_eq!(
            preset("Custom, all levels"),
            named(&[(0, "Easy"), (1, "Medium"), (2, "Hard"), (3, "Extreme")])
        );
        assert_eq!(
            preset("Tap Tap Reloaded"),
            named(&[(1, "Metallica"), (2, "Downloads"), (3, "Customs")])
        );
        assert_eq!(
            preset("Tap Tap Hardcore"),
            named(&[
                (0, "Extreme"),
                (1, "Intense"),
                (2, "Severe"),
                (3, "Customs")
            ])
        );
    }

    #[test]
    fn a_new_chart_keeps_its_standard_level_under_the_name_its_game_gives_it() {
        let document = Plugin
            .new_song(&levelled(&[("Extreme", 0), ("Customs", 3)]))
            .unwrap();
        let value: Value = serde_json::from_slice(&document.to_json().unwrap()).unwrap();
        assert_eq!(value["charts"][0]["name"], json!("Extreme"));
        assert_eq!(
            value["charts"][0]["settings"],
            json!({ "difficulty": "easy", "theme": "TTRDJ" })
        );
        assert_eq!(
            value["charts"][1]["settings"],
            json!({ "difficulty": "extreme", "theme": "TTRDJ" })
        );
        let song = Song::try_from(&document).unwrap();
        let levels: Vec<_> = song.charts().iter().map(|chart| chart.difficulty).collect();
        assert_eq!(levels, [Some(Difficulty::Easy), Some(Difficulty::Extreme)]);
    }

    #[test]
    fn a_chart_of_no_standard_level_has_no_difficulty_setting() {
        let document = Plugin.new_song(&new_song(0, &["Mine"])).unwrap();
        let value: Value = serde_json::from_slice(&document.to_json().unwrap()).unwrap();
        assert_eq!(value["charts"][0]["settings"], json!({ "theme": "TTRDJ" }));
    }

    #[test]
    fn a_new_song_and_each_of_its_levels_play_in_the_ttrdj_theme() {
        let document = Plugin
            .new_song(&levelled(&[("Easy", 0), ("Extreme", 3)]))
            .unwrap();
        let song = Song::try_from(&document).unwrap();
        assert_eq!(song.theme(), Some(DEFAULT_THEME));
        assert!(
            song.charts()
                .iter()
                .all(|chart| chart.theme.as_ref().map(Name::as_str) == Some(DEFAULT_THEME))
        );
    }

    #[test]
    fn a_song_and_each_of_its_charts_can_name_a_theme() {
        let mut value = document(json!([]));
        value["settings"] = json!({ "theme": "DeltaHeavyTheme" });
        value["charts"][0]["settings"] = json!({ "difficulty": "easy", "theme": "Neon" });
        let song = read(&value).unwrap();
        assert_eq!(song.theme(), Some("DeltaHeavyTheme"));
        assert_eq!(
            song.charts()[0].theme.as_ref().map(Name::as_str),
            Some("Neon")
        );
        value["ruleset"]["version"] = json!(VERSION);
        value["charts"][0]
            .as_object_mut()
            .unwrap()
            .remove("objects");
        assert_eq!(written(&song), value);
    }

    #[test]
    fn a_theme_that_is_not_a_name_is_refused() {
        for bad in [json!(3), json!(""), json!(null)] {
            let mut value = document(json!([]));
            value["settings"] = json!({ "theme": bad });
            assert!(matches!(read(&value), Err(Error::Theme { .. })), "{bad}");
            let mut chart_value = document(json!([]));
            chart_value["charts"][0]["settings"] = json!({ "theme": bad });
            assert!(
                matches!(read(&chart_value), Err(Error::Theme { .. })),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_level_the_game_does_not_have_is_refused() {
        Plugin.new_song(&levelled(&[("Beyond", 4)])).unwrap_err();
        let mut value = document(json!([]));
        value["charts"][0]["settings"] = json!({ "difficulty": "insane" });
        assert!(matches!(read(&value), Err(Error::Difficulty { .. })));
        value["charts"][0]["settings"] = json!({ "difficulty": 3 });
        assert!(matches!(read(&value), Err(Error::Difficulty { .. })));
    }

    #[test]
    fn a_note_changes_to_the_arrow_or_slice_its_lane_gives_or_a_big_tap_and_back() {
        let lanes = Lanes::Five;
        let tap = |lane: u16| {
            let note = Note {
                tick: Tick::new(0),
                kind: NoteKind::Tap {
                    lane: LaneIndex::new(lane),
                },
            };
            note.to_object(lanes).unwrap()
        };
        let changed = |to: fn(&Object, LaneCount) -> Option<Object>, lane: u16| {
            let object = to(&tap(lane), lanes.width()).unwrap();
            read_note(&object, lanes).unwrap()
        };
        let arrows: Vec<_> = (0..5).map(|lane| changed(to_arrow, lane)).collect();
        let (l, r, d) = (Shake::Left, Shake::Right, Shake::Down);
        assert_eq!(arrows, [l, d, d, d, r].map(NoteKind::Shake));
        let slices: Vec<_> = (0..5).map(|lane| changed(to_slice, lane)).collect();
        let (sl, sr) = (Side::Left, Side::Right);
        assert_eq!(slices, [sl, sl, sl, sr, sr].map(NoteKind::Slice));
        assert_eq!(changed(to_mega_tap, 3), NoteKind::MegaTap);
        let arrow = to_arrow(&tap(4), lanes.width()).unwrap();
        let back = to_tap(&arrow, lanes.width()).unwrap();
        assert_eq!(
            read_note(&back, lanes),
            Some(NoteKind::Tap {
                lane: LaneIndex::new(4)
            })
        );
        assert_eq!(Plugin.note_changes().len(), 4);
        let mut carrying = tap(1);
        carrying.layer = Some(charter_core::LayerName::new("Kick").unwrap());
        carrying
            .ext
            .insert("swing.accent".parse().unwrap(), serde_json::json!(true));
        let carrying_change = to_arrow(&carrying, lanes.width()).unwrap();
        assert_eq!(carrying_change.ext, carrying.ext);
        assert_eq!(carrying_change.layer, carrying.layer);
    }

    #[test]
    fn a_new_song_offers_three_four_or_five_lanes() {
        let lanes: Vec<_> = Plugin.layouts().iter().map(|layout| layout.lanes).collect();
        assert_eq!(lanes, [3, 4, 5].map(|n| LaneCount::new(n).unwrap()));
        assert_eq!(Plugin.layouts()[1].name, "4 lanes");
        assert_eq!(Plugin.name(), "Tap Tap Revenge");
    }

    #[test]
    fn a_new_song_has_the_lanes_and_charts_asked_for_and_no_notes() {
        let document = Plugin.new_song(&new_song(2, &["Easy", "Extreme"])).unwrap();
        Plugin.check(&document).unwrap();
        let song = Song::try_from(&document).unwrap();
        assert_eq!(song.charts()[0].lanes, Lanes::Five);
        let names: Vec<_> = song
            .charts()
            .iter()
            .map(|chart| chart.name.as_str())
            .collect();
        assert_eq!(names, ["Easy", "Extreme"]);
        assert!(song.charts().iter().all(|chart| chart.notes.is_empty()));
    }

    #[test]
    fn a_new_song_refuses_a_lane_count_it_does_not_have_or_no_charts() {
        Plugin.new_song(&new_song(3, &["Easy"])).unwrap_err();
        Plugin.new_song(&new_song(0, &[])).unwrap_err();
    }

    /// A three-lane song with an Easy and an Extreme chart, a note of each kind the game's files
    /// hold, as a document.
    fn exchangeable() -> ValidDocument {
        let hold = json!({ "tick": 1920, "position": { "lane": { "index": 1 } },
                           "kind": "hold", "shape": { "sustain": { "duration": 1920 } } });
        let shake = |tick: i32, index: u16| lane_object(tick, index, 1, "ttr.shake");
        let mut value = document(json!([lane_object(960, 0, 1, "tap")]));
        value["timing"] =
            json!({ "anchors": [ { "tick": 0, "seconds": 0.5 } ], "tempo": 128.0, "events": [] });
        value["charts"][0]["settings"] = json!({ "difficulty": "easy" });
        value["charts"].as_array_mut().unwrap().push(json!({ "name": "Customs", "lanes": 3,
            "settings": { "difficulty": "extreme" },
            "objects": [lane_object(0, 2, 1, "tap"), hold, shake(4800, 0), shake(5760, 2), shake(6720, 1)] }));
        ValidDocument::from_json(&serde_json::to_vec(&value).unwrap()).unwrap()
    }

    /// Each chart's notes as their ticks, kinds and positions.
    fn placed(document: &ValidDocument) -> Vec<Vec<(i64, Kind, Position)>> {
        document
            .charts()
            .map(|chart| {
                chart
                    .objects()
                    .map(|object| (i64::from(object.tick), object.kind.clone(), object.position))
                    .collect()
            })
            .collect()
    }

    /// Each chart's notes as their moments to the microsecond, kinds and positions.
    fn heard(document: &ValidDocument) -> Vec<Vec<(i64, Kind, Position)>> {
        document
            .charts()
            .map(|chart| {
                let timing = chart.timing();
                chart
                    .objects()
                    .map(|object| {
                        let seconds = timing.time_at(object.tick).seconds();
                        #[expect(
                            clippy::as_conversions,
                            clippy::cast_possible_truncation,
                            reason = "a test's moments are a few seconds"
                        )]
                        let micros = (seconds * 1e6).round() as i64;
                        (micros, object.kind.clone(), object.position)
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn the_plugin_lists_each_game_s_track_and_midi_layout() {
        let formats = Plugin.song_formats();
        let names: Vec<&str> = formats.iter().map(|format| format.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Tap Tap Revenge track",
                "Tap Tap Revenge: Tour track",
                "Tap Tap Revenge track, custom tap theme",
                "Editor on Fire MIDI",
                "Tap Tap Reloaded MIDI",
            ]
        );
        for track in &formats[..3] {
            assert_eq!(track.extensions, ["ttr2_track"]);
            assert_eq!(track.companions, ["info.plist"]);
        }
        for midi in &formats[3..] {
            assert_eq!(midi.extensions, ["mid", "midi"]);
            assert!(midi.companions.is_empty());
        }
        let unknown = Plugin.import_song(5, b"", &BTreeMap::new()).unwrap_err();
        assert_eq!(
            unknown.to_string(),
            "Tap Tap Revenge has no such song format"
        );
        Plugin.export_song(5, &exchangeable(), None).unwrap_err();
    }

    /// The plain Tap Tap Revenge track among the formats, and Editor on Fire's MIDI.
    const TRACK: usize = 0;
    const MIDI: usize = 3;

    #[test]
    fn a_tour_track_reads_side_and_middle_keys_as_slices_and_a_mega_tap() {
        let song = exchangeable();
        let track = Plugin.export_song(TRACK, &song, None).unwrap().bytes;
        let tour = Plugin.import_song(1, &track, &BTreeMap::new()).unwrap();
        let kinds = |document: &ValidDocument| -> Vec<String> {
            let chart = document.charts().nth(1).unwrap();
            chart
                .objects()
                .map(|object| format!("{:?}", object.kind))
                .collect()
        };
        let named = |name: &str| {
            format!("Other(Key {{ namespace: Namespace(\"ttr\"), name: Name({name:?}) }})")
        };
        assert_eq!(
            kinds(&tour.document),
            [
                "Tap".to_owned(),
                "Hold".to_owned(),
                named("slice"),
                named("slice"),
                named("mega_tap")
            ]
        );
        let tour_track = Plugin.export_song(1, &tour.document, Some(60.0)).unwrap();
        assert!(tour_track.left_out.is_empty(), "{:?}", tour_track.left_out);
        assert_eq!(tour_track.bytes, track, "Tour's notes sit on the same keys");
    }

    #[test]
    fn a_four_lane_level_comes_back_with_its_columns_from_the_exported_info_plist() {
        let mut value = document(json!([lane_object(0, 3, 1, "tap")]));
        value["charts"][0]["lanes"] = json!(4);
        value["charts"][0]["settings"] = json!({ "difficulty": "hard" });
        value["metadata"]["audio"] = json!("song.mp3");
        let song = ValidDocument::from_json(&serde_json::to_vec(&value).unwrap()).unwrap();
        let exported = Plugin.export_song(TRACK, &song, None).unwrap();
        assert_eq!(
            exported.left_out,
            ["how long the audio lasts, which info.plist holds, as the audio has not loaded"]
        );
        assert_eq!(exported.file_name.as_deref(), Some("taptrack.ttr2_track"));
        assert_eq!(
            exported.copies,
            [("audio.m4a".to_owned(), SongResource::AacAudio)]
        );
        let companions = exported.beside.clone();
        let info =
            plist::Value::from_reader(std::io::Cursor::new(&companions["info.plist"])).unwrap();
        let info = info.as_dictionary().unwrap();
        assert_eq!(
            info["availableDifficultyLevels"],
            plist::Value::Array(vec![3_u64.into()])
        );
        assert_eq!(info["kTTRDownloadableItemTitleKey"], "t".into());
        assert_eq!(info["kTTRDownloadableItemSubtitleKey"], "a".into());
        assert_eq!(info["themeName"], DEFAULT_THEME.into());
        // What info.plist holds comes back with the song.
        let mut themed = value.clone();
        themed["settings"] = json!({ "theme": "Muse" });
        themed["charts"][0]["settings"]["theme"] = json!("Slipknot");
        themed["metadata"]["genre"] = json!("Rock");
        let themed = ValidDocument::from_json(&serde_json::to_vec(&themed).unwrap()).unwrap();
        let bundle = Plugin.export_song(TRACK, &themed, Some(1.0)).unwrap();
        let back = Plugin
            .import_song(TRACK, &bundle.bytes, &bundle.beside)
            .unwrap();
        let back = Song::try_from(&back.document).unwrap();
        assert_eq!(back.theme(), Some("Muse"));
        assert_eq!(
            back.charts()[0].theme.as_ref().map(Name::as_str),
            Some("Slipknot")
        );
        assert_eq!(back.metadata().genre.as_deref(), Some("Rock"));
        assert_eq!(
            (
                back.metadata().title.as_str(),
                back.metadata().artist.as_str()
            ),
            ("t", "a")
        );
        let imported = Plugin
            .import_song(TRACK, &exported.bytes, &companions)
            .unwrap();
        assert!(imported.left_out.is_empty(), "{:?}", imported.left_out);
        assert_eq!(placed(&imported.document), placed(&song));
        // Without info.plist the level has three columns, where key 3 is a right shake.
        let three = Plugin
            .import_song(TRACK, &exported.bytes, &BTreeMap::new())
            .unwrap();
        let chart = three.document.charts().next().unwrap();
        assert_eq!(chart.lanes, LaneCount::new(3));
        let kinds: Vec<String> = chart
            .objects()
            .map(|object| format!("{:?}", object.kind))
            .collect();
        assert!(kinds[0].contains("shake"), "{kinds:?}");
    }

    #[test]
    fn tap_tap_reloaded_midi_plays_the_middle_note_as_a_mega_tap_and_has_no_side_keys() {
        let song = exchangeable();
        let reloaded = Plugin.export_song(4, &song, None).unwrap();
        assert_eq!(
            reloaded.left_out,
            [
                "a down shake, which Tap Tap Revenge: Tour plays as a mega tap",
                "a left shake, which Tap Tap Revenge: Tour plays as a left slice",
                "a right shake, which Tap Tap Revenge: Tour plays as a right slice",
                "left and right shakes and slices, which Tap Tap Reloaded's layout has no key for (2 times)",
                "the artist, which a MIDI file has no place for",
            ]
        );
        let imported = Plugin
            .import_song(4, &reloaded.bytes, &BTreeMap::new())
            .unwrap();
        let extreme = imported.document.charts().nth(1).unwrap();
        let kinds: Vec<String> = extreme
            .objects()
            .map(|object| format!("{:?}", object.kind))
            .collect();
        assert_eq!(kinds.len(), 3);
        assert!(kinds[2].contains("mega_tap"), "{kinds:?}");
    }

    #[test]
    fn a_song_comes_back_from_either_format_on_the_same_ticks() {
        let song = exchangeable();
        for format in [TRACK, MIDI] {
            let exported = Plugin.export_song(format, &song, None).unwrap();
            let imported = Plugin
                .import_song(format, &exported.bytes, &BTreeMap::new())
                .unwrap();
            assert!(imported.left_out.is_empty(), "{:?}", imported.left_out);
            assert_eq!(heard(&imported.document), heard(&song), "format {format}");
            let back = Song::try_from(&imported.document).unwrap();
            let names: Vec<&str> = back
                .charts()
                .iter()
                .map(|chart| chart.name.as_str())
                .collect();
            assert_eq!(names, ["Easy", "Extreme"]);
            assert_eq!(back.metadata().title, "t");
            Plugin.check(&imported.document).unwrap();
        }
        let midi = Plugin.export_song(MIDI, &song, None).unwrap();
        assert_eq!(
            midi.left_out,
            ["the artist, which a MIDI file has no place for"]
        );
        let track = Plugin.export_song(TRACK, &song, None).unwrap();
        assert_eq!(
            track.left_out,
            ["how long the audio lasts, which info.plist holds, as the audio has not loaded"]
        );
        let imported = Plugin
            .import_song(TRACK, &track.bytes, &BTreeMap::new())
            .unwrap();
        // The game's file keeps the ticks too; MIDI's lead-in starts the grid at the audio's
        // start, a beat earlier here.
        assert_eq!(placed(&imported.document), placed(&song));
        assert_eq!(imported.document.document().metadata.artist, "a");
        assert_eq!(
            imported.document.document().metadata.audio.as_str(),
            "audio.m4a"
        );
    }
}

/// Proofs for Kani, run with `cargo kani -p charter-ttr`.
#[cfg(kani)]
mod proofs {
    use super::*;

    fn lanes() -> Lanes {
        match kani::any::<u8>() % 3 {
            0 => Lanes::Three,
            1 => Lanes::Four,
            _ => Lanes::Five,
        }
    }

    fn fits(lanes: Lanes, kind: NoteKind) -> bool {
        lanes
            .position(kind)
            .lanes()
            .is_some_and(|span| span.fits(lanes.width()))
    }

    #[kani::proof]
    fn a_tap_or_hold_fits_exactly_when_its_lane_exists() {
        let (lanes, lane) = (lanes(), LaneIndex::new(kani::any()));
        let duration = Ticks::new(kani::any());
        kani::assume(duration.is_some());
        let kind = if kani::any() {
            NoteKind::Tap { lane }
        } else {
            NoteKind::Hold {
                lane,
                duration: duration.unwrap(),
            }
        };
        assert!(fits(lanes, kind) == (lane <= lanes.width().last()));
    }

    #[kani::proof]
    #[kani::unwind(8)]
    fn shakes_slices_and_mega_taps_fit_and_differ_by_direction() {
        use NoteKind::{MegaTap, Shake as S, Slice};
        let lanes = lanes();
        for kind in [
            S(Shake::Left),
            S(Shake::Right),
            S(Shake::Down),
            Slice(Side::Left),
            Slice(Side::Right),
            MegaTap,
        ] {
            assert!(fits(lanes, kind));
        }
        let [left, right, down] =
            [Shake::Left, Shake::Right, Shake::Down].map(|d| lanes.position(S(d)));
        assert!(left != right && right != down && left != down);
        assert!(lanes.position(Slice(Side::Left)) != lanes.position(Slice(Side::Right)));
    }

    #[kani::proof]
    #[kani::unwind(5)]
    fn lane_widths_read_back_exactly_when_ttr_supports_them() {
        let count: u16 = kani::any();
        let Some(width) = LaneCount::new(count) else {
            return;
        };
        assert!(Lanes::of(width).is_some() == (3..=5).contains(&count));
        if let Some(lanes) = Lanes::of(width) {
            assert!(lanes.width() == width);
        }
    }

    #[kani::proof]
    #[kani::unwind(16)]
    fn standard_difficulties_read_back_from_their_keys() {
        let level = Difficulty::ALL[kani::any_where(|index: &usize| *index < 4)];
        assert!(Difficulty::keyed(level.key()) == Some(level));
        assert!(level.index() < Difficulty::ALL.len());
    }
}
