//! A song's audio, decoded from a file's bytes into stereo frames a player can play. Decoding
//! only; the output device belongs to the program that plays.

mod click;
mod onsets;
mod peaks;
mod spectrum;
mod tags;
mod track;

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

/// Centralised numeric conversions used at DSP and storage boundaries.
mod cast {
    /// Audio buffer length as a floating-point sample count.
    #[expect(
        clippy::as_conversions,
        clippy::cast_precision_loss,
        reason = "audio buffer lengths exceed exact float precision only on impractically large tracks"
    )]
    pub(crate) const fn usize_f64(value: usize) -> f64 {
        value as f64
    }

    /// Short DSP window length as a floating-point sample count.
    #[expect(
        clippy::as_conversions,
        clippy::cast_precision_loss,
        reason = "DSP windows and test buffers are short"
    )]
    pub(crate) const fn usize_f32(value: usize) -> f32 {
        value as f32
    }

    /// Floating-point sample position truncated to an index, saturating outside its range.
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "sample indices saturate as Rust float-to-int casts specify"
    )]
    pub(crate) const fn f64_usize(value: f64) -> usize {
        value as usize
    }

    /// DSP sample value narrowed to the track's `f32` precision.
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        reason = "DSP sample values can lose precision when reduced to f32"
    )]
    pub(crate) const fn f64_f32(value: f64) -> f32 {
        value as f32
    }

    /// Spectrum level scaled into one byte.
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "spectrum levels are scaled to 0..=255 before conversion"
    )]
    pub(crate) const fn f64_u8(value: f64) -> u8 {
        value as u8
    }

    /// Frame count serialised as the low 64 bits on wider platforms.
    #[expect(
        clippy::as_conversions,
        reason = "serialized frame counts use the low 64 bits on wider platforms"
    )]
    pub(crate) const fn usize_u64(value: usize) -> u64 {
        value as u64
    }
}

#[expect(
    clippy::pub_use,
    clippy::useless_attribute,
    reason = "metronome mixing is part of charter-audio's flat DSP API"
)]
pub use click::{Clicks, Gain, Kit, Tick, Tone, Volumes, mix, render};
#[expect(
    clippy::pub_use,
    clippy::useless_attribute,
    reason = "onset detection and cache I/O are part of the flat audio API"
)]
pub use onsets::{Onset, onsets, onsets_from_bytes, onsets_to_bytes};
#[expect(
    clippy::pub_use,
    clippy::useless_attribute,
    reason = "waveform peaks and fingerprints are part of the flat audio API"
)]
pub use peaks::{Extent, Peaks, fingerprint};
#[expect(
    clippy::pub_use,
    clippy::useless_attribute,
    reason = "spectrum analysis is part of the flat audio API"
)]
pub use spectrum::Spectrum;
#[expect(
    clippy::pub_use,
    clippy::useless_attribute,
    reason = "decoded audio tags are part of the flat audio API"
)]
pub use tags::{Artwork, Tags, tags};
#[expect(
    clippy::pub_use,
    clippy::useless_attribute,
    reason = "decoded tracks and errors are part of the flat audio API"
)]
pub use track::{DecodeError, Track};
