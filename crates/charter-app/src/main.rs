//! charter's editor in a native window, opening the song whose path is the first argument, if
//! there is one.

use std::path::Path;

use charter_ui::Window;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]),
        ..eframe::NativeOptions::default()
    };
    let mut window = Window::default();
    if let Some(path) = std::env::args_os().nth(1) {
        let name = Path::new(&path).display().to_string();
        match std::fs::read(&path) {
            Ok(bytes) => window.open(&name, &bytes),
            Err(error) => window.refuse(&name, &error),
        }
    }
    eframe::run_ui_native("charter", options, move |ui, _frame| window.ui(ui))
}
