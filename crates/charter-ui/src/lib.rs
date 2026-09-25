//! charter's editor window: the notefield, the panels around it, and the state that ties them to
//! an open song. It draws into any egui `Ui` and never touches a file, so the binary that hosts
//! it opens the window and reads songs for it.

mod keys;
mod notefield;
mod panels;
mod style;
mod window;

use window::Action;
pub use window::Window;
