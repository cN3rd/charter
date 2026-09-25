//! charter's editor in a native window, opening the song whose path is the first argument, if
//! there is one. Songs of the built-in rulesets can be edited.

use std::path::Path;

use charter_core::RulesetPlugin;
use charter_ui::Window;

/// The ruleset plugins built into charter.
const PLUGINS: &[&dyn RulesetPlugin] = &[
    &charter_stepmania::Plugin,
    &charter_ttr::Plugin,
    &charter_osu_mania::Plugin,
];

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]),
        ..eframe::NativeOptions::default()
    };
    let mut window = Window::new(PLUGINS);
    if let Some(path) = std::env::args_os().nth(1) {
        let path = Path::new(&path);
        match std::fs::read(path) {
            Ok(bytes) => window.open(path, &bytes),
            Err(error) => window.refuse(path, &error),
        }
    }
    eframe::run_ui_native("charter", options, move |ui, _frame| window.ui(ui))
}
