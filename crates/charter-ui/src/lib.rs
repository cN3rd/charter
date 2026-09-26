//! charter's editor window: the notefield, the panels around it, and the state that ties them to
//! an open song. It draws into any egui `Ui` and never touches a file: it asks its host to read,
//! write and choose files through [`Request`]s, so the binary that hosts it opens the window and
//! does that I/O.

mod flip;
mod import;
mod keys;
mod new_song;
mod notefield;
mod panels;
mod recovery;
mod settings;
mod style;
mod sync;
mod window;

pub use flip::Direction;
pub use settings::{Settings, Sound};
use window::{Action, Note};
pub use window::{Request, Saved, Window};
