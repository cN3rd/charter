use std::collections::HashSet;
use std::fmt;

use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};

use crate::document::{
    Anchor, Chart, Child, Document, Extensions, FORMAT_VERSION, Object, Shape, Timing, TimingEvent,
    TimingKind,
};
use crate::error::Error;
use crate::grid::{GridPosition, Measures};
use crate::unit::{Bpm, Origin, ResolvedTime, Seconds, Tick, Ticks, TicksPerBeat, TimeSignature};
use crate::validate::validate;

/// Largest document a reader accepts.
pub const MAX_DOCUMENT_BYTES: usize = 64 << 20;

/// A [`Document`] that passed every rule of the format.
#[derive(Clone, Debug, PartialEq, Eq)]
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
        let value = serde_json::from_slice(bytes)?;
        let document = if format_version == 0 {
            migrate_v0(value)?
        } else {
            value
        };
        Self::new(serde_json::from_value(document)?)
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

    /// The validated raw document, without losing its ownership.
    #[must_use]
    pub const fn document(&self) -> &Document {
        &self.0
    }

    /// The song's timing map.
    #[must_use]
    pub const fn timing(&self) -> Valid<'_, Timing> {
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
    /// Validated chart part this view dereferences to.
    part: &'a T,
    /// Document whose cross-field rules have already been checked.
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

    /// Objects in this chart, in tick order.
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

    /// Children of this object, in tick order.
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
    #[expect(
        clippy::expect_used,
        reason = "validation guarantees a time for every tick"
    )]
    pub fn time_at(&self, tick: Tick) -> ResolvedTime {
        self.part
            .seconds_at(self.document.ticks_per_beat, tick)
            .expect("a valid timing map resolves every tick")
    }

    /// The tempo a single `bpm` event on `from`, in place of any others before `to`, needs for
    /// `to` to fall `seconds` after `from`, to six places, as when syncing a beat to the audio.
    /// Stops, delays and warps keep their lengths. `None` when `to` is not after `from`, or the
    /// pauses in between already take `seconds` or longer.
    #[must_use]
    pub fn tempo_for(&self, from: Tick, to: Tick, seconds: f64) -> Option<Bpm> {
        self.part
            .tempo_for(self.document.ticks_per_beat, from, to, seconds)
    }

    /// The tempo that governs `tick`, with anchored segments derived from their end points.
    ///
    /// # Panics
    ///
    /// Never: validation guarantees every anchored segment derives a tempo.
    #[must_use]
    #[expect(
        clippy::expect_used,
        reason = "validation guarantees every segment derives a tempo"
    )]
    pub fn tempo_at(&self, tick: Tick) -> f64 {
        self.part
            .tempo_at(self.document.ticks_per_beat, tick)
            .expect("a valid timing map derives every segment tempo")
    }

    /// The tempo at `tick` as it is shown and typed, to the fewest places that keep the next
    /// anchor on its tick; none for a rate six places cannot hold as a tempo.
    ///
    /// # Panics
    ///
    /// Never: validation guarantees every anchored segment derives a tempo.
    #[must_use]
    pub fn bpm_at(&self, tick: Tick) -> Option<Bpm> {
        let tempo = self.part.bpm_at(self.document.ticks_per_beat, tick)?;
        Bpm::nearest(tempo)
    }

    /// One tempo per anchor: each inner anchor's following segment, and the last anchor's tail.
    ///
    /// # Panics
    ///
    /// Never: validation guarantees every anchored segment derives a tempo.
    #[must_use]
    #[expect(
        clippy::expect_used,
        reason = "validation guarantees every segment derives a tempo"
    )]
    pub fn tempos(&self) -> Vec<(Tick, f64)> {
        self.part
            .tempos(self.document.ticks_per_beat)
            .expect("a valid timing map derives every segment tempo")
    }

    /// How many ticks the document counts to a beat, which a grid over this timing steps by.
    #[must_use]
    pub const fn ticks_per_beat(&self) -> TicksPerBeat {
        self.document.ticks_per_beat
    }

    /// The last tick an object on which is hit at or before `seconds`, the inverse of
    /// [`Valid::time_at`] where tick to time rises and the earliest tick of a stretch it does
    /// not. Tick to time never decreases, so a binary search over every tick finds it without
    /// inverting the timing map.
    #[must_use]
    pub fn latest_tick_by(&self, seconds: f64) -> Tick {
        let (mut early, mut late) = (i64::from(i32::MIN), i64::from(i32::MAX));
        let tick = |at: i64| Tick::new(i32::try_from(at).unwrap_or(i32::MAX));
        if self.time_at(tick(late)).seconds() <= seconds {
            return tick(late);
        }
        while late.saturating_sub(early) > 1 {
            let middle = early.saturating_add(late.saturating_sub(early).div_euclid(2));
            if self.time_at(tick(middle)).seconds() <= seconds {
                early = middle;
            } else {
                late = middle;
            }
        }
        tick(early)
    }

    /// The measures of the grid from the one holding `from` onward. Common time rules from tick
    /// 0 until the first time signature, and each time signature restarts the grid at its tick.
    ///
    /// # Panics
    ///
    /// Never: validation guarantees every measure is a whole number of ticks.
    #[must_use]
    #[expect(clippy::expect_used, reason = "validation guarantees whole measures")]
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
    #[expect(clippy::expect_used, reason = "validation guarantees whole measures")]
    pub fn grid_position(&self, tick: Tick) -> GridPosition {
        self.part
            .grid_position(self.document.ticks_per_beat, tick)
            .expect("a valid timing map has whole measures")
    }
}

