use std::collections::HashSet;
use std::fmt;

use serde::Deserialize;
use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};

use crate::document::{Document, FORMAT_VERSION};
use crate::error::Error;
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
    use serde_json::{Value, json};

    use super::*;
    use crate::error::{Rule, Violation};

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
}
