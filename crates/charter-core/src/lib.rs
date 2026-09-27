//! Charter's chart format: the data model from the book's format chapter, its JSON form, the
//! validation that turns bytes into a valid document, and [`RulesetPlugin`], which a ruleset
//! plugin implements so the editor can check documents against its game.

mod document;
mod error;
mod grid;
mod ident;
mod json;
mod number;
mod path;
mod plugin;
mod sections;
mod timing;
mod unit;
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
    Difficulties, DifficultyPreset, ImportedTiming, Layout, NewChart, NewSong, RulesetPlugin,
    SettingScope, TextSetting,
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
