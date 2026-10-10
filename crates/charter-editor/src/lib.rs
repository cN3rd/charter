//! Editing a song without drawing it: which chart is open, where the cursor is and how it moves,
//! notes placed, stretched and removed under the song's ruleset plugin, the selection, undo and
//! redo, whether there are unsaved changes, and a demo song to try that on.

pub mod demo;
mod edit;
mod points;
mod song;
mod sync;

#[expect(
    clippy::pub_use,
    clippy::useless_attribute,
    reason = "editor song state and edits form charter-editor's flat API"
)]
pub use song::{
    ChartIndex, KeepNotes, LayerIndex, LayerNotes, LayerRow, NoteAt, Refusal, SNAPS, Selecting,
    Song, Step, TimedSpan, Toggled,
};
#[expect(
    clippy::pub_use,
    clippy::useless_attribute,
    reason = "timing pins are part of charter-editor's flat API"
)]
pub use sync::Pin;

/// Deterministic property tests with no regression files.
#[cfg(test)]
fn proptest_config() -> proptest::test_runner::Config {
    proptest::test_runner::Config {
        failure_persistence: None,
        rng_seed: proptest::test_runner::RngSeed::Fixed(0),
        ..proptest::test_runner::Config::default()
    }
}
