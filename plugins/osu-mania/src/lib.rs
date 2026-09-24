//! osu!mania as a charter ruleset.
//!
//! A chart's lane count is its key count. Notes are taps and holds. Timing is BPM, meter in
//! beats per measure, and scroll speed changes; osu! has no stops, delays or warps.
//!
//! [`Song`] is the typed view of a document whose ruleset is [`ID`]. Reading one fails on
//! anything osu!mania cannot represent and skips data other plugins left behind;
//! [`Song::to_document`] writes this ruleset's data alone. A song is checked when built, by
//! reading or by [`Song::new`], so writing one cannot fail.

use std::collections::BTreeMap;
use std::num::NonZeroU16;

use charter_core::{
    Bpm, Chart as CoreChart, Decimal, Document, Extensions, FORMAT_VERSION, InvalidValue, Key,
    Kind, LaneCount, LaneIndex, Location, Metadata, Name, Namespace, Object, Origin, Position,
    ResourcePath, Ruleset, SampleId, Settings, Shape, Stream, Tick, Ticks, TicksPerBeat,
    TimeSignature, Timing as CoreTiming, TimingEvent as CoreEvent, TimingKind as CoreKind,
    ValidDocument,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The ruleset namespace.
pub const ID: &str = "osu_mania";

/// A song, checked against this ruleset and every document rule when built, and unchangeable
/// after.
#[derive(Clone, Debug, PartialEq)]
pub struct Song {
    metadata: Metadata,
    ticks_per_beat: TicksPerBeat,
    samples: BTreeMap<SampleId, ResourcePath>,
    timing: Timing,
    charts: Vec<Chart>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Timing {
    pub offset_seconds: Origin,
    pub events: Vec<TimingEvent>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimingEvent {
    pub tick: Tick,
    pub kind: TimingKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TimingKind {
    Bpm(Bpm),
    /// Beats per measure. osu! meters are always in quarter notes.
    Meter(NonZeroU16),
    /// Scroll speed multiplier, osu!'s slider velocity on an inherited timing point.
    ScrollSpeed(ScrollSpeed),
}

/// One difficulty, which osu! calls a beatmap's version.
#[derive(Clone, Debug, PartialEq)]
pub struct Chart {
    pub name: String,
    pub keys: Keys,
    /// HP drain rate.
    pub hp: Difficulty,
    /// Overall difficulty, which sets the timing windows.
    pub od: Difficulty,
    /// Replaces the song's timing when present. osu! stores timing per beatmap.
    pub timing: Option<Timing>,
    pub notes: Vec<Note>,
}

/// A key count osu!mania plays: 1 to 10 on one stage, or an even count up to 20 split across
/// two; see `MAX_STAGE_KEYS` in osu!lazer's `ManiaRuleset`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keys {
    K1,
    K2,
    K3,
    K4,
    K5,
    K6,
    K7,
    K8,
    K9,
    K10,
    K12,
    K14,
    K16,
    K18,
    K20,
}

impl Keys {
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
    match Decimal::from_millionths(n * 1_000_000) {
        Some(value) => value,
        None => unreachable!(),
    }
}

fn default_difficulty() -> Difficulty {
    Difficulty::DEFAULT
}

/// Chart settings as stored in the document.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChartSettings {
    #[serde(default = "default_difficulty")]
    hp: Difficulty,
    #[serde(default = "default_difficulty")]
    od: Difficulty,
}

impl Difficulty {
    /// osu!'s value when a beatmap does not set one.
    pub const DEFAULT: Self = Self(whole(5));

    #[must_use]
    pub fn new(value: Decimal) -> Option<Self> {
        (whole(0)..=whole(10))
            .contains(&value)
            .then_some(Self(value))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub tick: Tick,
    pub column: LaneIndex,
    /// Ticks held, for a hold note.
    pub hold: Option<Ticks>,
    pub keysound: Option<SampleId>,
}

const SCROLL_SPEED: &str = "scroll_speed";

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
        charts: Vec<Chart>,
    ) -> Result<Self, Error> {
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
            charts,
        };
        song.build()?;
        Ok(song)
    }

    #[must_use]
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    #[must_use]
    pub fn ticks_per_beat(&self) -> TicksPerBeat {
        self.ticks_per_beat
    }

    /// Keysounds.
    #[must_use]
    pub fn samples(&self) -> &BTreeMap<SampleId, ResourcePath> {
        &self.samples
    }

    #[must_use]
    pub fn timing(&self) -> &Timing {
        &self.timing
    }

    #[must_use]
    pub fn charts(&self) -> &[Chart] {
        &self.charts
    }

    /// Writes the song as a document holding only this ruleset's data.
    ///
    /// # Panics
    ///
    /// Never: [`Song::new`] checked the same document, and a song cannot change.
    #[must_use]
    #[allow(clippy::expect_used)] // see Panics
    pub fn to_document(&self) -> ValidDocument {
        self.build().expect("a song was checked when built")
    }

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
            charts,
        };
        Ok(ValidDocument::new(doc)?)
    }
}

