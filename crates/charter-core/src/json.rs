use std::collections::HashSet;
use std::fmt;

use serde::Deserialize;
use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};

use crate::document::{Chart, Child, Document, FORMAT_VERSION, Object, Shape, Timing};
use crate::error::Error;
use crate::grid::{GridPosition, Measures};
use crate::unit::{ResolvedTime, Tick};
use crate::validate::validate;

/// Largest document a reader accepts.
pub const MAX_DOCUMENT_BYTES: usize = 64 << 20;

/// A [`Document`] that passed every rule of the format.
#[derive(Clone, Debug, PartialEq)]
pub struct ValidDocument(Document);

impl ValidDocument {
    /// Reads a document from JSON.
    ///
    /// # Errors
    ///
    /// Fails on documents over [`MAX_DOCUMENT_BYTES`] or from a newer format version, on
    /// malformed JSON, or on a value outside its type.
    pub fn from_json(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_DOCUMENT_BYTES {
            return Err(Error::TooLarge(bytes.len()));
        }
        let VersionProbe { format_version } = serde_json::from_slice(bytes)?;
        if format_version > FORMAT_VERSION {
            return Err(Error::UnsupportedVersion(format_version));
        }
        serde_json::from_slice::<IJson>(bytes)?;
        Self::new(serde_json::from_slice(bytes)?)
    }

    /// Checks the version and every rule relating values to each other.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedVersion`] for a version newer than [`FORMAT_VERSION`], or
    /// [`Error::Invalid`] naming the first rule `document` breaks and where.
    pub fn new(document: Document) -> Result<Self, Error> {
        if document.format_version > FORMAT_VERSION {
            return Err(Error::UnsupportedVersion(document.format_version));
        }
        validate(&document)?;
        Ok(Self(document))
    }

    /// Writes the document as pretty-printed JSON, omitting every field that holds its default.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Json`] if serialization fails, which the document's types rule out.
    pub fn to_json(&self) -> Result<Vec<u8>, Error> {
        Ok(serde_json::to_vec_pretty(&self.0)?)
    }

    #[must_use]
    pub fn document(&self) -> &Document {
        &self.0
    }

    /// The song's timing map.
    #[must_use]
    pub fn timing(&self) -> Valid<'_, Timing> {
        Valid {
            part: &self.0.timing,
            document: &self.0,
        }
    }

    /// Every chart, in order.
    pub fn charts(&self) -> impl Iterator<Item = Valid<'_, Chart>> {
        self.0.charts.iter().map(|part| Valid {
            part,
            document: &self.0,
        })
    }
}

/// A part of a [`ValidDocument`], so known to pass every rule. Derefs to the part.
///
/// Rules that relate a part to the rest of the document hold here, so what a raw part can only
/// compute fallibly, a valid one computes outright: a time for every tick, an end for every span.
#[derive(Debug)]
pub struct Valid<'a, T> {
    part: &'a T,
    document: &'a Document,
}

impl<T> Clone for Valid<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Valid<'_, T> {}

impl<T> std::ops::Deref for Valid<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.part
    }
}

impl<'a> Valid<'a, Chart> {
    /// The timing map the chart's objects resolve through: its own, or the song's.
    #[must_use]
    pub fn timing(&self) -> Valid<'a, Timing> {
        Valid {
            part: self.part.timing.as_ref().unwrap_or(&self.document.timing),
            document: self.document,
        }
    }

    pub fn objects(&self) -> impl Iterator<Item = Valid<'a, Object>> + 'a {
        let document = self.document;
        self.part
            .objects
            .iter()
            .map(move |part| Valid { part, document })
    }
}

impl<'a> Valid<'a, Object> {
    /// The tick where the object's span ends.
    #[must_use]
    pub fn end(&self) -> Tick {
        span_end(self.part.tick, &self.part.shape)
    }

    pub fn children(&self) -> impl Iterator<Item = Valid<'a, Child>> + 'a {
        let document = self.document;
        self.part
            .children
            .iter()
            .map(move |part| Valid { part, document })
    }
}

impl Valid<'_, Child> {
    /// The tick where the child's span ends.
    #[must_use]
    pub fn end(&self) -> Tick {
        span_end(self.part.tick, &self.part.shape)
    }
}

