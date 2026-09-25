//! Editing a song without drawing it: which chart is open, where the cursor is and how it moves,
//! notes placed, stretched and removed under the song's ruleset plugin, undo and redo, and a demo
//! song to try that on.

pub mod demo;
mod edit;
mod song;

pub use song::{Refusal, Song, Step, Toggled};