impl Timing {
    fn to_core(&self) -> Result<CoreTiming, Error> {
        let events = self.events.iter().map(|event| {
            let kind = match event.kind {
                TimingKind::Bpm(bpm) => CoreKind::Bpm(bpm),
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
            offset_seconds: self.offset_seconds,
            events: events.collect::<Result<_, Error>>()?,
        })
    }
}

impl Chart {
    fn to_core(&self) -> Result<CoreChart, Error> {
        let settings = serde_json::to_value(ChartSettings {
            hp: self.hp,
            od: self.od,
        })?;
        let settings = match settings {
            Value::Object(map) => map.into_iter().collect(),
            _ => Settings::new(),
        };
        Ok(CoreChart {
            name: self.name.clone(),
            lanes: Some(self.keys.lanes()),
            settings,
            timing: self.timing.as_ref().map(Timing::to_core).transpose()?,
            objects: self.notes.iter().map(Note::to_object).collect(),
            ext: Extensions::new(),
        })
    }
}

impl Note {
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
            sample: self.keysound.clone(),
            children: Vec::new(),
            ext: Extensions::new(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ruleset is \"{0}\", not {ID:?}")]
    WrongRuleset(Namespace),
    #[error("{at}: osu!mania does not play {} keys", .found.map_or_else(|| "no".to_owned(), |lanes| lanes.to_string()))]
    Keys {
        at: Location,
        found: Option<LaneCount>,
    },
    #[error("{at}: {what} are not part of osu!mania")]
    Unsupported { at: Location, what: &'static str },
    #[error("{at}: not an osu!mania note")]
    UnsupportedObject { at: Location },
    #[error("{at}: \"{key}\" is not an osu!mania key")]
    UnknownKey { at: Location, key: Key },
    #[error("{at}: \"{key}\" is not an osu!mania setting")]
    UnknownSetting { at: Location, key: String },
    #[error("{at}: {source}")]
    Malformed {
        at: Location,
        source: serde_json::Error,
    },
    #[error(transparent)]
    Value(#[from] InvalidValue),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
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
        let timing = read_timing(&doc.timing, Stream::Song)?;
        Self::new(
            metadata,
            doc.ticks_per_beat,
            doc.samples.clone(),
            timing,
            charts,
        )
    }
}

fn read_chart(chart: &CoreChart, c: usize) -> Result<Chart, Error> {
    reject_own_keys(&chart.ext, Location::ChartExt(c))?;
    let keys = chart.lanes.and_then(Keys::of).ok_or(Error::Keys {
        at: Location::Chart(c),
        found: chart.lanes,
    })?;
    let settings = Value::Object(chart.settings.clone().into_iter().collect());
    let settings: ChartSettings =
        serde_json::from_value(settings).map_err(|source| Error::Malformed {
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

fn read_timing(timing: &CoreTiming, stream: Stream) -> Result<Timing, Error> {
    let mut events = Vec::with_capacity(timing.events.len());
    for (index, event) in timing.events.iter().enumerate() {
        let at = Location::Event { stream, index };
        reject_own_keys(&event.ext, at)?;
        let unsupported = |what| Error::Unsupported { at, what };
        let kind = match &event.kind {
            CoreKind::Bpm(bpm) => TimingKind::Bpm(*bpm),
            CoreKind::TimeSignature(TimeSignature {
                numerator,
                denominator: FOUR,
            }) => TimingKind::Meter(*numerator),
            CoreKind::TimeSignature(..) => return Err(unsupported("meters outside quarter notes")),
            CoreKind::Stop(_) => return Err(unsupported("stops")),
            CoreKind::Delay(_) => return Err(unsupported("delays")),
            CoreKind::Warp(_) => return Err(unsupported("warps")),
            CoreKind::Other { name, value }
                if name.namespace().as_str() == ID && name.name().as_str() == SCROLL_SPEED =>
            {
                TimingKind::ScrollSpeed(
                    ScrollSpeed::deserialize(value)
                        .map_err(|source| Error::Malformed { at, source })?,
                )
            }
            CoreKind::Other { name, .. } if name.namespace().as_str() == ID => {
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
        offset_seconds: timing.offset_seconds,
        events,
    })
}

/// Fails on a key in this ruleset's namespace, which defines none; other plugins' keys are
/// theirs to read.
fn reject_own_keys(ext: &Extensions, at: Location) -> Result<(), Error> {
    match ext.keys().find(|key| key.namespace().as_str() == ID) {
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
            "format_version": 0,
            "ruleset": { "id": "osu_mania", "version": "0" },
            "metadata": { "title": "t", "artist": "a", "audio": "audio.mp3" },
            "timing": { "offset_seconds": 0.042, "events": [ { "tick": 0, "kind": { "bpm": 140.0 } } ] },
            "charts": [ { "name": "7K Hard", "lanes": 7, "settings": { "hp": 8.0, "od": 8.5 } } ]
        });
        value["charts"][0]["objects"] = objects;
        value
    }

    fn tap(tick: i32, column: u16) -> Value {
        json!({ "tick": tick, "position": { "lane": { "index": column } }, "kind": "tap", "shape": "instant" })
    }

    fn read(value: &Value) -> Result<Song, Error> {
        Song::try_from(&ValidDocument::from_json(&serde_json::to_vec(value).unwrap()).unwrap())
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
            json!({ "tick": 3840, "kind": { "bpm": 70.0 } }),
        ]);
        let song = read(&value).unwrap();
        let kinds: Vec<TimingKind> = song
            .timing()
            .events
            .iter()
            .map(|event| event.kind)
            .collect();
        assert_eq!(
            kinds,
            [
                TimingKind::Bpm(Bpm::try_from(140.0).unwrap()),
                TimingKind::Meter(std::num::NonZeroU16::new(3).unwrap()),
                TimingKind::ScrollSpeed(ScrollSpeed::new(Decimal::try_from(0.5).unwrap()).unwrap()),
                TimingKind::Bpm(Bpm::try_from(70.0).unwrap()),
            ]
        );
        assert_eq!(written(&song)["timing"], value["timing"]);
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
        let own =
            json!({ "offset_seconds": 0.0, "events": [ { "tick": 0, "kind": { "bpm": 70.0 } } ] });
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
        let edits: [Edit; 6] = [
            |v| v["charts"][0]["objects"][0]["ext"] = json!({ "osu_mania.hitsound": 2 }),
            |v| v["metadata"]["ext"] = json!({ "osu_mania.source": "x" }),
            |v| v["charts"][0]["ext"] = json!({ "osu_mania.tags": "x" }),
            |v| v["timing"]["events"][0]["ext"] = json!({ "osu_mania.kiai": true }),
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
}