#[expect(
    clippy::expect_used,
    reason = "validation guarantees every span ends on a tick"
)]
/// End tick of a validated span, whose endpoint cannot overflow.
fn span_end(tick: Tick, shape: &Shape) -> Tick {
    shape.end(tick).expect("a valid span ends on a tick")
}

/// Upgrades format 0 song and chart timing to anchored format 1 timing.
fn migrate_v0(mut value: serde_json::Value) -> Result<serde_json::Value, Error> {
    let ticks_per_beat = value
        .get("ticks_per_beat")
        .cloned()
        .map(serde_json::from_value)
        .transpose()?
        .unwrap_or_default();
    let Some(object) = value.as_object_mut() else {
        return Ok(value);
    };
    object.insert(
        "format_version".to_owned(),
        serde_json::Value::from(FORMAT_VERSION),
    );
    if let Some(timing) = object.get_mut("timing") {
        migrate_timing_value(timing, ticks_per_beat)?;
    }
    if let Some(charts) = object
        .get_mut("charts")
        .and_then(serde_json::Value::as_array_mut)
    {
        for chart in charts {
            if let Some(timing) = chart.get_mut("timing") {
                migrate_timing_value(timing, ticks_per_beat)?;
            }
        }
    }
    Ok(value)
}

/// Replaces one format 0 timing value with an anchored timing map.
fn migrate_timing_value(
    value: &mut serde_json::Value,
    ticks_per_beat: TicksPerBeat,
) -> Result<(), Error> {
    let old: V0Timing = serde_json::from_value(value.take())?;
    let new = old.migrate(ticks_per_beat).map_err(|why| {
        <serde_json::Error as serde::de::Error>::custom(format!(
            "a format 0 timing cannot become anchors: {why}"
        ))
    })?;
    *value = serde_json::to_value(new)?;
    Ok(())
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Format 0 timing, with a global offset and BPM changes among its events.
struct V0Timing {
    /// Audio time of tick zero in seconds.
    offset_seconds: Origin,
    /// Format 0 events, including the tempo changes that become anchors.
    events: Vec<V0TimingEvent>,
}

/// An event in a format 0 timing map.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct V0TimingEvent {
    /// Tick at which the old event applies.
    tick: Tick,
    /// Tempo, shared timing or plugin-specific event at this tick.
    kind: V0TimingKind,
    /// Opaque plugin data retained on non-tempo events.
    #[serde(default)]
    ext: Extensions,
}

/// Timing event kinds accepted in format 0 files before conversion to anchors.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum V0TimingKind {
    /// Tempo change that becomes an anchor at its resolved audio time.
    Bpm(Bpm),
    /// Pause after the objects at this tick.
    Stop(Seconds),
    /// Pause before the objects at this tick.
    Delay(Seconds),
    /// Ticks skipped without taking time.
    Warp(Ticks),
    /// Meter in effect from this tick.
    TimeSignature(TimeSignature),
    /// Opaque timing event belonging to a plugin.
    Other {
        /// Namespaced identifier of the event.
        name: crate::Key,
        /// Plugin-specific event payload.
        value: serde_json::Value,
    },
}

