//! A song's audio, decoded from a file's bytes into stereo frames a player can play. Decoding
//! only; the output device belongs to the program that plays.

mod click;
mod onsets;
mod peaks;
mod spectrum;
mod tags;
mod track;

pub use click::{Gain, Kit, Tone, Volumes, mix, render};
pub use onsets::{Onset, onsets, onsets_from_bytes, onsets_to_bytes};
pub use peaks::{Extent, Peaks, fingerprint};
pub use spectrum::Spectrum;
pub use tags::{Artwork, Tags, tags};
pub use track::{DecodeError, Track};
