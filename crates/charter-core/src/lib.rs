//! Charter's chart format: the data model from the book's format chapter, its JSON form, and the
//! validation that turns bytes into a valid document.

mod document;
mod error;
mod ident;
mod json;
mod number;
mod path;
mod timing;
mod unit;
mod validate;

pub use document::{
    Chart, Child, DEFAULT_TICKS_PER_BEAT, Document, Extensions, FORMAT_VERSION, Metadata, Object,
    PathPoint, Plugin, Points, Position, Preview, Ruleset, SampleId, Settings, Shape, Timing,
    TimingEvent, TimingKind,
};
pub use error::{Error, InvalidValue, Rule, Violation};
pub use ident::{Ease, Key, Kind, Name, Namespace};
pub use json::{MAX_DOCUMENT_BYTES, ValidDocument};
pub use number::Decimal;
pub use path::ResourcePath;

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
