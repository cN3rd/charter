//! The editor window's state, and how actions from its panels, keys and gestures change it.

use std::path::{Path, PathBuf};

use charter_core::{Kind, LaneIndex, RulesetPlugin, Tick};
use egui::Ui;

use charter_editor::{NoteAt, Refusal, Selecting, Song, Step, Toggled, demo};

use crate::notefield::{self, Zoom};
use crate::{keys, panels, style};

/// Something only the program hosting the window can do. The host answers a save through
/// [`Window::saved`], and an open by passing the file to [`Window::open`].
#[derive(Debug, PartialEq, Eq)]
pub enum Request {
    /// Write `bytes`, the song as it stands, to `path`.
    Write { path: PathBuf, bytes: Vec<u8> },
    /// Ask where to save the song, suggesting the file name `name`, then write `bytes` there.
    SaveAs { name: String, bytes: Vec<u8> },
    /// Ask for a song to open.
    Open,
}

/// How a save the window asked for went.
#[derive(Debug, PartialEq, Eq)]
pub enum Saved {
    /// The song is in this file now.
    To(PathBuf),
    /// No file was chosen.
    Cancelled,
    /// The file could not be written, for this reason.
    Failed(String),
}

/// What would lose the open song's unsaved changes, waiting on whether to save them first.
#[derive(Debug)]
enum Discard {
    /// Closing the window.
    Close,
    /// Opening another song, read from `path` unless it is the demo.
    Open {
        path: Option<PathBuf>,
        song: Box<Song>,
    },
}

/// Where a save the user asked for goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Saving {
    /// To the song's file, or wherever the user chooses when it has none.
    InPlace,
    /// Wherever the user chooses.
    Elsewhere,
}

/// How far the window is in closing, once the user chose to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Leaving {
    #[default]
    Staying,
    /// The host's window is to be told to close.
    Now,
    /// Closing, whatever changes are left: a close request goes through.
    Going,
}

/// A note a lane key places.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Note {
    Tap,
    Mine,
}

impl Note {
    fn kind(self) -> Kind {
        match self {
            Self::Tap => Kind::Tap,
            Self::Mine => Kind::Mine,
        }
    }
}

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
    /// Places a note on the lane at the cursor, or removes what starts there.
    Toggle(LaneIndex, Note),
    /// The lane key of a lane that toggled came up.
    LaneUp(LaneIndex),
    /// Every lane key that toggled is up again.
    LanesReleased,
    /// Places a note at `tick` on `lane`, or removes what starts there.
    Place {
        tick: Tick,
        lane: LaneIndex,
        note: Note,
    },
    /// Makes the tap or hold starting on `lane` at `from` end at `to`.
    Stretch {
        lane: LaneIndex,
        from: Tick,
        to: Tick,
    },
    /// Meets the selection with a note, or with none: nothing was under the pointer.
    Select(Option<NoteAt>, Selecting),
    /// Meets the selection with the notes starting within `ticks` on a lane within `lanes`.
    SelectBetween {
        ticks: (Tick, Tick),
        lanes: (LaneIndex, LaneIndex),
        how: Selecting,
    },
    Undo,
    Redo,
    SelectAll,
    ClearSelection,
    DeleteSelection,
    /// Asks the host for a song to open.
    Open,
    /// Saves the song to its file, or asks where when it has none.
    Save,
    /// Asks where to save the song.
    SaveAs,
}

/// The editor window's state.
#[derive(Debug, Default)]
pub struct Window {
    /// The ruleset plugins every song is checked against.
    plugins: &'static [&'static dyn RulesetPlugin],
    song: Option<Song>,
    /// The file the open song was read from or last saved to.
    path: Option<PathBuf>,
    /// What the host has yet to do.
    requests: Vec<Request>,
    /// The title last given to the host's window.
    shown_title: String,
    /// A save asked for this frame, made once every edit of the frame is in.
    saving: Option<Saving>,
    /// What waits on the answer to whether to save the open song's changes.
    discard: Option<Discard>,
    /// What to do once the save the user asked for is done.
    after_save: Option<Discard>,
    /// How far the window is in closing.
    leaving: Leaving,
    /// How tall a second of the open song is on screen.
    zoom: Zoom,
    /// Why the last file failed to open, or the last edit was refused.
    failure: Option<String>,
    /// Touchpad scrolling not yet turned into cursor steps.
    scrolled: f32,
    /// Which lane keys are down, and which of those placed or removed a note.
    lane_keys: keys::LaneKeys,
    /// Whether a lane key placed a note that the cursor has not moved on from, which it does once
    /// every lane key is up, so the keys of a chord place it on one tick.
    placed: bool,
    /// Taps placed by lane keys still down, by lane and tick. Each stretches into a hold to the
    /// cursor as it moves.
    holding: Vec<(LaneIndex, Tick)>,
    /// A drag across the notefield in progress.
    gesture: Option<notefield::Gesture>,
}

