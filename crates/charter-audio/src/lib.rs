//! A song's audio, decoded from a file's bytes into stereo frames a player can play. Decoding
//! only; the output device belongs to the program that plays.

mod click;
mod peaks;
mod tags;
mod track;

pub use click::{Tone, mix, render};
pub use peaks::{Extent, Peaks};
pub use tags::{Tags, tags};
pub use track::{DecodeError, Track};
