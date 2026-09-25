//! Tap Tap Revenge as a charter ruleset.
//!
//! A song has 3, 4 or 5 lanes, the same in every chart. Notes scroll at a constant speed in
//! time, so a BPM change only moves the beat grid and never speeds play up or slows it down the
//! way StepMania's does. Stops, delays and warps have no meaning here.
//!
//! [`Song`] is the typed view of a document whose ruleset is [`ID`]. Reading one fails on
//! anything Tap Tap Revenge cannot represent and skips data other plugins left behind;
//! [`Song::to_document`] writes this ruleset's data alone. A song is checked when built, by
//! reading or by [`Song::new`], so writing one cannot fail. [`Plugin`] hands the editor the same
//! check, so a document is editable only while it reads as a song.

use std::collections::BTreeMap;

use charter_core::{
    Bpm, Chart as CoreChart, Document, Extensions, FORMAT_VERSION, InvalidValue, Key, Kind,
    LaneCount, LaneIndex, Location, Marker, MarkerKind, Metadata, Name, Namespace, Object, Origin,
    Position, Ruleset, RulesetPlugin, Settings, Shape, Stream, Tick, Ticks, TicksPerBeat,
    TimeSignature, Timing, TimingEvent, TimingKind, ValidDocument,
};

/// The ruleset namespace.
pub const ID: &str = "ttr";

