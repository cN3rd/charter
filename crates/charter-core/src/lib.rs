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
/// Relative resource paths safe for chart audio and assets.
mod path;
mod plugin;
/// Converts audio-based tempo sections into anchored timing.
mod sections;
/// Resolves ticks to audio time using anchors and pauses.
mod timing;
mod unit;
/// Cross-field rules applied before a document becomes valid.
mod validate;

pub use document::{
    Anchor, Chart, Child, DEFAULT_TICKS_PER_BEAT, Document, Extensions, FORMAT_VERSION, Marker,
    Metadata, Object, PathPoint, Plugin, Points, Position, Preview, Ruleset, SampleId, Settings,
    Shape, Timing, TimingEvent, TimingKind,
};
pub use error::{Error, InvalidValue, Location, Rule, Stream, Violation};
pub use grid::{GridPosition, Measure, Measures};
pub use ident::{Ease, Key, Kind, MarkerKind, Name, Namespace};
pub use json::{MAX_DOCUMENT_BYTES, Valid, ValidDocument};
pub use number::Decimal;
pub use path::ResourcePath;
pub use plugin::{
    Difficulties, DifficultyPreset, ImportedTiming, Layout, NewChart, NewSong, NoteChange,
    RulesetPlugin, SettingScope, TextSetting,
};
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
pub use unit::{
    AudioTime, Bpm, LaneCount, LaneIndex, LaneSpan, Origin, ResolvedTime, Seconds, Tick, Ticks,
    TicksPerBeat, TimeSignature,
};
