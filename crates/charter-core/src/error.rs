use std::fmt::{self, Display};

use crate::document::{FORMAT_VERSION, SampleId};
use crate::ident::Namespace;
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

/// A place in a document, shown as a path such as `charts[1].objects[4]`. Indices count from 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Location {
    Settings,
    Plugins,
    Samples,
    MetadataExt,
    /// The marker list as a whole.
    Markers,
    Marker(usize),
    /// A timing map.
    Timing(Stream),
    /// An event of a timing map.
    Event {
        stream: Stream,
        index: usize,
    },
    Chart(usize),
    ChartSettings(usize),
    ChartExt(usize),
    /// A chart's object list as a whole.
    Objects(usize),
    Object {
        chart: usize,
        index: usize,
    },
}

/// Which timing map a location lies in: the song's or a chart's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    Song,
    Chart(usize),
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Settings => f.write_str("settings"),
            Self::Plugins => f.write_str("plugins"),
            Self::Samples => f.write_str("samples"),
            Self::MetadataExt => f.write_str("metadata.ext"),
            Self::Markers => f.write_str("markers"),
            Self::Marker(m) => write!(f, "markers[{m}]"),
            Self::Timing(Stream::Song) => f.write_str("timing"),
            Self::Timing(Stream::Chart(c)) => write!(f, "charts[{c}].timing"),
            Self::Event { stream, index } => write!(f, "{}.events[{index}]", Self::Timing(stream)),
            Self::Chart(c) => write!(f, "charts[{c}]"),
            Self::ChartSettings(c) => write!(f, "charts[{c}].settings"),
            Self::ChartExt(c) => write!(f, "charts[{c}].ext"),
            Self::Objects(c) => write!(f, "charts[{c}].objects"),
            Self::Object { chart, index } => write!(f, "charts[{chart}].objects[{index}]"),
        }
    }
}

/// A broken rule relating values to each other, and where it broke.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{at}: {rule}")]
pub struct Violation {
    pub at: Location,
    pub rule: Rule,
}

/// The rules the format chapter lists for the validation pass.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Rule {
    #[error("items are not sorted by tick")]
    Unsorted,
    #[error("two timing events of one kind on tick {tick}")]
    DuplicateEvent { tick: Tick },
    #[error("two markers of one kind on tick {tick}")]
    DuplicateMarker { tick: Tick },
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
    #[error("sample \"{0}\" is not in the bank")]
    UnknownSample(SampleId),
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
    #[error("namespace \"{0}\" is neither the ruleset nor a declared plugin")]
    UndeclaredNamespace(Namespace),
    #[error("plugin \"{0}\" is declared but leaves no data")]
    UnusedPlugin(Namespace),
    #[error("ruleset \"{0}\" is also declared as a plugin")]
    RulesetInPlugins(Namespace),
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
            at: Location::Object { chart: 1, index: 4 },
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

    #[test]
    fn locations_show_as_document_paths() {
        let shown = [
            (Location::Settings, "settings"),
            (Location::Plugins, "plugins"),
            (Location::Samples, "samples"),
            (Location::MetadataExt, "metadata.ext"),
            (Location::Markers, "markers"),
            (Location::Marker(2), "markers[2]"),
            (Location::Timing(Stream::Song), "timing"),
            (Location::Timing(Stream::Chart(2)), "charts[2].timing"),
            (
                Location::Event {
                    stream: Stream::Song,
                    index: 3,
                },
                "timing.events[3]",
            ),
            (
                Location::Event {
                    stream: Stream::Chart(2),
                    index: 3,
                },
                "charts[2].timing.events[3]",
            ),
            (Location::Chart(1), "charts[1]"),
            (Location::ChartSettings(1), "charts[1].settings"),
            (Location::ChartExt(1), "charts[1].ext"),
            (Location::Objects(1), "charts[1].objects"),
            (
                Location::Object { chart: 1, index: 4 },
                "charts[1].objects[4]",
            ),
        ];
        for (location, path) in shown {
            assert_eq!(location.to_string(), path);
        }
    }
}
