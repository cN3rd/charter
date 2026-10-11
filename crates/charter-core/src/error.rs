use std::fmt::{self, Display};

use crate::document::{FORMAT_VERSION, SampleId};
use crate::ident::{LayerName, Namespace};
use crate::json::MAX_DOCUMENT_BYTES;
use crate::unit::{LaneCount, Tick, TimeSignature};

/// Why a document could not be read or written.
#[expect(
    clippy::error_impl_error,
    reason = "the crate's public error type is already named Error"
)]
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A document's JSON is over the byte limit charter reads.
    #[error("document is {0} bytes, over the {MAX_DOCUMENT_BYTES}-byte limit")]
    TooLarge(usize),
    /// The file uses a schema newer than this reader supports.
    #[error("format version {0} is newer than the supported {FORMAT_VERSION}")]
    UnsupportedVersion(u32),
    /// Input could not be decoded or written as JSON.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// A cross-field rule of the format was broken.
    #[error(transparent)]
    Invalid(#[from] Violation),
    /// A format 0 timing could not become anchors.
    #[error("a format 0 timing cannot become anchors: {0}")]
    Migration(#[from] MigrationFailure),
}

/// Why a format 0 timing could not become anchors.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum MigrationFailure {
    /// No BPM event gives the timing a tempo.
    #[error("it has no bpm event")]
    NoBpm,
    /// The BPM change on this tick resolves to no audio time.
    #[error("tick {0} has no time")]
    NoTime(Tick),
    /// The BPM change on this tick falls at a time an anchor cannot hold.
    #[error("tick {tick}: {why}")]
    OutOfRange {
        /// Tick of the BPM change.
        tick: Tick,
        /// The time it falls at, outside an anchor's range.
        why: InvalidValue,
    },
}

/// A place in a document, shown as a path such as `charts[1].objects[4]`. Indices count from 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Location {
    /// Song-level ruleset settings.
    Settings,
    /// Non-ruleset plugin declarations.
    Plugins,
    /// File-level sample bank.
    Samples,
    /// Plugin-owned song metadata.
    MetadataExt,
    /// The marker list as a whole.
    Markers,
    /// Marker by its index in the song's marker list.
    Marker(usize),
    /// A timing map.
    Timing(Stream),
    /// An event of a timing map.
    Event {
        /// Map containing the event.
        stream: Stream,
        /// Index in that map's events.
        index: usize,
    },
    /// Chart by index.
    Chart(usize),
    /// Layer by chart and row indices.
    Layer {
        /// Containing chart.
        chart: usize,
        /// Layer row.
        index: usize,
    },
    /// Ruleset settings of a chart by index.
    ChartSettings(usize),
    /// Plugin data of a chart by index.
    ChartExt(usize),
    /// A chart's object list as a whole.
    Objects(usize),
    /// An object by its chart and object indices.
    Object {
        /// Chart containing the object.
        chart: usize,
        /// Index in that chart's objects.
        index: usize,
    },
}

/// Which timing map a location lies in: the song's or a chart's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    /// The song's shared timing.
    Song,
    /// Timing of the chart at this index.
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
            Self::Layer { chart, index } => write!(f, "charts[{chart}].layers[{index}]"),
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
    /// Location of the first broken rule.
    pub at: Location,
    /// Rule broken at this location.
    pub rule: Rule,
}

/// The rules the format chapter lists for the validation pass.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Rule {
    /// Entries must appear in tick order.
    #[error("items are not sorted by tick")]
    Unsorted,
    /// Only one timing event of a given kind may occur on a tick.
    #[error("two timing events of one kind on tick {tick}")]
    DuplicateEvent {
        /// Tick containing the repeated event.
        tick: Tick,
    },
    /// Only one marker of a given kind may occur on a tick.
    #[error("two markers of one kind on tick {tick}")]
    DuplicateMarker {
        /// Tick containing the repeated marker.
        tick: Tick,
    },
    /// Timing requires at least one pinned beat.
    #[error("timing has no anchor")]
    NoAnchor,
    /// Pinned beats must progress in both ticks and audio time.
    #[error("anchors do not strictly increase in both ticks and seconds")]
    AnchorsNotIncreasing,
    /// A pinned beat cannot lie inside ticks that take no time.
    #[error("anchor on tick {tick} lies inside a warp")]
    AnchorInWarp {
        /// Pinned beat inside the warp.
        tick: Tick,
    },
    /// Pauses and warps leave insufficient time between anchors.
    #[error("the pauses and warps before the anchor on tick {tick} leave its beats no time")]
    BadAnchorSegment {
        /// Anchor ending the segment.
        tick: Tick,
    },
    /// A meter cannot divide this resolution into whole ticks.
    #[error("a {0} measure is not a whole number of ticks")]
    FractionalMeasure(TimeSignature),
    /// A span's end does not fit within the tick type.
    #[error("span ends past the last tick")]
    SpanPastLastTick,
    /// Warps may not cover the same ticks.
    #[error("warp overlaps the previous one")]
    OverlappingWarps,
    /// Two notes of shared kinds occupy the same lane and tick.
    #[error("two objects cover one lane on tick {tick}")]
    LaneTaken {
        /// Tick where the notes collide.
        tick: Tick,
    },
    /// Chart names must be unique within the file.
    #[error("chart name {0:?} is used twice")]
    DuplicateChartName(String),
    /// Layer names must be unique in a chart.
    #[error("layer \"{0}\" is used twice")]
    DuplicateLayer(LayerName),
    /// Every explicitly named object layer must exist in the chart.
    #[error("layer \"{0}\" is not in the chart")]
    UnknownLayer(LayerName),
    /// A note refers to a sample missing from the file's bank.
    #[error("sample \"{0}\" is not in the bank")]
    UnknownSample(SampleId),
    /// A lane span extends beyond the chart's layout.
    #[error("position covers a lane past the last of {lanes}")]
    LaneOutOfRange {
        /// Lane count of the containing chart.
        lanes: LaneCount,
    },
    /// Lane positions require a chart layout.
    #[error("lane position in a chart without lanes")]
    MissingLanes,
    /// Points of a path must progress in tick order.
    #[error("path ticks do not strictly increase")]
    PathNotIncreasing,
    /// A path cannot switch between lane and plane positions.
    #[error("path mixes lane and plane positions")]
    MixedPath,
    /// A child must fit inside its parent's span.
    #[error("child lies outside its parent's span")]
    ChildOutsideParent,
    /// Plugin data names an owner not declared by the document.
    #[error("namespace \"{0}\" is neither the ruleset nor a declared plugin")]
    UndeclaredNamespace(Namespace),
    /// A declared plugin left no data in the document.
    #[error("plugin \"{0}\" is declared but leaves no data")]
    UnusedPlugin(Namespace),
    /// The ruleset must not also appear as a non-ruleset plugin.
    #[error("ruleset \"{0}\" is also declared as a plugin")]
    RulesetInPlugins(Namespace),
}

/// A value outside its type's bounds, such as a zero BPM or an absolute path.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("expected {expected}, found {found}")]
pub struct InvalidValue {
    /// Grammar or range the rejected value should have met.
    expected: &'static str,
    /// Display form of the offending value.
    found: String,
}

impl InvalidValue {
    /// Captures `found` for an error naming the required grammar or range.
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
