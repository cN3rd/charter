use std::fmt::Display;

use crate::document::FORMAT_VERSION;
use crate::json::MAX_DOCUMENT_BYTES;
use crate::unit::{LaneCount, Tick, TimeSignature};

/// Why a document could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("document is {0} bytes, over the {MAX_DOCUMENT_BYTES}-byte limit")]
    TooLarge(usize),
    #[error("format version {0} is newer than the supported {FORMAT_VERSION}")]
    UnsupportedVersion(u32),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Invalid(#[from] Violation),
}

/// A broken rule relating values to each other, and where it broke.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{at}: {rule}")]
pub struct Violation {
    /// Location in the document, such as `charts[1].objects[4]`.
    pub at: String,
    pub rule: Rule,
}

/// The rules the format chapter lists for the validation pass.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Rule {
    #[error("items are not sorted by tick")]
    Unsorted,
    #[error("two timing events of one kind on tick {tick}")]
    DuplicateEvent { tick: Tick },
    #[error("timing has no bpm event")]
    NoBpm,
    #[error("a {0} measure is not a whole number of ticks")]
    FractionalMeasure(TimeSignature),
    #[error("span ends past the last tick")]
    SpanPastLastTick,
    #[error("warp overlaps the previous one")]
    OverlappingWarps,
    #[error("two objects cover one lane on tick {tick}")]
    LaneTaken { tick: Tick },
    #[error("chart name {0:?} is used twice")]
    DuplicateChartName(String),
    #[error("sample {0:?} is not in the bank")]
    UnknownSample(String),
    #[error("position covers a lane past the last of {lanes}")]
    LaneOutOfRange { lanes: LaneCount },
    #[error("lane position in a chart without lanes")]
    MissingLanes,
    #[error("path ticks do not strictly increase")]
    PathNotIncreasing,
    #[error("path mixes lane and plane positions")]
    MixedPath,
    #[error("child lies outside its parent's span")]
    ChildOutsideParent,
    #[error("namespace {0:?} is neither the ruleset nor a declared plugin")]
    UndeclaredNamespace(String),
    #[error("plugin {0:?} is declared but leaves no data")]
    UnusedPlugin(String),
    #[error("ruleset {0:?} is also declared as a plugin")]
    RulesetInPlugins(String),
}

/// A value outside its type's bounds, such as a zero BPM or an absolute path.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("expected {expected}, found {found}")]
pub struct InvalidValue {
    expected: &'static str,
    found: String,
}

impl InvalidValue {
    pub(crate) fn new(expected: &'static str, found: impl Display) -> Self {
        Self {
            expected,
            found: found.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_violation_reads_as_its_location_then_its_rule() {
        let violation = Violation {
            at: "charts[1].objects[4]".into(),
            rule: Rule::MissingLanes,
        };
        assert_eq!(
            violation.to_string(),
            format!("charts[1].objects[4]: {}", Rule::MissingLanes)
        );
    }

    #[test]
    fn an_invalid_value_names_what_was_expected_and_what_was_found() {
        let message = InvalidValue::new("a finite number > 0", -2.5).to_string();
        assert!(message.contains("a finite number > 0"), "{message}");
        assert!(message.contains("-2.5"), "{message}");
    }
}
