//! The editor window's state, and how actions from its panels, keys and gestures change it.

use egui::Ui;

use charter_editor::{Song, Step, demo};

use crate::notefield::{self, Zoom};
use crate::{panels, style};

/// Something a panel, key or gesture asks the editor to do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Action {
    OpenDemo,
    OpenChart(usize),
    Move(Step),
    MoveMeasure(Step),
    FinerSnap,
    CoarserSnap,
    ZoomBy(f32),
}

/// The editor window's state.
#[derive(Debug, Default)]
pub struct Window {
    song: Option<Song>,
    /// How tall a second of the open song is on screen.
    zoom: Zoom,
    /// Why the last file failed to open.
    failure: Option<String>,
    /// Touchpad scrolling not yet turned into cursor steps.
    scrolled: f32,
}

impl Window {
    /// Opens the song in `bytes`, read from a file called `name`. A file that fails leaves the
    /// open song as it was and says why.
    pub fn open(&mut self, name: &str, bytes: &[u8]) {
        match Song::open(bytes) {
            Ok(song) => {
                self.song = Some(song);
                self.zoom = Zoom::START;
                self.failure = None;
            }
            Err(error) => self.refuse(name, &error),
        }
    }

    /// Says that the file called `name` did not open, and why.
    pub fn refuse(&mut self, name: &str, why: &dyn std::fmt::Display) {
        self.failure = Some(format!("{name} did not open: {why}"));
    }

    pub fn open_demo(&mut self) {
        self.song = Some(Song::new(demo::song()));
        self.zoom = Zoom::START;
        self.failure = None;
    }

    fn apply(&mut self, action: Action) {
        if action == Action::OpenDemo {
            self.open_demo();
            return;
        }
        let Some(song) = &mut self.song else {
            return;
        };
        match action {
            Action::OpenDemo => {}
            Action::OpenChart(index) => song.open_chart(index),
            Action::Move(step) => song.step(step),
            Action::MoveMeasure(step) => song.step_measure(step),
            Action::FinerSnap => song.finer_snap(),
            Action::CoarserSnap => song.coarser_snap(),
            Action::ZoomBy(factor) => self.zoom = self.zoom.by(factor),
        }
    }

    /// Opens every file dropped on the window this frame, in order, so the last one that opens
    /// wins.
    fn open_dropped(&mut self, ui: &Ui) {
        let dropped = ui.input(|input| input.raw.dropped_files.clone());
        for file in dropped {
            let name = file.path().display().to_string();
            match file.bytes() {
                Ok(bytes) => self.open(&name, &bytes),
                Err(error) => self.refuse(&name, &error),
            }
        }
    }

    /// Draws the window into `ui` and answers this frame's input.
    pub fn ui(&mut self, ui: &mut Ui) {
        style::apply(ui.ctx());
        self.open_dropped(ui);
        let mut actions = Vec::new();
        if self.song.is_some() && !ui.ctx().egui_wants_keyboard_input() {
            actions.extend(ui.input(panels::key_actions));
        }
        let song_open = self.song.is_some();
        egui::Panel::top("menu").show(ui, |ui| panels::menu_bar(ui, song_open, &mut actions));
        egui::Panel::bottom("status").show(ui, |ui| {
            panels::status_bar(ui, self.song.as_ref(), self.zoom);
        });
        if let Some(song) = &self.song {
            egui::Panel::left("song").show(ui, |ui| panels::song_panel(ui, song, &mut actions));
            egui::Panel::right("inspector").show(ui, panels::inspector);
        }
        egui::CentralPanel::default().show(ui, |ui| {
            if let Some(failure) = &self.failure {
                panels::failure(ui, failure);
            }
            match &self.song {
                Some(song) => {
                    notefield::show(ui, song, self.zoom, &mut self.scrolled, &mut actions);
                }
                None => panels::empty(ui, &mut actions),
            }
        });
        for action in actions {
            self.apply(action);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use super::*;

    /// The format chapter, embedded at compile time so the tests do no I/O.
    const SPEC: &str = include_str!("../../../docs/src/format.md");

    /// The format chapter's full example song.
    fn example() -> &'static [u8] {
        let start = SPEC.rfind("```json\n").unwrap() + "```json\n".len();
        let end = start + SPEC[start..].find("```").unwrap();
        &SPEC.as_bytes()[start..end]
    }

    fn window(app: Window) -> Harness<'static, Window> {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .build_ui_state(|ui, app: &mut Window| app.ui(ui), app);
        harness.run();
        harness
    }