/// A song, checked against this ruleset and every document rule when built, and unchangeable
/// after.
#[derive(Clone, Debug, PartialEq)]
pub struct Song {
    metadata: Metadata,
    ticks_per_beat: TicksPerBeat,
    lanes: Lanes,
    tempo: Tempo,
    markers: Vec<Marker>,
    charts: Vec<Chart>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lanes {
    Three,
    Four,
    Five,
}

/// A song's tempo map.
#[derive(Clone, Debug, PartialEq)]
pub struct Tempo {
    pub offset_seconds: Origin,
    pub changes: Vec<TempoChange>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TempoChange {
    pub tick: Tick,
    pub kind: TempoKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TempoKind {
    Bpm(Bpm),
    TimeSignature(TimeSignature),
}

/// One difficulty.
#[derive(Clone, Debug, PartialEq)]
pub struct Chart {
    pub name: String,
    /// Replaces the song's tempo map when present.
    pub tempo: Option<Tempo>,
    pub notes: Vec<Note>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Note {
    pub tick: Tick,
    pub kind: NoteKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteKind {
    Tap {
        lane: LaneIndex,
    },
    Hold {
        lane: LaneIndex,
        duration: Ticks,
    },
    /// Shake the device in this direction.
    Shake(Shake),
    /// Slice across the note.
    Slice(Side),
    /// Tapped anywhere across the whole tap area.
    MegaTap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shake {
    Left,
    Right,
    Down,
}

/// Written as the ruleset version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ruleset is \"{0}\", not {ID:?}")]
    WrongRuleset(Namespace),
    #[error("a tap tap revenge song needs at least one chart")]
    NoCharts,
    #[error("{at}: not a tap tap revenge note")]
    UnsupportedObject { at: Location },
    #[error("{at}: {what} are not part of tap tap revenge")]
    Unsupported { at: Location, what: &'static str },
    #[error("{at}: \"{key}\" is not a tap tap revenge key")]
    UnknownKey { at: Location, key: Key },
    #[error("{at}: \"{key}\" is not a tap tap revenge setting")]
    UnknownSetting { at: Location, key: String },
    #[error("{at} has {} lanes; every chart of a song has the same 3, 4 or 5", .lanes.map_or_else(|| "no".to_owned(), |lanes| lanes.to_string()))]
    Lanes {
        at: Location,
        lanes: Option<LaneCount>,
    },
    #[error(transparent)]
    Value(#[from] InvalidValue),
    #[error(transparent)]
    Document(#[from] charter_core::Error),
}

impl Song {
    /// A song of `charts`, each with `lanes` lanes.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCharts`] without a chart, [`Error::UnknownKey`] for metadata extension
    /// data, which this ruleset does not keep, or [`Error::Document`] for the first document rule
    /// the song breaks, such as a tap in a lane the song does not have.
    pub fn new(
        metadata: Metadata,
        ticks_per_beat: TicksPerBeat,
        lanes: Lanes,
        tempo: Tempo,
        markers: Vec<Marker>,
        charts: Vec<Chart>,
    ) -> Result<Self, Error> {
        let own = markers
            .iter()
            .enumerate()
            .find_map(|(m, marker)| match &marker.kind {
                MarkerKind::Other(key) if key.namespace().as_str() == ID => Some((m, key)),
                _ => None,
            });
        if let Some((m, key)) = own {
            return Err(Error::UnknownKey {
                at: Location::Marker(m),
                key: key.clone(),
            });
        }
        if charts.is_empty() {
            return Err(Error::NoCharts);
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
            lanes,
            tempo,
            markers,
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

    #[must_use]
    pub fn lanes(&self) -> Lanes {
        self.lanes
    }

    #[must_use]
    pub fn tempo(&self) -> &Tempo {
        &self.tempo
    }

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
    #[allow(clippy::expect_used)] // see Panics
    pub fn to_document(&self) -> ValidDocument {
        self.build().expect("a song was checked when built")
    }

    fn build(&self) -> Result<ValidDocument, Error> {
        let charts = self
            .charts
            .iter()
            .map(|chart| chart.to_core(self.lanes))
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
            samples: BTreeMap::new(),
            timing: self.tempo.to_timing(),
            markers: self.markers.clone(),
            charts,
        };
        Ok(ValidDocument::new(doc)?)
    }
}

impl Lanes {
    /// How many lanes each chart of the song has.
    #[must_use]
    pub const fn width(self) -> LaneCount {
        match self {
            Self::Three => lanes(3),
            Self::Four => lanes(4),
            Self::Five => lanes(5),
        }
    }

    fn of(width: LaneCount) -> Option<Self> {
        [Self::Three, Self::Four, Self::Five]
            .into_iter()
            .find(|lanes| lanes.width() == width)
    }

    /// Where a note of `kind` sits. A shake or slice takes the lane of its direction; a down
    /// shake takes the middle lane, or both middle lanes of four. A mega tap covers every lane.
    #[must_use]
    pub fn position(self, kind: NoteKind) -> Position {
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
    match LaneCount::new(n) {
        Some(lanes) => lanes,
        None => unreachable!(),
    }
}

const SHAKE: &str = "shake";
const SLICE: &str = "slice";
const MEGA_TAP: &str = "mega_tap";

/// This ruleset's kind called `name`.
fn own(name: &str) -> Result<Kind, InvalidValue> {
    Ok(Kind::Other(Key::new(Namespace::new(ID)?, Name::new(name)?)))
}

fn one_lane(index: LaneIndex) -> Position {
    Position::Lane {
        index,
        width: LaneCount::ONE,
    }
}

impl Tempo {
    fn to_timing(&self) -> Timing {
        let events = self.changes.iter().map(|change| TimingEvent {
            tick: change.tick,
            kind: match change.kind {
                TempoKind::Bpm(bpm) => TimingKind::Bpm(bpm),
                TempoKind::TimeSignature(meter) => TimingKind::TimeSignature(meter),
            },
            ext: Extensions::new(),
        });
        Timing {
            offset_seconds: self.offset_seconds,
            events: events.collect(),
        }
    }
}

impl Chart {
    fn to_core(&self, lanes: Lanes) -> Result<CoreChart, InvalidValue> {
        Ok(CoreChart {
            name: self.name.clone(),
            lanes: Some(lanes.width()),
            settings: Settings::new(),
            timing: self.tempo.as_ref().map(Tempo::to_timing),
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
        reject_keys(doc.settings.keys(), Location::Settings)?;
        reject_own_keys(&doc.metadata.ext, Location::MetadataExt)?;
        if !doc.samples.is_empty() {
            return Err(Error::Unsupported {
                at: Location::Samples,
                what: "keysounds",
            });
        }
        let lanes = song_lanes(&doc.charts)?;
        let charts = doc
            .charts
            .iter()
            .enumerate()
            .map(|(c, chart)| read_chart(c, chart, lanes))
            .collect::<Result<_, _>>()?;
        let metadata = Metadata {
            ext: Extensions::new(),
            ..doc.metadata.clone()
        };
        let markers = doc
            .markers
            .iter()
            .filter(|marker| marker.kind.namespace().is_none_or(|ns| ns.as_str() == ID))
            .cloned()
            .collect();
        let tempo = read_tempo(&doc.timing, Stream::Song)?;
        Self::new(metadata, doc.ticks_per_beat, lanes, tempo, markers, charts)
    }
}

/// The lane count every chart shares.
fn song_lanes(charts: &[CoreChart]) -> Result<Lanes, Error> {
    let lanes_of = |chart: &CoreChart| chart.lanes.and_then(Lanes::of);
    let first = charts.first().ok_or(Error::NoCharts)?;
    let first = lanes_of(first).ok_or(Error::Lanes {
        at: Location::Chart(0),
        lanes: first.lanes,
    })?;
    match charts
        .iter()
        .enumerate()
        .find(|(_, chart)| lanes_of(chart) != Some(first))
    {
        Some((chart, other)) => Err(Error::Lanes {
            at: Location::Chart(chart),
            lanes: other.lanes,
        }),
        None => Ok(first),
    }
}

/// This ruleset for the editor, which checks a document by reading it as a [`Song`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Plugin;

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
}

fn read_chart(c: usize, chart: &CoreChart, lanes: Lanes) -> Result<Chart, Error> {
    reject_keys(chart.settings.keys(), Location::ChartSettings(c))?;
    reject_own_keys(&chart.ext, Location::ChartExt(c))?;
    let notes = chart
        .objects
        .iter()
        .enumerate()
        .map(|(j, object)| {
            let at = Location::Object { chart: c, index: j };
            reject_own_keys(&object.ext, at)?;
            if !object.children.is_empty() {
                return Err(Error::Unsupported {
                    at,
                    what: "child objects",
                });
            }
            let kind = read_note(object, lanes).ok_or(Error::UnsupportedObject { at })?;
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
        name: chart.name.clone(),
        tempo,
        notes,
    })
}

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
        (Kind::Other(key), Shape::Instant) if key.namespace().as_str() == ID => {
            match key.name().as_str() {
                SHAKE => placed(&[
                    NoteKind::Shake(Shake::Left),
                    NoteKind::Shake(Shake::Right),
                    NoteKind::Shake(Shake::Down),
                ]),
                SLICE => placed(&[NoteKind::Slice(Side::Left), NoteKind::Slice(Side::Right)]),
                MEGA_TAP => placed(&[NoteKind::MegaTap]),
                _ => None,
            }
        }
        _ => None,
    }
}

fn read_tempo(timing: &Timing, stream: Stream) -> Result<Tempo, Error> {
    let mut changes = Vec::with_capacity(timing.events.len());
    for (index, event) in timing.events.iter().enumerate() {
        let at = Location::Event { stream, index };
        reject_own_keys(&event.ext, at)?;
        let unsupported = |what| Error::Unsupported { at, what };
        let kind = match &event.kind {
            TimingKind::Bpm(bpm) => TempoKind::Bpm(*bpm),
            TimingKind::TimeSignature(meter) => TempoKind::TimeSignature(*meter),
            TimingKind::Stop(_) => return Err(unsupported("stops")),
            TimingKind::Delay(_) => return Err(unsupported("delays")),
            TimingKind::Warp(_) => return Err(unsupported("warps")),
            TimingKind::Other { name, .. } if name.namespace().as_str() == ID => {
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
        offset_seconds: timing.offset_seconds,
        changes,
    })
}

/// Fails on the first key, since this ruleset defines no settings.
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

    /// A one-chart, three-lane ttr document holding `objects`.
    fn document(objects: Value) -> Value {
        let mut value = json!({
            "format_version": 0,
            "ruleset": { "id": "ttr", "version": "0" },
            "metadata": { "title": "t", "artist": "a", "audio": "song.ogg" },
            "timing": { "offset_seconds": -0.25, "events": [ { "tick": 0, "kind": { "bpm": 150.0 } } ] },
            "charts": [ { "name": "Easy", "lanes": 3 } ]
        });
        value["charts"][0]["objects"] = objects;
        value
    }

    fn lane_object(tick: i32, index: u16, width: u16, kind: &str) -> Value {
        json!({ "tick": tick, "position": { "lane": { "index": index, "width": width } },
                "kind": kind, "shape": "instant" })
    }

    fn read(value: &Value) -> Result<Song, Error> {
        Song::try_from(&ValidDocument::from_json(&serde_json::to_vec(value).unwrap()).unwrap())
    }

    #[test]
    fn reads_taps_in_a_three_lane_song() {
        let song = read(&document(json!([lane_object(0, 1, 1, "tap")]))).unwrap();
        assert_eq!(song.lanes(), Lanes::Three);
        assert_eq!(
            song.tempo().changes,
            [TempoChange {
                tick: Tick::new(0),
                kind: TempoKind::Bpm(Bpm::try_from(150.0).unwrap())
            }]
        );
        assert_eq!(
            song.charts(),
            [Chart {
                name: "Easy".into(),
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
        assert!(Plugin.check(&song).is_ok());
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
            assert_eq!(song.lanes(), lanes);
            assert_eq!(written(&song)["charts"][0]["lanes"], json!(count));
        }
    }

    #[test]
    fn every_chart_shares_the_songs_three_four_or_five_lanes() {
        let mut mismatched = with_lanes(4);
        mismatched["charts"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "name": "Hard", "lanes": 5 }));
        for value in [with_lanes(2), with_lanes(6), mismatched] {
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
    fn a_mega_tap_covers_every_lane_of_its_tick() {
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
        let crowded = Song::new(
            song.metadata().clone(),
            song.ticks_per_beat(),
            song.lanes(),
            song.tempo().clone(),
            song.markers().to_vec(),
            charts,
        );
        assert!(matches!(crowded, Err(Error::Document(_))));
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
        events.push(event(3840, json!({ "bpm": 75.0 })));
        let song = read(&value).unwrap();
        let meter = |n| std::num::NonZeroU16::new(n).unwrap();
        assert_eq!(
            song.tempo().changes,
            [
                TempoChange {
                    tick: Tick::new(0),
                    kind: TempoKind::Bpm(Bpm::try_from(150.0).unwrap())
                },
                TempoChange {
                    tick: Tick::new(0),
                    kind: TempoKind::TimeSignature(TimeSignature {
                        numerator: meter(3),
                        denominator: meter(4)
                    })
                },
                TempoChange {
                    tick: Tick::new(3840),
                    kind: TempoKind::Bpm(Bpm::try_from(75.0).unwrap())
                },
            ]
        );
        assert_eq!(written(&song)["timing"], value["timing"]);

        for gimmick in [
            json!({ "stop": 1.0 }),
            json!({ "delay": 1.0 }),
            json!({ "warp": 960 }),
        ] {
            let mut value = document(json!([]));
            value["timing"]["events"]
                .as_array_mut()
                .unwrap()
                .push(event(960, gimmick));
            assert!(
                matches!(read(&value), Err(Error::Unsupported { .. })),
                "{value}"
            );
        }
    }

    #[test]
    fn a_chart_can_carry_its_own_tempo() {
        let own =
            json!({ "offset_seconds": 0.5, "events": [ { "tick": 0, "kind": { "bpm": 200.0 } } ] });
        let mut value = document(json!([]));
        value["charts"][0]["timing"] = own.clone();
        let song = read(&value).unwrap();
        let tempo = song.charts()[0].tempo.as_ref().unwrap();
        assert_eq!(tempo.offset_seconds, Origin::try_from(0.5).unwrap());
        assert_eq!(
            tempo.changes,
            [TempoChange {
                tick: Tick::new(0),
                kind: TempoKind::Bpm(Bpm::try_from(200.0).unwrap())
            }]
        );
        assert_eq!(written(&song)["charts"][0]["timing"], own);
    }

    #[test]
    fn keys_in_the_ttr_namespace_or_settings_that_it_does_not_define_are_refused() {
        type Edit = fn(&mut Value);
        let edits: [Edit; 7] = [
            |v| v["charts"][0]["objects"][0]["ext"] = json!({ "ttr.color": 1 }),
            |v| v["metadata"]["ext"] = json!({ "ttr.banner": "b.png" }),
            |v| v["charts"][0]["ext"] = json!({ "ttr.rating": 3 }),
            |v| v["timing"]["events"][0]["ext"] = json!({ "ttr.x": 1 }),
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
                song.lanes(),
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
            name: "Hard".into(),
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
}
