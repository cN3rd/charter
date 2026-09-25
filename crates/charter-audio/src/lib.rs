//! A song's audio, decoded from a file's bytes into stereo frames a player can play. Decoding
//! only; the output device belongs to the program that plays.

mod track;

pub use track::{DecodeError, Track};