impl Window {
    /// A window with no song open, which checks songs against `plugins`.
    #[must_use]
    pub fn new(plugins: &'static [&'static dyn RulesetPlugin]) -> Self {
        Self {
            plugins,
            ..Self::default()
        }
    }

    /// What the window needs its host to do, oldest first. A host takes these after every frame.
    pub fn take_requests(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.requests)
    }

    /// Takes how a save the window asked for went.
    pub fn saved(&mut self, saved: Saved) {
        match saved {
            Saved::To(path) => {
                if let Some(song) = &mut self.song {
                    song.mark_saved();
                }
                self.path = Some(path);
                self.failure = None;
                if let Some(discard) = self.after_save.take() {
                    self.carry_on(discard);
                }
            }
            Saved::Cancelled => self.after_save = None,
            Saved::Failed(why) => {
                self.after_save = None;
                self.failure = Some(format!("Could not save: {why}"));
            }
        }
    }

    /// The name the host's window goes by: the open song, starred while it has unsaved changes.
    #[must_use]
    pub fn title(&self) -> String {
        let Some(song) = &self.song else {
            return "charter".to_owned();
        };
        let star = if song.is_modified() { "*" } else { "" };
        format!("{star}{} - charter", self.file_name(song))
    }

    /// The open song's file name, or one made from its title when it has no file.
    fn file_name(&self, song: &Song) -> String {
        file_name(self.path.as_deref(), song)
    }

    /// Asks the host to write the song to its file, or to ask where when `saving` says so or the
    /// song has no file.
    fn save(&mut self, saving: Saving) {
        let Some(song) = &self.song else {
            return;
        };
        let bytes = match song.document().to_json() {
            Ok(bytes) => bytes,
            Err(error) => {
                self.failure = Some(format!("Could not save: {error}"));
                return;
            }
        };
        let request = match &self.path {
            Some(path) if saving == Saving::InPlace => Request::Write {
                path: path.clone(),
                bytes,
            },
            _ => Request::SaveAs {
                name: self.file_name(song),
                bytes,
            },
        };
        self.requests.push(request);
    }

    /// Opens the song in `bytes`, read from the file at `path`. A file that fails leaves the
    /// open song as it was and says why.
    ///
    /// Over a song with unsaved changes, it first asks whether to save them.
    pub fn open(&mut self, path: &Path, bytes: &[u8]) {
        match Song::open(bytes, self.plugins) {
            Ok(song) => self.after_asking(Discard::Open {
                path: Some(path.to_path_buf()),
                song: Box::new(song),
            }),
            Err(error) => self.refuse(path, &error),
        }
    }

    /// Says that the file at `path` did not open, and why.
    pub fn refuse(&mut self, path: &Path, why: &dyn std::fmt::Display) {
        self.failure = Some(format!("{} did not open: {why}", path.display()));
    }

    /// Opens the demo song, first asking whether to save unsaved changes to the song open.
    pub fn open_demo(&mut self) {
        let song = Box::new(Song::new(demo::song(), self.plugins));
        self.after_asking(Discard::Open { path: None, song });
    }

    /// Does what would lose the open song's unsaved changes, once the user says what to do with
    /// them; at once when there are none.
    fn after_asking(&mut self, discard: Discard) {
        if self.song.as_ref().is_some_and(Song::is_modified) {
            self.discard = Some(discard);
        } else {
            self.carry_on(discard);
        }
    }

    fn carry_on(&mut self, discard: Discard) {
        match discard {
            Discard::Close => self.leaving = Leaving::Now,
            Discard::Open { path, song } => {
                self.saving = None;
                self.song = Some(*song);
                self.path = path;
                self.zoom = Zoom::START;
                self.failure = None;
                self.placed = false;
                self.holding.clear();
            }
        }
    }

    /// Takes the answer to whether to save before the changes are lost.
    fn answer(&mut self, answer: panels::Answer) {
        let Some(discard) = self.discard.take() else {
            return;
        };
        match answer {
            panels::Answer::Save => {
                self.after_save = Some(discard);
                self.saving = Some(Saving::InPlace);
            }
            panels::Answer::DontSave => self.carry_on(discard),
            panels::Answer::Cancel => {}
        }
    }

    fn apply(&mut self, action: Action) {
        // What the window does whether or not a song is open.
        match action {
            Action::OpenDemo => self.open_demo(),
            Action::Open => self.requests.push(Request::Open),
            Action::Save => self.saving = Some(Saving::InPlace),
            Action::SaveAs => self.saving = Some(Saving::Elsewhere),
            _ => {}
        }
        let Some(song) = &mut self.song else {
            return;
        };
        match action {
            Action::OpenChart(index) => {
                song.open_chart(index);
                self.holding.clear();
            }
            Action::Move(step) => {
                song.step(step);
                self.failure = stretch(song, &self.holding).or(self.failure.take());
            }
            Action::MoveMeasure(step) => {
                song.step_measure(step);
                self.failure = stretch(song, &self.holding).or(self.failure.take());
            }
            Action::FinerSnap => song.finer_snap(),
            Action::CoarserSnap => song.coarser_snap(),
            Action::ZoomBy(factor) => self.zoom = self.zoom.by(factor),
            Action::Toggle(lane, note) => {
                let toggled = song.toggle(song.cursor(), lane, note.kind());
                if let Ok(Toggled::Placed) = toggled {
                    self.placed = true;
                    if note == Note::Tap {
                        self.holding.push((lane, song.cursor()));
                    }
                }
                self.failure = refused(toggled.err());
            }
            Action::LaneUp(lane) => self.holding.retain(|(held, _)| *held != lane),
            Action::LanesReleased => {
                if std::mem::take(&mut self.placed) {
                    song.step(Step::Later);
                }
            }
            Action::Undo => {
                self.placed = false;
                self.holding.clear();
                self.failure = refused(song.undo().err());
            }
            Action::Redo => {
                self.placed = false;
                self.holding.clear();
                self.failure = refused(song.redo().err());
            }
            Action::Place { tick, lane, note } => {
                self.failure = refused(song.toggle(tick, lane, note.kind()).err());
            }
            Action::Stretch { lane, from, to } => {
                self.failure = refused(song.stretch(lane, from, to).err());
            }
            Action::Select(Some(note), how) => song.select([note], how),
            Action::Select(None, Selecting::Replace) | Action::ClearSelection => {
                song.clear_selection();
            }
            Action::OpenDemo
            | Action::Open
            | Action::Save
            | Action::SaveAs
            | Action::Select(None, Selecting::Add | Selecting::Subtract) => {}
            Action::SelectBetween { ticks, lanes, how } => {
                song.select_between(ticks.0..=ticks.1, lanes.0..=lanes.1, how);
            }
            Action::SelectAll => song.select_all(),
            Action::DeleteSelection => self.failure = refused(song.delete_selection().err()),
        }
    }

    /// Opens every file dropped on the window this frame, in order, so the last one that opens
    /// wins.
    fn open_dropped(&mut self, ui: &Ui) {
        let dropped = ui.input(|input| input.raw.dropped_files.clone());
        for file in dropped {
            match file.bytes() {
                Ok(bytes) => self.open(file.path(), &bytes),
                Err(error) => self.refuse(file.path(), &error),
            }
        }
    }

    /// Draws the window into `ui` and answers this frame's input.
    pub fn ui(&mut self, ui: &mut Ui) {
        style::apply(ui.ctx());
        if self.leaving == Leaving::Now {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            self.leaving = Leaving::Going;
        }
        if ui.input(|input| input.viewport().close_requested()) && self.leaving == Leaving::Staying
        {
            if self.song.as_ref().is_some_and(Song::is_modified) {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.discard = Some(Discard::Close);
            } else {
                self.leaving = Leaving::Going;
            }
        }
        if self.discard.is_some() {
            let name = self.song.as_ref().map(|song| self.file_name(song));
            if let Some(answer) = panels::ask_to_save(ui, name.as_deref().unwrap_or_default()) {
                self.answer(answer);
            }
        }
        self.open_dropped(ui);
        let mut actions = Vec::new();
        if let Some(song) = &self.song {
            let lanes = editable_lanes(song);
            let lane_keys = &mut self.lane_keys;
            let asked = ui.input(|input| keys::key_actions(input, lanes, lane_keys));
            // A focused widget or a question takes the keyboard, but a lane key coming up still
            // ends its chord.
            let busy = ui.ctx().egui_wants_keyboard_input() || self.discard.is_some();
            let ends_a_chord =
                |action: &Action| matches!(action, Action::LaneUp(_) | Action::LanesReleased);
            actions.extend(
                asked
                    .into_iter()
                    .filter(|action| !busy || ends_a_chord(action)),
            );
        }
        let song = self.song.as_ref();
        egui::Panel::top("menu").show(ui, |ui| panels::menu_bar(ui, song, &mut actions));
        egui::Panel::bottom("status").show(ui, |ui| {
            panels::status_bar(ui, self.song.as_ref(), self.zoom);
        });
        if let Some(song) = &self.song {
            egui::Panel::left("song").show(ui, |ui| panels::song_panel(ui, song, &mut actions));
            egui::Panel::right("inspector").show(ui, |ui| panels::inspector(ui, song));
        }
        egui::CentralPanel::default().show(ui, |ui| {
            if let Some(failure) = &self.failure {
                panels::failure(ui, failure);
            }
            if let Some(why) = self.song.as_ref().and_then(Song::read_only) {
                panels::read_only(ui, why);
            }
            match &self.song {
                Some(song) => {
                    let (zoom, scrolled, gesture) =
                        (self.zoom, &mut self.scrolled, &mut self.gesture);
                    notefield::show(ui, song, zoom, scrolled, gesture, &mut actions);
                }
                None => panels::empty(ui, &mut actions),
            }
        });
        for action in actions {
            self.apply(action);
        }
        if let Some(saving) = self.saving.take() {
            self.save(saving);
        }
        let title = self.title();
        if title != self.shown_title {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.shown_title = title;
        }
    }
}