impl V0Timing {
    /// The timing with an anchor on every BPM change's tick at the time it resolved to, to the
    /// microsecond, and the last BPM as the tempo after them. Refuses a BPM change whose time
    /// does not resolve, or does not fit a time, rather than leaving its anchor out.
    fn migrate(&self, ticks_per_beat: TicksPerBeat) -> Result<Timing, String> {
        let mut tempo = None;
        let mut anchors = Vec::new();
        let mut events = Vec::with_capacity(self.events.len());
        for event in &self.events {
            match &event.kind {
                V0TimingKind::Bpm(bpm) => {
                    tempo = Some(*bpm);
                    let seconds = self
                        .seconds_at(ticks_per_beat, event.tick)
                        .ok_or_else(|| format!("tick {} has no time", event.tick))?;
                    let seconds = Origin::try_from((seconds * 1e6).round() / 1e6)
                        .map_err(|why| format!("tick {}: {why}", event.tick))?;
                    anchors.push(Anchor {
                        tick: event.tick,
                        seconds,
                    });
                }
                kind @ (V0TimingKind::Stop(_)
                | V0TimingKind::Delay(_)
                | V0TimingKind::Warp(_)
                | V0TimingKind::TimeSignature(_)
                | V0TimingKind::Other { .. }) => events.push(TimingEvent {
                    tick: event.tick,
                    kind: kind.clone().into(),
                    ext: event.ext.clone(),
                }),
            }
        }
        let tempo = tempo.ok_or("it has no bpm event")?;
        Ok(Timing {
            anchors,
            tempo,
            events,
        })
    }

    /// Audio time of `tick`, using the old offset and tempo events.
    fn seconds_at(&self, ticks_per_beat: TicksPerBeat, tick: Tick) -> Option<f64> {
        let offset = ResolvedTime::from(self.offset_seconds).seconds();
        let zero = Tick::new(0);
        Some(if tick >= zero {
            offset + self.elapsed(ticks_per_beat, zero, tick)?
        } else {
            offset - self.elapsed(ticks_per_beat, tick, zero)?
        })
    }

    /// Seconds from `from` to `to` under the old tempo, pauses and warps.
    fn elapsed(&self, ticks_per_beat: TicksPerBeat, from: Tick, to: Tick) -> Option<f64> {
        let timed = |cursor: Tick, until: Tick, warp_end: Option<Tick>, bpm: Bpm| {
            let start = warp_end.map_or(cursor, |end| end.max(cursor));
            start
                .ticks_until(until)
                .map_or(0.0, |ticks| bpm.seconds(ticks, ticks_per_beat))
        };
        let mut bpm = self.events.iter().find_map(|event| match event.kind {
            V0TimingKind::Bpm(value) => Some(value),
            V0TimingKind::Stop(_)
            | V0TimingKind::Delay(_)
            | V0TimingKind::Warp(_)
            | V0TimingKind::TimeSignature(_)
            | V0TimingKind::Other { .. } => None,
        })?;
        let mut elapsed = 0.0;
        let mut cursor = from;
        let mut warp_end = None;
        for event in &self.events {
            let at = event.tick;
            if at > to {
                break;
            }
            if at > from {
                elapsed += timed(cursor, at, warp_end, bpm);
                cursor = at;
            }
            match event.kind {
                V0TimingKind::Bpm(value) => bpm = value,
                V0TimingKind::Stop(pause) if at >= from && at < to => elapsed += pause.seconds(),
                V0TimingKind::Delay(pause) if at > from => elapsed += pause.seconds(),
                V0TimingKind::Warp(ticks) => warp_end = Some(at.checked_add(ticks)?),
                V0TimingKind::Stop(_)
                | V0TimingKind::Delay(_)
                | V0TimingKind::TimeSignature(..)
                | V0TimingKind::Other { .. } => {}
            }
        }
        Some(elapsed + timed(cursor, to, warp_end, bpm))
    }
}

impl From<V0TimingKind> for TimingKind {
    #[expect(
        clippy::unreachable,
        reason = "BPM variants become anchors before conversion"
    )]
    fn from(kind: V0TimingKind) -> Self {
        match kind {
            V0TimingKind::Bpm(_) => unreachable!(),
            V0TimingKind::Stop(pause) => Self::Stop(pause),
            V0TimingKind::Delay(pause) => Self::Delay(pause),
            V0TimingKind::Warp(ticks) => Self::Warp(ticks),
            V0TimingKind::TimeSignature(meter) => Self::TimeSignature(meter),
            V0TimingKind::Other { name, value } => Self::Other { name, value },
        }
    }
}