    fn with_demo() -> Harness<'static, Window> {
        let mut app = Window::default();
        app.open_demo();
        window(app)
    }

    /// A file dropped on the window, held in memory.
    #[derive(Debug)]
    struct Dropped(&'static str, Vec<u8>);

    impl egui::DroppedFile for Dropped {
        fn path(&self) -> &Path {
            Path::new(self.0)
        }

        fn bytes(&self) -> Result<Vec<u8>, String> {
            Ok(self.1.clone())
        }
    }

    #[test]
    fn an_empty_window_offers_the_demo_song() {
        let mut harness = window(Window::default());
        harness.get_by_label("Open the demo song").click();
        harness.run();
        assert!(harness.query_by_label("Demo").is_some());
    }

    #[test]
    fn an_opened_song_lists_its_charts() {
        let mut app = Window::default();
        app.open("example.json", example());
        let harness = window(app);
        assert!(harness.query_all_by_label("Future").next().is_some());
        assert!(harness.query_all_by_label("Beyond").next().is_some());
    }

    #[test]
    fn a_chart_says_what_it_cannot_draw_without_its_ruleset() {
        let mut app = Window::default();
        app.open("example.json", example());
        let harness = window(app);
        let note = "1 object on the plane needs the arcaea plugin to be drawn";
        assert!(harness.query_by_label(note).is_some());
    }

    #[test]
    fn clicking_a_chart_opens_it() {
        let mut harness = with_demo();
        harness.get_by_label("Hard").click();
        harness.run();
        let song = harness.state().song.as_ref().unwrap();
        assert_eq!(song.chart_index(), 1);
    }

    #[test]
    fn a_file_that_breaks_a_rule_says_what_and_where() {
        let broken = String::from_utf8(example().to_vec())
            .unwrap()
            .replace("\"Beyond\"", "\"Future\"");
        let mut app = Window::default();
        app.open("broken.json", broken.as_bytes());
        let harness = window(app);
        assert!(
            harness
                .query_by_label_contains("broken.json did not open")
                .is_some()
        );
        assert!(harness.query_by_label_contains("charts[1]").is_some());
    }

    #[test]
    fn the_keyboard_moves_the_cursor_and_changes_the_snap() {
        let mut harness = with_demo();
        for _ in 0..4 {
            harness.key_press(egui::Key::ArrowDown);
        }
        harness.run();
        assert!(harness.query_by_label("0:1:0").is_some());
        harness.key_press(egui::Key::ArrowRight);
        harness.run();
        assert!(harness.query_by_label("Snap 24th").is_some());
        harness.key_press(egui::Key::PageDown);
        harness.run();
        assert!(harness.query_by_label("1:0:0").is_some());
    }

    #[test]
    fn ctrl_plus_zooms_the_notefield_and_leaves_the_window_its_size() {
        let mut harness = with_demo();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Plus);
        harness.run();
        assert!(harness.query_by_label("Zoom 125%").is_some());
        assert!((harness.ctx.zoom_factor() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn dropping_a_song_on_the_window_opens_it() {
        let mut harness = window(Window::default());
        let file = Arc::new(Dropped("example.json", example().to_vec()));
        harness.input_mut().dropped_files.push(file);
        harness.run();
        assert!(harness.query_all_by_label("Future").next().is_some());
    }
}