impl Valid<'_, Timing> {
    /// Audio time of an object on `tick`, at the document's resolution. The time of a tick
    /// includes a `delay` on it and excludes a `stop` on it; warps take no time, and the first
    /// `bpm` also rules earlier ticks.
    ///
    /// # Panics
    ///
    /// Never: validation guarantees a `bpm` and warps that end on a tick.
    #[must_use]
    #[allow(clippy::expect_used)] // see Panics
    pub fn time_at(&self, tick: Tick) -> ResolvedTime {
        self.part
            .seconds_at(self.document.ticks_per_beat, tick)
            .expect("a valid timing map resolves every tick")
    }

    /// The measures of the grid from the one holding `from` onward. Common time rules from tick
    /// 0 until the first time signature, and each time signature restarts the grid at its tick.
    ///
    /// # Panics
    ///
    /// Never: validation guarantees every measure is a whole number of ticks.
    #[must_use]
    #[allow(clippy::expect_used)] // see Panics
    pub fn measures_from(&self, from: Tick) -> Measures {
        self.part
            .measures_from(self.document.ticks_per_beat, from)
            .expect("a valid timing map has whole measures")
    }

    /// Where `tick` falls in the grid, as its measure, beat and remaining ticks.
    ///
    /// # Panics
    ///
    /// Never: validation guarantees every measure is a whole number of ticks.
    #[must_use]
    #[allow(clippy::expect_used)] // see Panics
    pub fn grid_position(&self, tick: Tick) -> GridPosition {
        self.part
            .grid_position(self.document.ticks_per_beat, tick)
            .expect("a valid timing map has whole measures")
    }
}

#[allow(clippy::expect_used)] // validation guarantees every span ends on a tick
fn span_end(tick: Tick, shape: &Shape) -> Tick {
    shape.end(tick).expect("a valid span ends on a tick")
}

/// Reads the version before anything else, so a newer file fails on its version rather than on
/// the first key this reader does not know.
#[derive(Deserialize)]
struct VersionProbe {
    format_version: u32,
}

/// Largest integer magnitude I-JSON allows, `2^53 - 1`: every integer up to it is a double.
const MAX_EXACT_INTEGER: u64 = (1 << 53) - 1;

/// Walks any JSON value and fails where it breaks I-JSON: on a repeated key, which `serde_json`
/// would resolve by keeping the last, and on an integer a double cannot hold exactly.
struct IJson;

impl<'de> Deserialize<'de> for IJson {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(IJsonVisitor)
    }
}

struct IJsonVisitor;

impl<'de> Visitor<'de> for IJsonVisitor {
    type Value = IJson;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("any JSON value")
    }

    fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
        Ok(IJson)
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
        if v.unsigned_abs() > MAX_EXACT_INTEGER {
            return Err(inexact(v));
        }
        Ok(IJson)
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
        if v > MAX_EXACT_INTEGER {
            return Err(inexact(v));
        }
        Ok(IJson)
    }

    fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E> {
        Ok(IJson)
    }

    fn visit_str<E>(self, _: &str) -> Result<Self::Value, E> {
        Ok(IJson)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(IJson)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        while seq.next_element::<IJson>()?.is_some() {}
        Ok(IJson)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut seen = HashSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if seen.contains(&key) {
                return Err(de::Error::custom(format_args!("duplicate key {key:?}")));
            }
            map.next_value::<IJson>()?;
            seen.insert(key);
        }
        Ok(IJson)
    }
}

