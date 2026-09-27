//! Editing a song without drawing it: which chart is open, where the cursor is and how it moves,
//! notes placed, stretched and removed under the song's ruleset plugin, the selection, undo and
//! redo, whether there are unsaved changes, and a demo song to try that on.

pub mod demo;
mod edit;
mod points;
mod song;
mod sync;

pub use song::{KeepNotes, NoteAt, Refusal, SNAPS, Selecting, Song, Step, TimedSpan, Toggled};
pub use sync::Pin;
