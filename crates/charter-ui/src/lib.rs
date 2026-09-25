//! charter's editor window: the notefield, the panels around it, and the state that ties them to
//! an open song. It draws into any egui `Ui`, so the binary that hosts it only opens a window.

mod notefield;
mod panels;
mod style;
mod window;

use window::Action;
pub use window::Window;