/// Reads the version before anything else, so a newer file fails on its version rather than on
/// the first key this reader does not know.
#[derive(Deserialize)]
struct VersionProbe {
    /// Version checked before decoding any fields newer than this reader knows.
    format_version: u32,
}

/// Largest integer magnitude I-JSON allows, `2^53 - 1`: every integer up to it is a double.
const MAX_EXACT_INTEGER: u64 = (1 << 53) - 1;

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "2^53 - 1 is exactly representable"
)]
/// Largest I-JSON exact integer expressed as a double.
fn exact_integer_limit_as_f64() -> f64 {
    MAX_EXACT_INTEGER as f64
}

/// Walks any JSON value and fails where it breaks I-JSON: on a repeated key, which `serde_json`
/// would resolve by keeping the last, and on an integer a double cannot hold exactly.
struct IJson;

impl<'de> Deserialize<'de> for IJson {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(IJsonVisitor)
    }
}

/// Checks integer precision and duplicate map keys while walking arbitrary JSON.
struct IJsonVisitor;

impl<'de> Visitor<'de> for IJsonVisitor {
    type Value = IJson;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any JSON value")
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

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
        if v.fract() == 0.0 && v.abs() > exact_integer_limit_as_f64() {
            return Err(inexact(v));
        }
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

/// Deserialization error for an integer outside the exact double range.
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
        Anchor, Chart, Child, Extensions, Marker, Metadata, Object, PathPoint, Points, Position,
        Preview, Ruleset, SampleId, Settings, Shape, Timing, TimingEvent, TimingKind,
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
            "format_version": 1,
            "ruleset": { "id": "test", "version": "1" },
            "metadata": { "title": "t", "artist": "a", "audio": "song.ogg" },
            "timing": { "anchors": [ { "tick": 0, "seconds": 0.0 } ], "tempo": 120.0, "events": [] }
        })
    }

    fn read(value: &Value) -> Result<ValidDocument, Error> {
        ValidDocument::from_json(&serde_json::to_vec(value)?)
    }

    /// Reads `value` and returns what the writer makes of it.
    fn round_trip(value: &Value) -> Value {
        serde_json::from_slice(&read(value).unwrap().to_json().unwrap()).unwrap()
    }

    /// The format chapter's full example, embedded at compile time so the test does no I/O.
    const SPEC: &str = include_str!("../../../docs/src/format.md");

    #[expect(
        clippy::string_slice,
        reason = "substring searches return UTF-8 byte boundaries"
    )]
    fn spec_example() -> Value {
        let start = SPEC
            .rfind("```json\n")
            .unwrap()
            .saturating_add("```json\n".len());
        let end = start.saturating_add(SPEC[start..].find("```").unwrap());
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

    fn v0(timing: &serde_json::Value) -> Vec<u8> {
        let value = serde_json::json!({
            "format_version": 0,
            "ruleset": { "id": "t", "version": "1" },
            "metadata": { "title": "t", "artist": "a", "audio": "a.ogg" },
            "timing": timing,
            "charts": [ { "name": "Easy", "timing": timing } ]
        });
        serde_json::to_vec(&value).unwrap()
    }

    #[test]
    fn a_format_0_map_becomes_anchors_that_keep_every_ticks_time() {
        let timing = serde_json::json!({
            "offset_seconds": -0.014,
            "events": [
                { "tick": 0, "kind": { "bpm": 174.0 } },
                { "tick": 960, "kind": { "stop": 0.25 } },
                { "tick": 1920, "kind": { "bpm": 87.0 } },
                { "tick": 3840, "kind": { "time_signature": [3, 4] } }
            ]
        });
        let document = ValidDocument::from_json(&v0(&timing)).unwrap();
        assert_eq!(document.document().format_version, FORMAT_VERSION);
        let migrated = document.timing();
        let ticks: Vec<Tick> = migrated.anchors.iter().map(|anchor| anchor.tick).collect();
        assert_eq!(ticks, [Tick::new(0), Tick::new(1920)]);
        assert_eq!(migrated.tempo, Bpm::try_from(87.0).unwrap());
        assert_eq!(migrated.events.len(), 2, "the stop and the meter stay");
        // At 174 BPM a beat is 60/174 s; a quarter of a second stops after beat 1.
        let beat = 60.0 / 174.0;
        let expected = [
            (0, -0.014),
            (960, -0.014 + beat),
            (1920, -0.014 + 2.0 * beat + 0.25),
            (2880, -0.014 + 2.0 * beat + 0.25 + 60.0 / 87.0),
        ];
        for (tick, seconds) in expected {
            let at = migrated.time_at(Tick::new(tick)).seconds();
            assert!(
                (at - seconds).abs() < 1e-6,
                "tick {tick}: {at} vs {seconds}"
            );
        }
        let chart = document.charts().next().unwrap().timing();
        assert_eq!(chart.anchors.len(), 2);
        let written = String::from_utf8(document.to_json().unwrap()).unwrap();
        assert!(!written.contains("offset_seconds") && !written.contains("\"bpm\""));
        assert_eq!(
            ValidDocument::from_json(written.as_bytes()).unwrap(),
            document
        );
    }

    #[test]
    fn a_format_0_map_without_a_bpm_is_refused_rather_than_guessed() {
        let timing = serde_json::json!({ "offset_seconds": 0.0, "events": [] });
        let refused = ValidDocument::from_json(&v0(&timing)).unwrap_err();
        assert!(refused.to_string().contains("no bpm event"), "{refused}");
    }

    #[test]
    fn a_format_0_warp_past_the_last_tick_is_refused_during_migration() {
        let timing = json!({
            "offset_seconds": 0.0,
            "events": [
                { "tick": 0, "kind": { "bpm": 120.0 } },
                { "tick": 1, "kind": { "warp": 4_294_967_295_u64 } },
                { "tick": 2, "kind": { "bpm": 120.0 } }
            ]
        });
        let refused = ValidDocument::from_json(&v0(&timing)).unwrap_err();
        assert!(refused.to_string().contains("no time"), "{refused}");
    }

    #[test]
    fn a_format_0_warp_skips_ticks_without_moving_the_next_tempo_change_in_audio() {
        let timing = json!({
            "offset_seconds": 0.0,
            "events": [
                { "tick": 0, "kind": { "bpm": 120.0 } },
                { "tick": 480, "kind": { "warp": 960 } },
                { "tick": 1920, "kind": { "bpm": 60.0 } }
            ]
        });
        let migrated = ValidDocument::from_json(&v0(&timing)).unwrap();
        assert_eq!(
            migrated.timing().anchors[1].seconds,
            Origin::try_from(0.5).unwrap()
        );
    }

    #[test]
    fn a_format_0_tempo_change_whose_audio_time_cannot_fit_is_refused() {
        let timing = json!({
            "offset_seconds": 0.0,
            "events": [
                { "tick": 0, "kind": { "bpm": 0.000_001 } },
                { "tick": 2_147_483_647, "kind": { "bpm": 120.0 } }
            ]
        });
        let refused = ValidDocument::from_json(&v0(&timing)).unwrap_err();
        assert!(refused.to_string().contains("tick 2147483647"), "{refused}");
    }

    #[test]
    fn a_format_0_song_without_chart_timing_migrates_only_its_song_timing() {
        let mut value = minimal();
        value["format_version"] = json!(0);
        value["timing"] = json!({
            "offset_seconds": 0.25,
            "events": [{ "tick": 0, "kind": { "bpm": 120.0 } }]
        });
        value["charts"] = json!([{ "name": "Easy", "lanes": 4 }]);
        let migrated = read(&value).unwrap();
        assert!((migrated.timing().time_at(Tick::new(960)).seconds() - 0.75).abs() < 1e-9);
        assert_eq!(migrated.charts().next().unwrap().timing().anchors.len(), 1);
        let mut without_charts = value;
        without_charts.as_object_mut().unwrap().remove("charts");
        assert_eq!(read(&without_charts).unwrap().document().charts.len(), 0);
    }

    #[test]
    fn a_format_0_bpm_before_tick_zero_migrates_to_a_negative_tick_anchor() {
        let timing = json!({
            "offset_seconds": 1.0,
            "events": [
                { "tick": -960, "kind": { "bpm": 120.0 } },
                { "tick": 0, "kind": { "bpm": 60.0 } }
            ]
        });
        let migrated = ValidDocument::from_json(&v0(&timing)).unwrap();
        let anchors = &migrated.timing().anchors;
        assert_eq!(anchors[0].tick, Tick::new(-960));
        assert_eq!(anchors[0].seconds, Origin::try_from(0.5).unwrap());
        assert!((migrated.timing().time_at(Tick::new(960)).seconds() - 2.0).abs() < 1e-9);
    }

    #[test]
    fn a_format_0_delay_and_stop_keep_their_distinct_tick_boundaries() {
        let timing = json!({
            "offset_seconds": 0.0,
            "events": [
                { "tick": 0, "kind": { "bpm": 120.0 } },
                { "tick": 960, "kind": { "stop": 0.25 } },
                { "tick": 1920, "kind": { "delay": 0.5 } },
                { "tick": 2880, "kind": { "bpm": 60.0 } }
            ]
        });
        let migrated = ValidDocument::from_json(&v0(&timing)).unwrap();
        assert_eq!(
            migrated.timing().anchors[1].seconds,
            Origin::try_from(2.25).unwrap()
        );
        assert_eq!(migrated.timing().events.len(), 2);
        assert_eq!(
            ValidDocument::from_json(&migrated.to_json().unwrap()).unwrap(),
            migrated
        );
    }

    #[test]
    fn newer_versions_are_refused_before_their_keys_are_read() {
        let mut value = minimal();
        value["format_version"] = json!(2);
        value["from_the_future"] = json!(true);
        assert!(matches!(read(&value), Err(Error::UnsupportedVersion(2))));
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
        value["timing"]["anchors"] = json!([]);
        assert!(matches!(
            read(&value),
            Err(Error::Invalid(Violation {
                rule: Rule::NoAnchor,
                ..
            }))
        ));
    }

    #[test]
    fn documents_built_in_code_are_validated_too() {
        let mut document: Document = serde_json::from_value(minimal()).unwrap();
        document.timing.anchors.clear();
        assert!(matches!(
            ValidDocument::new(document),
            Err(Error::Invalid(Violation {
                rule: Rule::NoAnchor,
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

    #[test]
    fn an_out_of_range_integer_spelled_as_a_float_is_rejected() {
        let bytes = br#"{"format_version":1,"ruleset":{"id":"test","version":"1"},"settings":{"n":9007199254740993.0},"metadata":{"title":"t","artist":"a","audio":"song.ogg"},"timing":{"anchors":[{"tick":0,"seconds":0}],"tempo":120,"events":[]}}"#;
        assert!(matches!(
            ValidDocument::from_json(bytes),
            Err(Error::Json(_))
        ));
    }

    #[test]
    fn excessive_nesting_is_refused_without_recursing_forever() {
        let mut document = minimal();
        document["settings"] = json!({ "nest": 0 });
        read(&document).unwrap();
        let shallow = serde_json::to_string(&document).unwrap();
        let nested = format!("{}0{}", "[".repeat(129), "]".repeat(129));
        let text = shallow.replace(r#""nest":0"#, &format!(r#""nest":{nested}"#));
        assert_ne!(text, shallow);
        assert!(matches!(
            ValidDocument::from_json(text.as_bytes()),
            Err(Error::Json(_))
        ));
    }

    #[test]
    fn a_valid_map_resolves_before_its_first_anchor_and_at_its_last_tick() {
        let mut value = minimal();
        value["ticks_per_beat"] = json!(480);
        value["timing"] = json!({
            "anchors": [
                { "tick": 480, "seconds": 1.0 },
                { "tick": 960, "seconds": 1.5 }
            ],
            "tempo": 60.0, "events": []
        });
        let doc = read(&value).unwrap();
        let timing = doc.timing();
        assert!((timing.time_at(Tick::new(0)).seconds() - 0.5).abs() < 1e-10);
        assert!((timing.tempo_at(Tick::new(0)) - 120.0).abs() < 1e-10);
        assert_eq!(timing.bpm_at(Tick::new(480)), Bpm::try_from(120.0).ok());
        assert_eq!(timing.bpm_at(Tick::new(960)), Bpm::try_from(60.0).ok());
        assert_eq!(
            timing.tempos(),
            vec![(Tick::new(480), 120.0), (Tick::new(960), 60.0)]
        );
        assert!((timing.time_at(Tick::new(1440)).seconds() - 2.5).abs() < 1e-10);
        assert_eq!(timing.latest_tick_by(1.5), Tick::new(960));
    }

    #[test]
    #[expect(
        clippy::clone_on_copy,
        reason = "the explicit Clone impl is a coverage target"
    )]
    fn a_valid_timing_view_can_be_copied_and_derives_tempo_for_a_synchronised_beat() {
        let doc = read(&minimal()).unwrap();
        let timing = doc.timing();
        let copy = timing.clone();
        assert_eq!(copy.anchors, timing.anchors);
        assert_eq!(
            copy.tempo_for(Tick::new(0), Tick::new(960), 1.0),
            Bpm::try_from(60.0).ok()
        );
        assert_eq!(copy.tempo_for(Tick::new(960), Tick::new(0), 1.0), None);
    }

    proptest! {
        #![proptest_config(crate::proptest_config())]

        #[test]
        fn arbitrary_short_json_never_panics(the_bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
            drop(ValidDocument::from_json(&the_bytes));
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
            (exact.saturating_neg()..=exact).prop_map(Value::from),
            any::<f64>()
                .prop_filter("finite I-JSON float", |f| {
                    let largest_safe = exact_integer_limit_as_f64();
                    f.is_finite() && (f.fract() != 0.0 || f.abs() <= largest_safe)
                })
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
                .prop_flat_map(|index| (Just(index), 1..=LANES.saturating_sub(index)))
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
                            tick: Tick::new(tick.saturating_add(100_i32.saturating_mul(j))),
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
            .prop_map(
                move |(object_position, object_kind, shape, sample, children, object_ext)| {
                    let start = Tick::new(tick);
                    let end = shape.end(start).unwrap();
                    let children = children
                        .into_iter()
                        .map(|(offset, position, kind, ext)| Child {
                            tick: Tick::new(tick.saturating_add(offset)).min(end),
                            position,
                            kind,
                            shape: Shape::Instant,
                            sample: None,
                            ext,
                        })
                        .collect();
                    Object {
                        tick: start,
                        position: object_position,
                        kind: object_kind,
                        shape,
                        sample,
                        children,
                        ext: object_ext,
                    }
                },
            )
            .boxed()
    }

    /// A timing map with an anchor on tick 0 and one event on each of a few other ticks, spaced
    /// so no warp overlaps the next and every meter is whole at any resolution.
    fn timing() -> BoxedStrategy<Timing> {
        let event = prop_oneof![
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
            .prop_map(|(seconds, tempo, events)| {
                let events = events
                    .into_iter()
                    .map(|(k, (kind, ext))| TimingEvent {
                        tick: Tick::new(16_i32.saturating_mul(k)),
                        kind,
                        ext,
                    })
                    .collect();
                Timing {
                    anchors: vec![Anchor {
                        tick: Tick::new(0),
                        seconds,
                    }],
                    tempo,
                    events,
                }
            })
            .boxed()
    }

    fn chart(name: String, samples: usize) -> BoxedStrategy<Chart> {
        (0_i32..4)
            .prop_flat_map(move |count| {
                let objects: Vec<_> = (0..count)
                    .map(|i| object(1024_i32.saturating_mul(i), samples))
                    .collect();
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
                artwork: None,
                genre: None,
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
                    .map(|(tick, (marker_kind, text))| Marker {
                        tick: Tick::new(tick),
                        kind: marker_kind,
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
        let own = json!({
            "anchors": [ { "tick": 0, "seconds": 1.0 } ],
            "tempo": 60.0,
            "events": []
        });
        value["charts"] = json!([{ "name": "Easy", "lanes": 4 }, { "name": "Hard", "lanes": 4 }]);
        value["charts"][0]["objects"] = json!([hold]);
        value["charts"][1]["timing"] = own;
        let valid = read(&value).unwrap();
        let seconds = |s| ResolvedTime::from(AudioTime::try_from(s).unwrap());
        let charts: Vec<_> = valid.charts().collect();
        assert_eq!(valid.timing().time_at(Tick::new(960)), seconds(0.5));
        assert_eq!(charts[0].timing().time_at(Tick::new(960)), seconds(0.5));
        assert_eq!(charts[1].timing().time_at(Tick::new(960)), seconds(2.0));
        let hold_object = charts[0].objects().next().unwrap();
        let child = hold_object.children().next().unwrap();
        assert_eq!(
            (hold_object.end(), child.end()),
            (Tick::new(1440), Tick::new(1440))
        );
    }
}