/// The file name of `song`, read from `path`, or one made from its title when it has no file.
fn file_name(path: Option<&Path>, song: &Song) -> String {
    path.and_then(Path::file_name).map_or_else(
        || format!("{}.json", song.document().document().metadata.title),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// How many lanes of the open chart lane keys can edit: none when the song is read-only.
fn editable_lanes(song: &Song) -> u16 {
    if song.read_only().is_some() {
        return 0;
    }
    song.chart()
        .and_then(|chart| chart.lanes)
        .map_or(0, u16::from)
}

/// Stretches each tap a held lane key placed into a hold to the cursor, and says why if the song
/// refuses one.
fn stretch(song: &mut Song, holding: &[(LaneIndex, Tick)]) -> Option<String> {
    let cursor = song.cursor();
    let refusals = holding
        .iter()
        .filter_map(|&(lane, from)| song.stretch(lane, from, cursor).err());
    refused(refusals.last())
}

/// What to say about a refused edit, or nothing when the edit went through.
fn refused(refusal: Option<Refusal>) -> Option<String> {
    refusal.map(|refusal| format!("Refused: {refusal}"))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use std::error::Error;

    use charter_core::{
        Chart, LaneCount, LaneIndex, LaneSpan, Position, Tick, Valid, ValidDocument,
    };
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};

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

    /// Accepts every `sm` document, standing in for the StepMania plugin.
    #[derive(Debug)]
    struct AnySm;

    impl RulesetPlugin for AnySm {
        fn id(&self) -> &'static str {
            "sm"
        }

        fn check(&self, _: &ValidDocument) -> Result<(), Box<dyn Error + Send + Sync>> {
            Ok(())
        }
    }

    /// Refuses an `sm` document with a note on the fourth lane, which no format rule does.
    #[derive(Debug)]
    struct NoFourthLane;

    impl RulesetPlugin for NoFourthLane {
        fn id(&self) -> &'static str {
            "sm"
        }

        fn check(&self, document: &ValidDocument) -> Result<(), Box<dyn Error + Send + Sync>> {
            let fourth = Some(LaneSpan::new(LaneIndex::new(3), LaneCount::ONE));
            let on_fourth = |chart: Valid<'_, Chart>| {
                chart
                    .objects()
                    .any(|object| object.position.lanes() == fourth)
            };
            if document.charts().any(on_fourth) {
                return Err("a note on lane 4".into());
            }
            Ok(())
        }
    }

    const PLUGINS: &[&dyn RulesetPlugin] = &[&AnySm];

    fn editable_demo() -> Harness<'static, Window> {
        let mut app = Window::new(PLUGINS);
        app.open_demo();
        window(app)
    }

    /// The lanes of the open chart's objects on `tick`, lowest first.
    fn lanes_at(harness: &Harness<'_, Window>, tick: i32) -> Vec<u16> {
        let song = harness.state().song.as_ref().unwrap();
        let chart = song.chart().unwrap();
        let mut lanes: Vec<u16> = chart
            .objects
            .iter()
            .filter(|object| object.tick == Tick::new(tick))
            .filter_map(|object| match object.position {
                Position::Lane { index, .. } => Some(u16::from(index)),
                Position::Plane { .. } => None,
            })
            .collect();
        lanes.sort_unstable();
        lanes
    }

    fn cursor_shows(harness: &Harness<'_, Window>, position: &str) -> bool {
        harness.query_by_label(position).is_some()
    }

    #[test]
    fn a_song_its_plugin_accepts_opens_for_editing() {
        let harness = editable_demo();
        assert!(harness.query_by_label("Editing").is_some());
        assert!(harness.query_by_label_contains("Read only").is_none());
    }

    #[test]
    fn a_song_without_its_plugin_is_read_only_and_names_the_plugin() {
        let mut app = Window::new(PLUGINS);
        app.open(Path::new("example.json"), example());
        let harness = window(app);
        assert!(
            harness
                .query_by_label("Read only: charter has no arcaea plugin")
                .is_some()
        );
        assert!(harness.query_by_label("Read only").is_some());
    }

    #[test]
    fn a_lane_key_places_a_tap_at_the_cursor_and_moves_on_a_snap() {
        let mut harness = editable_demo();
        harness.key_press(egui::Key::Num1);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [0, 3]);
        assert!(cursor_shows(&harness, "0:0:240"));
    }

    #[test]
    fn lane_keys_held_together_place_a_chord_and_move_on_once_all_are_up() {
        let mut harness = editable_demo();
        harness.key_down(egui::Key::Num1);
        harness.key_down(egui::Key::Num2);
        harness.run();
        harness.key_up(egui::Key::Num1);
        harness.run();
        assert!(cursor_shows(&harness, "0:0:0"));
        harness.key_up(egui::Key::Num2);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [0, 1, 3]);
        assert!(cursor_shows(&harness, "0:0:240"));
    }

    /// Presses and releases each of `keys` in turn, all in the next frame, as a fast typist does
    /// when frames are slow. Queued events would each get a frame of their own.
    fn tap_in_one_frame(harness: &mut Harness<'_, Window>, keys: &[egui::Key]) {
        for &key in keys {
            for pressed in [true, false] {
                harness.input_mut().events.push(egui::Event::Key {
                    key,
                    pressed,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                    physical_key: None,
                });
            }
        }
    }

    #[test]
    fn a_lane_key_tapped_twice_in_one_frame_places_two_notes_down_the_lane() {
        let mut harness = editable_demo();
        tap_in_one_frame(&mut harness, &[egui::Key::Num1, egui::Key::Num1]);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [0, 3]);
        assert_eq!(lanes_at(&harness, 240), [0]);
        assert!(cursor_shows(&harness, "0:0:480"));
    }

    #[test]
    fn two_lane_keys_tapped_in_one_frame_make_a_stream_not_a_chord() {
        let mut harness = editable_demo();
        tap_in_one_frame(&mut harness, &[egui::Key::Num1, egui::Key::Num2]);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [0, 3]);
        assert_eq!(lanes_at(&harness, 240), [1]);
    }

    #[test]
    fn keys_in_one_frame_act_in_the_order_they_were_pressed() {
        let mut harness = editable_demo();
        tap_in_one_frame(&mut harness, &[egui::Key::Num1, egui::Key::ArrowDown]);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [0, 3]);
        assert!(cursor_shows(&harness, "0:0:480"));
    }

    #[test]
    fn a_lane_key_released_under_ctrl_still_moves_the_cursor_on() {
        let mut harness = editable_demo();
        harness.key_down(egui::Key::Num1);
        harness.run();
        harness.event(egui::Event::ModifiersChanged(egui::Modifiers::COMMAND));
        harness.event(egui::Event::Key {
            key: egui::Key::Num1,
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
            physical_key: None,
        });
        harness.run();
        assert!(cursor_shows(&harness, "0:0:240"));
    }

    /// A key event as a keyboard sends it: what the layout makes of the key under `modifiers`,
    /// and where the key sits.
    fn keyboard(
        key: egui::Key,
        place: egui::Key,
        pressed: bool,
        modifiers: egui::Modifiers,
    ) -> egui::Event {
        egui::Event::Key {
            key,
            pressed,
            repeat: false,
            modifiers,
            physical_key: Some(place),
        }
    }

    #[test]
    fn a_lane_key_released_under_shift_still_moves_the_cursor_on() {
        let mut harness = editable_demo();
        let shift = egui::Modifiers::SHIFT;
        harness.event(keyboard(
            egui::Key::Num1,
            egui::Key::Num1,
            true,
            egui::Modifiers::NONE,
        ));
        harness.run();
        harness.event(egui::Event::ModifiersChanged(shift));
        harness.event(keyboard(
            egui::Key::Exclamationmark,
            egui::Key::Num1,
            false,
            shift,
        ));
        harness.run();
        assert!(cursor_shows(&harness, "0:0:240"));
    }

    #[test]
    fn a_lane_key_held_through_shift_coming_up_repeats_without_toggling() {
        let mut harness = editable_demo();
        shift_down(&harness, egui::Key::Exclamationmark, egui::Key::Num1);
        harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        let none = egui::Modifiers::NONE;
        harness.event(keyboard(egui::Key::Num1, egui::Key::Num1, true, none));
        harness.run();
        assert_eq!(note_at(&harness, 0, 0), Some(mine()));
    }

    #[test]
    fn ctrl_minus_zooms_out_where_the_layout_puts_minus_on_a_digit_key() {
        let mut harness = editable_demo();
        let command = egui::Modifiers::COMMAND;
        harness.event(egui::Event::ModifiersChanged(command));
        harness.event(keyboard(egui::Key::Minus, egui::Key::Num6, true, command));
        harness.run();
        assert!(harness.query_by_label("Zoom 80%").is_some());
    }

    #[test]
    fn a_numpad_arrow_moves_the_cursor_rather_than_placing_a_note() {
        let mut harness = editable_demo();
        let none = egui::Modifiers::NONE;
        harness.event(keyboard(egui::Key::ArrowDown, egui::Key::Num2, true, none));
        harness.event(keyboard(egui::Key::ArrowDown, egui::Key::Num2, false, none));
        harness.run();
        assert!(cursor_shows(&harness, "0:0:240"));
        assert_eq!(lanes_at(&harness, 0), [3]);
    }

    #[test]
    fn a_lane_key_released_while_a_widget_has_focus_still_moves_the_cursor_on() {
        let mut harness = editable_demo();
        harness.key_down(egui::Key::Num1);
        harness.run();
        harness.key_press(egui::Key::Tab);
        harness.run();
        assert!(harness.ctx.egui_wants_keyboard_input());
        harness.key_up(egui::Key::Num1);
        harness.run();
        assert!(cursor_shows(&harness, "0:0:240"));
    }

    #[test]
    fn losing_focus_with_a_lane_key_down_moves_on_as_if_it_came_up() {
        let mut harness = editable_demo();
        harness.key_down(egui::Key::Num1);
        harness.run();
        harness.event(egui::Event::WindowFocused(false));
        harness.run();
        assert!(cursor_shows(&harness, "0:0:240"));
        harness.event(egui::Event::WindowFocused(true));
        harness.key_press(egui::Key::Num1);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [0, 3]);
        assert_eq!(lanes_at(&harness, 240), [0]);
    }

    #[test]
    fn undoing_a_note_whose_key_is_still_down_keeps_the_cursor_when_it_comes_up() {
        let mut harness = editable_demo();
        harness.key_down(egui::Key::Num1);
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        harness.key_up(egui::Key::Num1);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [3]);
        assert!(cursor_shows(&harness, "0:0:0"));
    }

    #[test]
    fn opening_a_song_with_a_lane_key_down_leaves_its_cursor_alone() {
        let mut harness = editable_demo();
        harness.key_down(egui::Key::Num1);
        harness.run();
        harness.state_mut().open_demo();
        harness.run();
        harness.get_by_label("Don't save").click();
        harness.key_up(egui::Key::Num1);
        harness.run();
        assert!(cursor_shows(&harness, "0:0:0"));
    }

    #[test]
    fn keys_wait_while_the_window_asks_about_unsaved_changes() {
        let mut harness = edited_four_taps();
        harness
            .state_mut()
            .open(Path::new("example.json"), example());
        harness.run();
        harness.key_press(egui::Key::Num3);
        harness.run();
        assert_eq!(note_count(&harness), 5);
    }

    /// What starts on `lane` at `tick` in the open chart, as the format writes it.
    fn note_at(harness: &Harness<'_, Window>, tick: i32, lane: u16) -> Option<serde_json::Value> {
        let song = harness.state().song.as_ref().unwrap();
        let chart = song.chart().unwrap();
        chart
            .objects
            .iter()
            .find(|object| {
                object.tick == Tick::new(tick)
                    && matches!(object.position, Position::Lane { index, .. } if index == LaneIndex::new(lane))
            })
            .map(|object| serde_json::json!({ "kind": object.kind.to_string(), "shape": object.shape }))
    }

    fn tap() -> serde_json::Value {
        serde_json::json!({ "kind": "tap", "shape": "instant" })
    }

    fn hold(duration: u32) -> serde_json::Value {
        serde_json::json!({ "kind": "hold", "shape": { "sustain": { "duration": duration } } })
    }

    fn mine() -> serde_json::Value {
        serde_json::json!({ "kind": "mine", "shape": "instant" })
    }

    /// Presses the lane key for `lane` under Shift, which a US layout reports as a symbol.
    fn shift_down(harness: &Harness<'_, Window>, symbol: egui::Key, place: egui::Key) {
        let shift = egui::Modifiers::SHIFT;
        harness.event(egui::Event::ModifiersChanged(shift));
        harness.event(keyboard(symbol, place, true, shift));
    }

    #[test]
    fn shift_and_a_lane_key_place_a_mine_and_move_on() {
        let mut harness = editable_demo();
        shift_down(&harness, egui::Key::Exclamationmark, egui::Key::Num1);
        harness.event(keyboard(
            egui::Key::Exclamationmark,
            egui::Key::Num1,
            false,
            egui::Modifiers::SHIFT,
        ));
        harness.run();
        assert_eq!(note_at(&harness, 0, 0), Some(mine()));
        assert!(cursor_shows(&harness, "0:0:240"));
    }

    #[test]
    fn holding_a_lane_key_while_the_cursor_moves_draws_a_hold() {
        let mut harness = editable_demo();
        harness.key_down(egui::Key::Num1);
        harness.key_press(egui::Key::ArrowDown);
        harness.key_press(egui::Key::ArrowDown);
        harness.run();
        assert_eq!(note_at(&harness, 0, 0), Some(hold(480)));
        harness.key_up(egui::Key::Num1);
        harness.run();
        assert!(cursor_shows(&harness, "0:0:720"));
    }

    #[test]
    fn a_hold_drawn_with_a_lane_key_undoes_in_one_step() {
        let mut harness = editable_demo();
        harness.key_down(egui::Key::Num1);
        harness.key_press(egui::Key::ArrowDown);
        harness.key_up(egui::Key::Num1);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(note_at(&harness, 0, 0), None);
        assert!(cursor_shows(&harness, "0:0:0"));
    }

    #[test]
    fn moving_back_to_where_the_key_went_down_makes_the_hold_a_tap_again() {
        let mut harness = editable_demo();
        harness.key_down(egui::Key::Num1);
        harness.key_press(egui::Key::ArrowDown);
        harness.key_press(egui::Key::ArrowUp);
        harness.run();
        assert_eq!(note_at(&harness, 0, 0), Some(tap()));
    }

    #[test]
    fn a_mine_does_not_stretch() {
        let mut harness = editable_demo();
        shift_down(&harness, egui::Key::Exclamationmark, egui::Key::Num1);
        harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        harness.key_press(egui::Key::ArrowDown);
        harness.run();
        assert_eq!(note_at(&harness, 0, 0), Some(mine()));
    }

    /// An `sm` song with one four-lane chart of taps walking across the lanes a sixteenth apart.
    fn four_taps() -> Vec<u8> {
        let tap = |tick: i32, lane: u16| {
            serde_json::json!({ "tick": tick, "position": { "lane": { "index": lane } },
                                "kind": "tap", "shape": "instant" })
        };
        let song = serde_json::json!({
            "format_version": 0,
            "ruleset": { "id": "sm", "version": "1" },
            "metadata": { "title": "Four", "artist": "a", "audio": "a.ogg" },
            "timing": { "offset_seconds": 0.0, "events": [ { "tick": 0, "kind": { "bpm": 120.0 } } ] },
            "charts": [ { "name": "Easy", "lanes": 4,
                          "objects": [tap(0, 0), tap(240, 1), tap(480, 2), tap(720, 3)] } ]
        });
        serde_json::to_vec(&song).unwrap()
    }

    fn editable_four_taps() -> Harness<'static, Window> {
        let mut app = Window::new(PLUGINS);
        app.open(Path::new("four.json"), &four_taps());
        window(app)
    }

    fn note_count(harness: &Harness<'_, Window>) -> usize {
        let song = harness.state().song.as_ref().unwrap();
        song.chart().unwrap().objects.len()
    }

    #[test]
    fn ctrl_a_selects_the_chart_and_delete_removes_it_in_one_step() {
        let mut harness = editable_four_taps();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.run();
        assert!(harness.query_by_label("4 notes").is_some());
        harness.key_press(egui::Key::Delete);
        harness.run();
        assert_eq!(note_count(&harness), 0);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(note_count(&harness), 4);
    }

    #[test]
    fn escape_clears_the_selection() {
        let mut harness = editable_four_taps();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.key_press(egui::Key::Escape);
        harness.run();
        assert!(harness.query_by_label("Nothing selected").is_some());
        harness.key_press(egui::Key::Backspace);
        harness.run();
        assert_eq!(note_count(&harness), 4);
    }

    #[test]
    fn the_inspector_shows_what_the_selection_shares_and_mixed_where_it_differs() {
        let mut harness = editable_four_taps();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.run();
        assert_eq!(harness.query_all_by_label("mixed").count(), 2);
        assert!(harness.query_by_label("tap").is_some());
        assert!(harness.query_by_label("instant").is_some());
    }

    /// Where the middle of `lane` meets the line of `tick` on screen, `below` points lower.
    fn spot(harness: &Harness<'_, Window>, lane: u16, tick: i32, below: f32) -> egui::Pos2 {
        let area = harness.get_by_label("Notefield").rect();
        let app = harness.state();
        let song = app.song.as_ref().unwrap();
        let chart = song.chart().unwrap();
        let timing = chart.timing();
        let lanes = chart.lanes.map_or(0, u16::from);
        let field = notefield::Field::new(area, lanes, timing.time_at(song.cursor()), app.zoom);
        let x = field
            .columns
            .span(LaneIndex::new(lane), LaneCount::ONE)
            .center();
        egui::pos2(x, field.view.y(timing.time_at(Tick::new(tick))) + below)
    }

    fn press(
        harness: &Harness<'_, Window>,
        pos: egui::Pos2,
        pressed: bool,
        modifiers: egui::Modifiers,
    ) {
        harness.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        });
    }

    fn click(harness: &mut Harness<'_, Window>, pos: egui::Pos2, modifiers: egui::Modifiers) {
        harness.hover_at(pos);
        harness.event(egui::Event::ModifiersChanged(modifiers));
        press(harness, pos, true, modifiers);
        press(harness, pos, false, modifiers);
        harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        harness.run();
    }

    fn drag(
        harness: &mut Harness<'_, Window>,
        from: egui::Pos2,
        to: egui::Pos2,
        modifiers: egui::Modifiers,
    ) {
        harness.hover_at(from);
        harness.event(egui::Event::ModifiersChanged(modifiers));
        press(harness, from, true, modifiers);
        for step in 1..=4 {
            #[allow(clippy::cast_precision_loss)] // four steps
            harness.hover_at(from.lerp(to, step as f32 / 4.0));
        }
        press(harness, to, false, modifiers);
        harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        harness.run();
    }

    #[test]
    fn ctrl_clicking_a_lane_places_a_tap_on_the_nearest_snap_line() {
        let mut harness = editable_four_taps();
        let pos = spot(&harness, 3, 240, 4.0);
        click(&mut harness, pos, egui::Modifiers::COMMAND);
        assert_eq!(note_at(&harness, 240, 3), Some(tap()));
    }

    #[test]
    fn ctrl_shift_clicking_places_a_mine() {
        let mut harness = editable_four_taps();
        let pos = spot(&harness, 1, 480, 0.0);
        click(
            &mut harness,
            pos,
            egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT),
        );
        assert_eq!(note_at(&harness, 480, 1), Some(mine()));
    }

    #[test]
    fn ctrl_dragging_down_a_lane_draws_a_hold_that_undoes_in_one_step() {
        let mut harness = editable_four_taps();
        let (from, to) = (spot(&harness, 3, 240, 0.0), spot(&harness, 3, 720, -3.0));
        drag(&mut harness, from, to, egui::Modifiers::COMMAND);
        assert_eq!(note_at(&harness, 240, 3), Some(hold(480)));
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(note_at(&harness, 240, 3), None);
    }

    #[test]
    fn clicking_notes_selects_them_as_shift_adds_and_alt_takes_away() {
        let mut harness = editable_four_taps();
        let (first, second) = (spot(&harness, 0, 0, 0.0), spot(&harness, 1, 240, 2.0));
        click(&mut harness, first, egui::Modifiers::NONE);
        assert!(harness.query_by_label("1 note").is_some());
        click(&mut harness, second, egui::Modifiers::SHIFT);
        assert!(harness.query_by_label("2 notes").is_some());
        click(&mut harness, first, egui::Modifiers::ALT);
        assert!(harness.query_by_label("1 note").is_some());
        let empty = spot(&harness, 3, 0, 0.0);
        click(&mut harness, empty, egui::Modifiers::NONE);
        assert!(harness.query_by_label("Nothing selected").is_some());
    }

    #[test]
    fn dragging_a_box_selects_the_notes_whose_heads_it_covers() {
        let mut harness = editable_four_taps();
        let (from, to) = (spot(&harness, 1, 180, 0.0), spot(&harness, 2, 540, 0.0));
        drag(&mut harness, from, to, egui::Modifiers::NONE);
        assert!(harness.query_by_label("2 notes").is_some());
        let song = harness.state().song.as_ref().unwrap();
        let ticks: Vec<_> = song.selection().map(|note| note.tick).collect();
        assert_eq!(ticks, [Tick::new(240), Tick::new(480)]);
    }

    #[test]
    fn the_mouse_places_nothing_in_a_read_only_song() {
        let mut app = Window::default();
        app.open(Path::new("four.json"), &four_taps());
        let mut harness = window(app);
        let pos = spot(&harness, 3, 240, 0.0);
        click(&mut harness, pos, egui::Modifiers::COMMAND);
        assert_eq!(note_at(&harness, 240, 3), None);
        assert!(harness.query_by_label_contains("Refused").is_none());
    }

    /// The one request the window made since the last were taken.
    fn request(harness: &mut Harness<'_, Window>) -> Request {
        let mut requests = harness.state_mut().take_requests();
        assert_eq!(requests.len(), 1, "{requests:?}");
        requests.remove(0)
    }

    /// Whether `bytes` hold a song whose open chart has a tap on `lane` at `tick`.
    fn holds_tap(bytes: &[u8], tick: i32, lane: u16) -> bool {
        let song = charter_core::ValidDocument::from_json(bytes).unwrap();
        let chart = song.charts().next().unwrap();
        chart.objects().any(|object| {
            object.tick == Tick::new(tick)
                && object.position.lanes()
                    == Some(LaneSpan::new(LaneIndex::new(lane), LaneCount::ONE))
        })
    }

    #[test]
    fn ctrl_s_writes_the_song_back_to_its_file() {
        let mut harness = editable_four_taps();
        harness.key_press(egui::Key::Num4);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        let Request::Write { path, bytes } = request(&mut harness) else {
            panic!("expected a write");
        };
        assert_eq!(path, Path::new("four.json"));
        assert!(holds_tap(&bytes, 0, 3));
    }

    #[test]
    fn saving_a_song_without_a_file_asks_where() {
        let mut harness = editable_demo();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        let Request::SaveAs { name, .. } = request(&mut harness) else {
            panic!("expected save as");
        };
        assert_eq!(name, "Demo.json");
    }

    #[test]
    fn ctrl_shift_s_asks_where_even_for_a_song_with_a_file() {
        let mut harness = editable_four_taps();
        let command_shift = egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT);
        harness.key_press_modifiers(command_shift, egui::Key::S);
        harness.run();
        let Request::SaveAs { name, .. } = request(&mut harness) else {
            panic!("expected save as");
        };
        assert_eq!(name, "four.json");
    }

    #[test]
    fn ctrl_o_and_the_file_menu_ask_for_a_song_to_open() {
        let mut harness = editable_four_taps();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::O);
        harness.run();
        assert_eq!(request(&mut harness), Request::Open);
        harness.get_by_label("File").click();
        harness.run();
        harness.get_by_label_contains("Open…").click();
        harness.run();
        assert_eq!(request(&mut harness), Request::Open);
    }

    #[test]
    fn the_title_names_the_song_and_marks_unsaved_changes() {
        let mut harness = editable_four_taps();
        assert_eq!(harness.state().title(), "four.json - charter");
        harness.key_press(egui::Key::Num4);
        harness.run();
        assert_eq!(harness.state().title(), "*four.json - charter");
        harness
            .state_mut()
            .saved(Saved::To(PathBuf::from("four.json")));
        harness.run();
        assert_eq!(harness.state().title(), "four.json - charter");
    }

    #[test]
    fn a_song_saved_elsewhere_is_saved_there_from_then_on() {
        let mut harness = editable_demo();
        harness
            .state_mut()
            .saved(Saved::To(PathBuf::from("mine.json")));
        harness.run();
        assert_eq!(harness.state().title(), "mine.json - charter");
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        let Request::Write { path, .. } = request(&mut harness) else {
            panic!("expected a write");
        };
        assert_eq!(path, Path::new("mine.json"));
    }

    #[test]
    fn a_failed_save_says_why_and_keeps_the_changes_unsaved() {
        let mut harness = editable_four_taps();
        harness.key_press(egui::Key::Num4);
        harness.run();
        harness
            .state_mut()
            .saved(Saved::Failed("the disk is full".to_owned()));
        harness.run();
        assert!(
            harness
                .query_by_label_contains("the disk is full")
                .is_some()
        );
        assert_eq!(harness.state().title(), "*four.json - charter");
    }

    /// The four taps with one more placed, so there is something to lose.
    fn edited_four_taps() -> Harness<'static, Window> {
        let mut harness = editable_four_taps();
        harness.key_press(egui::Key::Num4);
        harness.run();
        harness
    }

    /// Asks to close the window as its close button does, and runs the one frame that answers.
    fn close(harness: &mut Harness<'_, Window>) {
        let root = harness.input_mut().viewports.entry(egui::ViewportId::ROOT);
        root.or_default().events.push(egui::ViewportEvent::Close);
        harness.step();
    }

    /// Whether the last frame sent `command` to the host's window.
    fn sent(harness: &Harness<'_, Window>, command: &egui::ViewportCommand) -> bool {
        let output = harness
            .output()
            .viewport_output
            .get(&egui::ViewportId::ROOT);
        output.is_some_and(|viewport| viewport.commands.contains(command))
    }

    fn asks_to_save(harness: &Harness<'_, Window>) -> bool {
        harness
            .query_by_label("Save changes to four.json?")
            .is_some()
    }

    #[test]
    fn closing_with_unsaved_changes_asks_first() {
        let mut harness = edited_four_taps();
        close(&mut harness);
        assert!(sent(&harness, &egui::ViewportCommand::CancelClose));
        harness.run();
        assert!(asks_to_save(&harness));
    }

    #[test]
    fn closing_without_changes_does_not_ask() {
        let mut harness = editable_four_taps();
        close(&mut harness);
        assert!(!sent(&harness, &egui::ViewportCommand::CancelClose));
        harness.run();
        assert!(!asks_to_save(&harness));
    }

    #[test]
    fn dont_save_closes_and_lets_the_close_through() {
        let mut harness = edited_four_taps();
        close(&mut harness);
        harness.run();
        harness.get_by_label("Don't save").click();
        harness.step();
        harness.step();
        assert!(sent(&harness, &egui::ViewportCommand::Close));
        close(&mut harness);
        assert!(!sent(&harness, &egui::ViewportCommand::CancelClose));
    }

    #[test]
    fn save_writes_and_closes_once_the_host_says_it_saved() {
        let mut harness = edited_four_taps();
        close(&mut harness);
        harness.run();
        harness.get_by_label("Save").click();
        harness.run();
        let Request::Write { path, .. } = request(&mut harness) else {
            panic!("expected a write");
        };
        harness.state_mut().saved(Saved::To(path));
        harness.step();
        assert!(sent(&harness, &egui::ViewportCommand::Close));
    }

    #[test]
    fn cancel_keeps_the_window_and_its_changes() {
        let mut harness = edited_four_taps();
        close(&mut harness);
        harness.run();
        harness.get_by_label("Cancel").click();
        harness.run();
        assert!(!asks_to_save(&harness));
        assert!(!sent(&harness, &egui::ViewportCommand::Close));
        assert_eq!(harness.state().title(), "*four.json - charter");
    }

    #[test]
    fn opening_another_song_over_unsaved_changes_asks_first() {
        let mut harness = edited_four_taps();
        harness
            .state_mut()
            .open(Path::new("example.json"), example());
        harness.run();
        assert!(asks_to_save(&harness));
        assert_eq!(harness.state().title(), "*four.json - charter");
        harness.get_by_label("Don't save").click();
        harness.run();
        assert_eq!(harness.state().title(), "example.json - charter");
    }

    #[test]
    fn a_save_that_fails_leaves_everything_as_it_was() {
        let mut harness = edited_four_taps();
        harness.get_by_label("File").click();
        harness.run();
        harness.get_by_label("Open the demo song").click();
        harness.run();
        harness.get_by_label("Save").click();
        harness.run();
        assert!(matches!(request(&mut harness), Request::Write { .. }));
        harness
            .state_mut()
            .saved(Saved::Failed("the disk is full".to_owned()));
        harness.run();
        assert_eq!(harness.state().title(), "*four.json - charter");
        assert!(!asks_to_save(&harness));
    }

    #[test]
    fn a_ctrl_drag_draws_its_hold_while_a_message_comes_and_goes() {
        let mut harness = editable_four_taps();
        harness
            .state_mut()
            .refuse(Path::new("other.json"), &"it is not a song");
        harness.run();
        let (from, to) = (spot(&harness, 3, 240, 0.0), spot(&harness, 3, 720, -3.0));
        drag(&mut harness, from, to, egui::Modifiers::COMMAND);
        assert_eq!(note_at(&harness, 240, 3), Some(hold(480)));
    }

    #[test]
    fn a_hold_reaches_where_the_drag_lets_go() {
        let mut harness = editable_four_taps();
        let (from, near, far) = (
            spot(&harness, 3, 240, 0.0),
            spot(&harness, 3, 480, 0.0),
            spot(&harness, 3, 960, 0.0),
        );
        harness.hover_at(from);
        harness.event(egui::Event::ModifiersChanged(egui::Modifiers::COMMAND));
        press(&harness, from, true, egui::Modifiers::COMMAND);
        harness.hover_at(from.lerp(near, 0.5));
        harness.hover_at(near);
        press(&harness, far, false, egui::Modifiers::COMMAND);
        harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        harness.run();
        assert_eq!(note_at(&harness, 240, 3), Some(hold(720)));
    }

    #[test]
    fn escape_during_a_box_selection_cancels_it() {
        let mut harness = editable_four_taps();
        let (from, to) = (spot(&harness, 1, 180, 0.0), spot(&harness, 2, 540, 0.0));
        harness.hover_at(from);
        press(&harness, from, true, egui::Modifiers::NONE);
        harness.hover_at(from.lerp(to, 0.5));
        harness.hover_at(to);
        harness.key_press(egui::Key::Escape);
        press(&harness, to, false, egui::Modifiers::NONE);
        harness.run();
        assert!(harness.query_by_label("Nothing selected").is_some());
    }

    #[test]
    fn a_save_holds_every_edit_made_in_its_frame() {
        let mut harness = editable_four_taps();
        let key = |key, pressed, modifiers| egui::Event::Key {
            key,
            pressed,
            repeat: false,
            modifiers,
            physical_key: None,
        };
        let command = egui::Modifiers::COMMAND;
        let events = &mut harness.input_mut().events;
        events.push(key(egui::Key::S, true, command));
        events.push(key(egui::Key::S, false, command));
        events.push(key(egui::Key::Num4, true, egui::Modifiers::NONE));
        events.push(key(egui::Key::Num4, false, egui::Modifiers::NONE));
        harness.run();
        let Request::Write { path, bytes } = request(&mut harness) else {
            panic!("expected a write");
        };
        assert!(holds_tap(&bytes, 0, 3));
        harness.state_mut().saved(Saved::To(path));
        assert_eq!(harness.state().title(), "four.json - charter");
    }

    #[test]
    fn a_lane_key_held_across_a_chart_switch_leaves_the_other_chart_alone() {
        let mut harness = editable_demo();
        harness.key_down(egui::Key::Num1);
        harness.run();
        harness.get_by_label("Hard").click();
        harness.run();
        harness.key_press(egui::Key::ArrowDown);
        harness.run();
        assert_eq!(note_at(&harness, 0, 0), Some(tap()));
    }

    #[test]
    fn a_lane_key_on_a_note_removes_it_and_leaves_the_cursor_there() {
        let mut harness = editable_demo();
        harness.key_press(egui::Key::Num4);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), Vec::<u16>::new());
        assert!(cursor_shows(&harness, "0:0:0"));
    }

    #[test]
    fn lane_keys_leave_a_read_only_song_alone() {
        let mut harness = with_demo();
        harness.key_press(egui::Key::Num1);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [3]);
        assert!(harness.query_by_label_contains("Refused").is_none());
    }

    #[test]
    fn a_key_past_the_last_lane_does_nothing() {
        let mut harness = editable_demo();
        harness.key_press(egui::Key::Num5);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [3]);
        assert!(cursor_shows(&harness, "0:0:0"));
        assert!(harness.query_by_label_contains("Refused").is_none());
    }

    #[test]
    fn an_edit_the_game_refuses_says_why() {
        const PICKY: &[&dyn RulesetPlugin] = &[&NoFourthLane];
        let small = serde_json::json!({
            "format_version": 0,
            "ruleset": { "id": "sm", "version": "1" },
            "metadata": { "title": "Small", "artist": "a", "audio": "a.ogg" },
            "timing": { "offset_seconds": 0.0, "events": [ { "tick": 0, "kind": { "bpm": 120.0 } } ] },
            "charts": [ { "name": "Easy", "lanes": 4 } ]
        });
        let mut app = Window::new(PICKY);
        app.open(
            Path::new("small.json"),
            &serde_json::to_vec(&small).unwrap(),
        );
        let mut harness = window(app);
        harness.key_press(egui::Key::Num4);
        harness.run();
        assert!(
            harness
                .query_by_label("Refused: a note on lane 4")
                .is_some()
        );
        assert_eq!(lanes_at(&harness, 0), Vec::<u16>::new());
    }

    #[test]
    fn ctrl_z_takes_an_edit_back_and_ctrl_shift_z_or_ctrl_y_makes_it_again() {
        let mut harness = editable_demo();
        let command = egui::Modifiers::COMMAND;
        harness.key_press(egui::Key::Num1);
        harness.run();
        harness.key_press_modifiers(command, egui::Key::Z);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [3]);
        assert!(cursor_shows(&harness, "0:0:0"));
        harness.key_press_modifiers(command.plus(egui::Modifiers::SHIFT), egui::Key::Z);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [0, 3]);
        harness.key_press_modifiers(command, egui::Key::Z);
        harness.run();
        harness.key_press_modifiers(command, egui::Key::Y);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [0, 3]);
    }

    #[test]
    fn the_edit_menu_undoes_and_redoes_what_it_can() {
        let mut harness = editable_demo();
        harness.get_by_label("Edit").click();
        harness.run();
        let disabled = |harness: &Harness<'_, Window>, item: &str| {
            harness
                .get_by_label_contains(item)
                .accesskit_node()
                .is_disabled()
        };
        assert!(disabled(&harness, "Undo") && disabled(&harness, "Redo"));
        harness.key_press(egui::Key::Escape);
        harness.key_press(egui::Key::Num1);
        harness.run();
        harness.get_by_label("Edit").click();
        harness.run();
        assert!(!disabled(&harness, "Undo"));
        harness.get_by_label_contains("Undo").click();
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [3]);
        harness.get_by_label("Edit").click();
        harness.run();
        harness.get_by_label_contains("Redo").click();
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [0, 3]);
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
        app.open(Path::new("example.json"), example());
        let harness = window(app);
        assert!(harness.query_all_by_label("Future").next().is_some());
        assert!(harness.query_all_by_label("Beyond").next().is_some());
    }

    #[test]
    fn a_chart_says_what_it_cannot_draw_without_its_ruleset() {
        let mut app = Window::default();
        app.open(Path::new("example.json"), example());
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
        app.open(Path::new("broken.json"), broken.as_bytes());
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