fn inexact<E: de::Error>(v: impl fmt::Display) -> E {
    E::custom(format_args!(
        "integer {v} is beyond what a double holds exactly"
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::num::NonZeroU16;
    use std::ops::RangeInclusive;

    use proptest::collection::{btree_map, vec};
    use proptest::option;
    use proptest::prelude::*;
    use serde_json::{Value, json};

    use super::*;
    use crate::document::{
        Chart, Child, Extensions, Marker, Metadata, Object, PathPoint, Points, Position, Preview,
        Ruleset, SampleId, Settings, Shape, Timing, TimingEvent, TimingKind,
    };
    use crate::error::{InvalidValue, Rule, Violation};
    use crate::ident::{Ease, Key, Kind, MarkerKind, Namespace};
    use crate::number::{MAX_MILLIONTHS, from_millionths};
    use crate::path::ResourcePath;
    use crate::unit::{
        AudioTime, LaneCount, LaneIndex, Seconds, Tick, Ticks, TicksPerBeat, TimeSignature,
    };

    fn minimal() -> Value {
        json!({
            "format_version": 0,
            "ruleset": { "id": "test", "version": "1" },
            "metadata": { "title": "t", "artist": "a", "audio": "song.ogg" },
            "timing": { "offset_seconds": 0.0, "events": [ { "tick": 0, "kind": { "bpm": 120.0 } } ] }
        })
    }

    fn read(value: &Value) -> Result<ValidDocument, Error> {
        ValidDocument::from_json(&serde_json::to_vec(value).unwrap())
    }

    /// Reads `value` and returns what the writer makes of it.
    fn round_trip(value: &Value) -> Value {
        serde_json::from_slice(&read(value).unwrap().to_json().unwrap()).unwrap()
    }

    /// The format chapter's full example, embedded at compile time so the test does no I/O.
    const SPEC: &str = include_str!("../../../docs/src/format.md");

    fn spec_example() -> Value {
        let start = SPEC.rfind("```json\n").unwrap() + "```json\n".len();
        let end = start + SPEC[start..].find("```").unwrap();
        serde_json::from_str(&SPEC[start..end]).unwrap()
    }

    #[test]
    fn a_minimal_document_reads_and_writes_back_unchanged() {
        assert_eq!(round_trip(&minimal()), minimal());
    }

    #[test]
    fn the_format_chapter_example_turns_seven_eight_on_measure_sixteen() {
        let song = read(&spec_example()).unwrap();
        let timing = song.timing();
        let drop = Tick::new(61440);
        assert_eq!(timing.grid_position(drop).to_string(), "16:0:0");
        let measure = timing.measures_from(drop).next().unwrap();
        assert_eq!((measure.number, measure.start), (16, drop));
        assert_eq!(measure.meter.to_string(), "7/8");
    }

    #[test]
    fn the_format_chapter_example_survives_a_round_trip() {
        let example = spec_example();
        assert_eq!(round_trip(&example), example);
    }

    #[test]
    fn newer_versions_are_refused_before_their_keys_are_read() {
        let mut value = minimal();
        value["format_version"] = json!(1);
        value["from_the_future"] = json!(true);
        assert!(matches!(read(&value), Err(Error::UnsupportedVersion(1))));
    }

    #[test]
    fn oversized_documents_are_refused_unread() {
        let bytes = vec![b' '; MAX_DOCUMENT_BYTES + 1];
        assert!(matches!(
            ValidDocument::from_json(&bytes),
            Err(Error::TooLarge(n)) if n == bytes.len()
        ));
    }

    #[test]
    fn duplicate_keys_are_rejected_at_any_depth() {
        let mut nested = minimal();
        nested["metadata"]["ext"] = json!({ "test.a": [ { "b": 1 } ] });
        let nested = serde_json::to_string(&nested)
            .unwrap()
            .replace(r#"{"b":1}"#, r#"{"b":1,"b":2}"#);
        let top = serde_json::to_string(&minimal())
            .unwrap()
            .replace(r#""title":"t""#, r#""title":"t","title":"u""#);
        for text in [nested, top] {
            assert!(
                matches!(
                    ValidDocument::from_json(text.as_bytes()),
                    Err(Error::Json(_))
                ),
                "{text}"
            );
        }
    }

    #[test]
    fn the_reader_refuses_documents_that_break_a_rule() {
        let mut value = minimal();
        value["timing"]["events"][0]["kind"] = json!({ "time_signature": [4, 4] });
        assert!(matches!(
            read(&value),
            Err(Error::Invalid(Violation {
                rule: Rule::NoBpm,
                ..
            }))
        ));
    }

    #[test]
    fn documents_built_in_code_are_validated_too() {
        let mut document: Document = serde_json::from_value(minimal()).unwrap();
        document.timing.events.clear();
        assert!(matches!(
            ValidDocument::new(document),
            Err(Error::Invalid(Violation {
                rule: Rule::NoBpm,
                ..
            }))
        ));
    }

    #[test]
    fn documents_built_in_code_cannot_claim_a_newer_version() {
        let mut document: Document = serde_json::from_value(minimal()).unwrap();
        document.format_version = FORMAT_VERSION + 7;
        assert!(matches!(
            ValidDocument::new(document),
            Err(Error::UnsupportedVersion(v)) if v == FORMAT_VERSION + 7
        ));
    }

    #[test]
    fn full_precision_numbers_survive_a_round_trip() {
        let mut value = minimal();
        value["metadata"]["ext"] = json!({ "test.offset": -1.840_801_817_358_691_7 });
        assert_eq!(round_trip(&value), value);
    }

    #[test]
    fn integers_a_double_cannot_hold_exactly_are_rejected() {
        for (n, accepted) in [
            (json!(9_007_199_254_740_991_u64), true),
            (json!(-9_007_199_254_740_991_i64), true),
            (json!(9_007_199_254_740_992_u64), false),
            (json!(-9_007_199_254_740_992_i64), false),
        ] {
            let mut value = minimal();
            value["settings"] = json!({ "n": n });
            assert_eq!(read(&value).is_ok(), accepted, "{value}");
        }
    }

    /// Lanes in every generated chart.
    const LANES: u16 = 8;

    /// A decimal-backed value built from a count of millionths in `range`, ends drawn often.
    fn decimal<T>(range: RangeInclusive<i64>) -> BoxedStrategy<T>
    where
        T: TryFrom<f64, Error = InvalidValue> + fmt::Debug + 'static,
    {
        prop_oneof![Just(*range.start()), Just(*range.end()), range]
            .prop_map(|m| T::try_from(from_millionths(m)).unwrap())
            .boxed()
    }

    fn key() -> BoxedStrategy<Key> {
        "[a-z0-9_.]{1,6}"
            .prop_map(|name| format!("test.{name}").parse().unwrap())
            .boxed()
    }

    /// Any I-JSON value: integers a double holds exactly, finite floats, nesting a few deep.
    fn value() -> BoxedStrategy<Value> {
        let exact = i64::try_from(MAX_EXACT_INTEGER).unwrap();
        let leaf = prop_oneof![
            Just(Value::Null),
            any::<bool>().prop_map(Value::from),
            (-exact..=exact).prop_map(Value::from),
            any::<f64>()
                .prop_filter("finite", |f| f.is_finite())
                .prop_map(Value::from),
            "\\PC{0,8}".prop_map(Value::from),
        ];
        leaf.prop_recursive(3, 16, 4, |inner| {
            prop_oneof![
                vec(inner.clone(), 0..4).prop_map(Value::from),
                btree_map("\\PC{0,6}", inner, 0..4)
                    .prop_map(|map| Value::Object(map.into_iter().collect())),
            ]
        })
        .boxed()
    }

    fn ext() -> BoxedStrategy<Extensions> {
        btree_map(key(), value(), 0..3).boxed()
    }

    fn settings() -> BoxedStrategy<Settings> {
        btree_map("[a-z_]{1,6}", value(), 0..3).boxed()
    }

    fn kind() -> BoxedStrategy<Kind> {
        prop_oneof![
            Just(Kind::Tap),
            Just(Kind::Hold),
            Just(Kind::Roll),
            Just(Kind::Mine),
            Just(Kind::Lift),
            Just(Kind::Fake),
            key().prop_map(Kind::Other),
        ]
        .boxed()
    }

    fn ease() -> BoxedStrategy<Ease> {
        prop_oneof![
            Just(Ease::Linear),
            Just(Ease::Step),
            Just(Ease::In),
            Just(Ease::Out),
            Just(Ease::InOut),
            key().prop_map(Ease::Other),
        ]
        .boxed()
    }

    /// A lane position within [`LANES`], or a point on the plane.
    fn position(plane: bool) -> BoxedStrategy<Position> {
        if plane {
            (
                decimal(-MAX_MILLIONTHS..=MAX_MILLIONTHS),
                decimal(-MAX_MILLIONTHS..=MAX_MILLIONTHS),
            )
                .prop_map(|(x, y)| Position::Plane { x, y })
                .boxed()
        } else {
            (0..LANES)
                .prop_flat_map(|index| (Just(index), 1..=LANES - index))
                .prop_map(|(index, width)| Position::Lane {
                    index: LaneIndex::new(index),
                    width: LaneCount::new(width).unwrap(),
                })
                .boxed()
        }
    }

    /// An object on `tick` whose path, if any, and children stay in its span and geometry.
    fn object(tick: i32, samples: usize) -> BoxedStrategy<Object> {
        any::<bool>()
            .prop_flat_map(move |plane| {
                let path = vec((position(plane), ease(), ext()), 1..3).prop_map(move |points| {
                    let points = points
                        .into_iter()
                        .zip(1..)
                        .map(|((position, ease, ext), j)| PathPoint {
                            tick: Tick::new(tick + 100 * j),
                            position,
                            ease,
                            ext,
                        });
                    Shape::Path {
                        points: Points::new(points.collect()).unwrap(),
                    }
                });
                let shape = prop_oneof![
                    Just(Shape::Instant),
                    (1_u32..=900).prop_map(|d| Shape::Sustain {
                        duration: Ticks::new(d).unwrap()
                    }),
                    path,
                ];
                let sample = if samples == 0 {
                    Just(None).boxed()
                } else {
                    option::of((0..samples).prop_map(|s| SampleId::new(format!("s{s}")).unwrap()))
                        .boxed()
                };
                let children = vec((0_i32..=1000, position(plane), kind(), ext()), 0..3);
                (position(plane), kind(), shape, sample, children, ext())
            })
            .prop_map(move |(position, kind, shape, sample, children, ext)| {
                let start = Tick::new(tick);
                let end = shape.end(start).unwrap();
                let children = children
                    .into_iter()
                    .map(|(offset, position, kind, ext)| Child {
                        tick: Tick::new(tick + offset).min(end),
                        position,
                        kind,
                        shape: Shape::Instant,
                        sample: None,
                        ext,
                    })
                    .collect();
                Object {
                    tick: start,
                    position,
                    kind,
                    shape,
                    sample,
                    children,
                    ext,
                }
            })
            .boxed()
    }

    /// A timing map with a tempo on tick 0 and one event on each of a few other ticks, spaced
    /// so no warp overlaps the next and every meter is whole at any resolution.
    fn timing() -> BoxedStrategy<Timing> {
        let event = prop_oneof![
            decimal(1..=MAX_MILLIONTHS).prop_map(TimingKind::Bpm),
            decimal(1..=MAX_MILLIONTHS).prop_map(TimingKind::Stop),
            decimal(1..=MAX_MILLIONTHS).prop_map(TimingKind::Delay),
            (1_u32..=16).prop_map(|w| TimingKind::Warp(Ticks::new(w).unwrap())),
            (1_u16..=16, prop_oneof![Just(1_u16), Just(2), Just(4)]).prop_map(|(n, d)| {
                TimingKind::TimeSignature(TimeSignature {
                    numerator: NonZeroU16::new(n).unwrap(),
                    denominator: NonZeroU16::new(d).unwrap(),
                })
            }),
            (key(), value()).prop_map(|(name, value)| TimingKind::Other { name, value }),
        ];
        let ticks = prop_oneof![-8_i32..=-1, 1_i32..=16];
        (
            decimal(-MAX_MILLIONTHS..=MAX_MILLIONTHS),
            decimal(1..=MAX_MILLIONTHS),
            btree_map(ticks, (event, ext()), 0..6),
        )
            .prop_map(|(offset_seconds, bpm, mut events)| {
                events.insert(0, (TimingKind::Bpm(bpm), Extensions::new()));
                let events = events
                    .into_iter()
                    .map(|(k, (kind, ext))| TimingEvent {
                        tick: Tick::new(16 * k),
                        kind,
                        ext,
                    })
                    .collect();
                Timing {
                    offset_seconds,
                    events,
                }
            })
            .boxed()
    }

    fn chart(name: String, samples: usize) -> BoxedStrategy<Chart> {
        (0_i32..4)
            .prop_flat_map(move |count| {
                let objects: Vec<_> = (0..count).map(|i| object(1024 * i, samples)).collect();
                (objects, settings(), option::of(timing()), ext())
            })
            .prop_map(move |(objects, settings, timing, ext)| Chart {
                name: name.clone(),
                lanes: LaneCount::new(LANES),
                settings,
                timing,
                objects,
                ext,
            })
            .boxed()
    }

    fn metadata() -> BoxedStrategy<Metadata> {
        let preview = (
            decimal::<AudioTime>(0..=MAX_MILLIONTHS),
            option::of(decimal::<Seconds>(1..=MAX_MILLIONTHS)),
        )
            .prop_map(|(start_seconds, length_seconds)| Preview {
                start_seconds,
                length_seconds,
            });
        ("\\PC{0,8}", "\\PC{0,8}", option::of(preview), ext())
            .prop_map(|(title, artist, preview, ext)| Metadata {
                title,
                artist,
                audio: ResourcePath::new("song.ogg").unwrap(),
                preview,
                ext,
            })
            .boxed()
    }

    /// Markers on distinct ticks, so sorted and one per tick and kind.
    fn markers() -> BoxedStrategy<Vec<Marker>> {
        let kind = prop_oneof![
            Just(MarkerKind::Section),
            Just(MarkerKind::Bookmark),
            key().prop_map(MarkerKind::Other),
        ];
        btree_map(-64_i32..=64, (kind, "\\PC{0,8}"), 0..4)
            .prop_map(|markers| {
                markers
                    .into_iter()
                    .map(|(tick, (kind, text))| Marker {
                        tick: Tick::new(tick),
                        kind,
                        text,
                    })
                    .collect()
            })
            .boxed()
    }

    /// A document that passes every rule, built so by construction: sorted objects on distinct
    /// ticks, lanes that fit, samples from the bank and every namespace the ruleset's.
    fn document() -> BoxedStrategy<Document> {
        (0_usize..3, 0_usize..3)
            .prop_flat_map(|(samples, charts)| {
                let charts: Vec<_> = (0..charts)
                    .map(|c| chart(format!("c{c}"), samples))
                    .collect();
                let resolution = prop_oneof![Just(1_u16), Just(48), Just(960), Just(3840)];
                (
                    resolution,
                    settings(),
                    metadata(),
                    timing(),
                    markers(),
                    charts,
                    Just(samples),
                )
            })
            .prop_map(
                |(resolution, settings, metadata, timing, markers, charts, samples)| Document {
                    format_version: FORMAT_VERSION,
                    ticks_per_beat: TicksPerBeat::new(resolution).unwrap(),
                    ruleset: Ruleset {
                        id: Namespace::new("test").unwrap(),
                        version: "1".into(),
                    },
                    settings,
                    plugins: BTreeMap::new(),
                    metadata,
                    samples: (0..samples)
                        .map(|s| {
                            let path = ResourcePath::new(format!("keysounds/s{s}.wav")).unwrap();
                            (SampleId::new(format!("s{s}")).unwrap(), path)
                        })
                        .collect(),
                    timing,
                    markers,
                    charts,
                },
            )
            .boxed()
    }

    proptest! {
        #![proptest_config(crate::proptest_config())]

        #[test]
        fn every_valid_document_reads_back_as_itself(document in document()) {
            let valid = ValidDocument::new(document).unwrap();
            let read = ValidDocument::from_json(&valid.to_json().unwrap()).unwrap();
            prop_assert_eq!(read, valid);
        }
    }

    #[test]
    fn a_valid_document_resolves_times_and_span_ends_without_failing() {
        let mut value = minimal();
        let hold = json!({ "tick": 960, "position": { "lane": { "index": 0 } }, "kind": "hold",
                           "shape": { "sustain": { "duration": 480 } },
                           "children": [ { "tick": 1200, "position": { "lane": { "index": 0 } },
                                           "kind": "tap", "shape": { "sustain": { "duration": 240 } } } ] });
        let own =
            json!({ "offset_seconds": 1.0, "events": [ { "tick": 0, "kind": { "bpm": 60.0 } } ] });
        value["charts"] = json!([{ "name": "Easy", "lanes": 4 }, { "name": "Hard", "lanes": 4 }]);
        value["charts"][0]["objects"] = json!([hold]);
        value["charts"][1]["timing"] = own;
        let valid = read(&value).unwrap();
        let seconds = |s| ResolvedTime::from(AudioTime::try_from(s).unwrap());
        let charts: Vec<_> = valid.charts().collect();
        assert_eq!(valid.timing().time_at(Tick::new(960)), seconds(0.5));
        assert_eq!(charts[0].timing().time_at(Tick::new(960)), seconds(0.5));
        assert_eq!(charts[1].timing().time_at(Tick::new(960)), seconds(2.0));
        let hold = charts[0].objects().next().unwrap();
        let child = hold.children().next().unwrap();
        assert_eq!(
            (hold.end(), child.end()),
            (Tick::new(1440), Tick::new(1440))
        );
    }
}
