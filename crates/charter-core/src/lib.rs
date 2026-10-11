//! Charter's chart format: the data model from the book's format chapter, its JSON form, the
//! validation that turns bytes into a valid document, and [`RulesetPlugin`], which a ruleset
//! plugin implements so the editor can check documents against its game.

/// Chart data model and the format's default values.
mod document;
/// Locations and rules describing invalid documents.
mod error;
/// Measure boundaries and tick positions derived from meters.
mod grid;
/// Namespaced keys and shared vocabulary for plugin data.
mod ident;
/// JSON reading, writing and version migration.
mod json;
/// Exact six-place decimals and their textual representation.
mod number;
/// Relative resource paths and plain file names.
mod path;
mod plugin;
/// Converts audio-based tempo sections into anchored timing.
mod sections;
/// Resolves ticks to audio time using anchors and pauses.
mod timing;
mod unit;
/// Cross-field rules applied before a document becomes valid.
mod validate;

#[expect(
    clippy::pub_use,
    clippy::useless_attribute,
    reason = "the chart model is exported through the flat charter-core format API"
)]
pub use document::{
    Anchor, Chart, Child, CurrentFormat, Document, Extensions, FORMAT_VERSION, Layer, LayerColour,
    LayerView, Marker, MarkerKey, Metadata, Object, PathPoint, Plugin, Points, Position, Preview,
    Ruleset, SampleId, Settings, Shape, Timing, TimingEvent, TimingKind,
};
#[expect(
    clippy::pub_use,
    clippy::useless_attribute,
    reason = "format validation errors belong in the flat charter-core API"
)]
pub use error::{Error, InvalidValue, Location, MigrationFailure, Rule, Stream, Violation};
#[expect(
    clippy::pub_use,
    clippy::useless_attribute,
    reason = "measure and grid positions are part of the flat format API"
)]
pub use grid::{GridPosition, Measure, Measures};
#[expect(
    clippy::pub_use,
    clippy::useless_attribute,
    reason = "validated identifier types are part of the flat format API"
)]
pub use ident::{Ease, Key, Kind, LayerName, MarkerKind, Name, Namespace};
#[expect(
    clippy::pub_use,
    clippy::useless_attribute,
    reason = "validated documents are the flat format API's parse result"
)]
pub use json::{MAX_DOCUMENT_BYTES, Valid, ValidDocument};
#[expect(
    clippy::pub_use,
    clippy::useless_attribute,
    reason = "exact decimals are part of the flat format API"
)]
pub use number::Decimal;
#[expect(
    clippy::pub_use,
    clippy::useless_attribute,
    reason = "validated resource paths and file names are part of the flat format API"
)]
pub use path::{FileName, ResourcePath};
#[expect(
    clippy::pub_use,
    clippy::useless_attribute,
    reason = "ruleset plugin types form the flat format extension API"
)]
pub use plugin::{
    ChangeIndex, Difficulties, DifficultyPreset, ExportedSong, ImportedSong, ImportedTiming,
    Layout, NewChart, NewSong, NoteChange, RulesetPlugin, SettingScope, SongFormat, SongResource,
    TextSetting,
};
#[expect(
    clippy::pub_use,
    clippy::useless_attribute,
    reason = "tempo sections are exposed through the flat format API"
)]
pub use sections::{Section, SectionError, Sectioned, sectioned};

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
#[expect(
    clippy::pub_use,
    clippy::useless_attribute,
    reason = "unit-safe timing and lane types are exposed through the flat format API"
)]
pub use unit::{
    AudioTime, BeatLength, Bpm, LaneCount, LaneIndex, LaneOffset, LaneSpan, NoteValue, Origin,
    ResolvedTime, Seconds, Tick, TickOffset, Ticks, TicksPerBeat, TimeShift, TimeSignature,
};
