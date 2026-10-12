//! charter's editor window: the notefield, the panels around it, and the state that ties them to
//! an open song. It draws into any egui `Ui` and never touches a file: it asks its host to read,
//! write and choose files through [`Request`]s, so the binary that hosts it opens the window and
//! does that I/O.

mod artwork;
/// Casts at the boundary between screen coordinates, samples and integer indices.
mod cast {
    #[expect(
        clippy::as_conversions,
        clippy::cast_precision_loss,
        reason = "pixel and sample values need floating-point casts"
    )]
    /// A sample or pixel count as a floating-point value for UI calculations.
    pub(crate) const fn usize_f64(value: usize) -> f64 {
        value as f64
    }

    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        reason = "UI pixel values use f32"
    )]
    /// A calculated coordinate narrowed to the precision egui draws with.
    pub(crate) const fn f64_f32(value: f64) -> f32 {
        value as f32
    }

    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "flooring a screen coordinate yields a lane"
    )]
    /// A floored screen coordinate narrowed to a lane index.
    pub(crate) const fn f32_u16(value: f32) -> u16 {
        value as u16
    }

    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "saturating float-to-index cast for display tiles"
    )]
    /// A floating-point display coordinate saturated to a tile index.
    pub(crate) const fn f64_usize(value: f64) -> usize {
        value as usize
    }

    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "rounded color channel is in u8 range"
    )]
    /// A rounded colour channel narrowed to its byte value.
    pub(crate) const fn f32_u8(value: f32) -> u8 {
        value as u8
    }

    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        reason = "UI scroll notches fit in i32"
    )]
    /// A scroll distance narrowed to a count of notches.
    pub(crate) const fn f32_i32(value: f32) -> i32 {
        value as i32
    }

    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        reason = "bounded audio pitch in cents fits in i16"
    )]
    /// An audio pitch offset in cents narrowed to the track's pitch type.
    pub(crate) const fn f64_i16(value: f64) -> i16 {
        value as i16
    }
}
mod flip;
mod import;
mod keys;
mod latency;
mod minimap;
mod new_song;
mod notefield;
mod panels;
mod recovery;
mod settings;
// Every window shares one look; the name keeps `crate::style` paths as they were.
use charter_style as style;
mod timing_panel;
mod window;

#[expect(
    clippy::pub_use,
    clippy::useless_attribute,
    reason = "host-supplied decoded artwork is part of charter-ui's flat host API"
)]
pub use artwork::Picture;
#[expect(
    clippy::pub_use,
    clippy::useless_attribute,
    reason = "the host's flip direction is part of charter-ui's flat host API"
)]
pub use flip::Direction;
#[expect(
    clippy::pub_use,
    clippy::useless_attribute,
    reason = "persisted UI settings are part of charter-ui's flat host API"
)]
pub use settings::{
    AlternateRate, AutosaveSeconds, InterfaceScale, LatencyMs, Settings, SnapshotMinutes,
    SnapshotsKept, SnippetMs, Sound, ZoomPercent,
};
use window::{Action, Note};
#[expect(
    clippy::pub_use,
    clippy::useless_attribute,
    reason = "the window and its host requests are charter-ui's flat host API"
)]
pub use window::{Answer, FfmpegPlatform, LoopRegion, Request, Saved, Tracker, Window};

/// Property tests run from a fixed seed and keep no regression files, so they repeat exactly
/// and touch no files.
#[cfg(test)]
fn proptest_config() -> proptest::test_runner::Config {
    proptest::test_runner::Config {
        cases: 64,
        failure_persistence: None,
        rng_seed: proptest::test_runner::RngSeed::Fixed(0),
        ..proptest::test_runner::Config::default()
    }
}
