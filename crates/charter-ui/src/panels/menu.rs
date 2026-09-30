use super::{
    Action, HELP, NEW_SONG, OPEN, REDO, SAVE, SAVE_AS, SETTINGS, Song, Step, Tick, UNDO, Ui, Zoom,
    egui,
};

/// The menu bar. `recent` is the songs opened before, the latest first.
#[expect(clippy::too_many_lines, reason = "menus read left to right")]
#[expect(
    clippy::non_ascii_literal,
    reason = "menu labels use ellipses for dialogs and a check mark for the selected option"
)]
pub(crate) fn menu_bar(
    ui: &mut Ui,
    (song, recent): (Option<&Song>, &[std::path::PathBuf]),
    (playing, importable, findable, colour_notes_by_layer): (bool, bool, bool, bool),
    (songs_importable, song_exportable): (bool, bool),
    actions: &mut Vec<Action>,
) {
    let song_open = song.is_some();
    let editable = song.is_some_and(|song| song.read_only().is_none());
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("File", |ui| {
            let item = |item_ui: &mut Ui, label, shortcut, enabled| {
                let item = egui::Button::new(label)
                    .shortcut_text(item_ui.ctx().format_shortcut(&shortcut));
                item_ui.add_enabled(enabled, item).clicked()
            };
            if item(ui, "New song…", NEW_SONG, true) {
                actions.push(Action::NewSong);
            }
            if item(ui, "Open…", OPEN, true) {
                actions.push(Action::Open);
            }
            open_recent(ui, recent, actions);
            if item(ui, "Save", SAVE, song_open) {
                actions.push(Action::Save);
            }
            if item(ui, "Save as…", SAVE_AS, song_open) {
                actions.push(Action::SaveAs);
            }
            ui.separator();
            let import = egui::Button::new("Import song…");
            if ui.add_enabled(songs_importable, import).clicked() {
                actions.push(Action::ImportSong);
            }
            let export = egui::Button::new("Export song…");
            if ui.add_enabled(song_exportable, export).clicked() {
                actions.push(Action::ExportSong);
            }
            ui.separator();
            let choose = egui::Button::new("Choose audio…");
            if ui.add_enabled(editable, choose).clicked() {
                actions.push(Action::ChooseAudio);
            }
            if ui
                .add_enabled(editable, egui::Button::new("Choose artwork…"))
                .clicked()
            {
                actions.push(Action::ChooseArtwork);
            }
        });
        ui.menu_button("Edit", |ui| {
            let items = [
                ("Undo", UNDO, song.is_some_and(Song::can_undo), Action::Undo),
                ("Redo", REDO, song.is_some_and(Song::can_redo), Action::Redo),
            ];
            for (label, shortcut, enabled, action) in items {
                let item =
                    egui::Button::new(label).shortcut_text(ui.ctx().format_shortcut(&shortcut));
                if ui.add_enabled(enabled, item).clicked() {
                    actions.push(action);
                }
            }
            ui.separator();
            menu_items(
                ui,
                song_open,
                actions,
                [
                    ("Select all", "Ctrl+A", Action::SelectAll),
                    ("Select none", "Escape", Action::ClearSelection),
                ],
            );
            menu_items(
                ui,
                editable,
                actions,
                [
                    ("Cut", "Ctrl+X", Action::Cut),
                    ("Copy", "Ctrl+C", Action::Copy),
                    (
                        "Paste at the cursor",
                        "Ctrl+V",
                        Action::PasteHere(song.map_or(Tick::new(0), Song::cursor)),
                    ),
                    ("Duplicate", "Ctrl+D", Action::Duplicate),
                    ("Delete the selection", "Delete", Action::DeleteSelection),
                    (
                        "Move a lane left",
                        "Ctrl+Left",
                        Action::ShiftSelection {
                            snaps: 0,
                            lanes: -1,
                        },
                    ),
                    (
                        "Move a lane right",
                        "Ctrl+Right",
                        Action::ShiftSelection { snaps: 0, lanes: 1 },
                    ),
                    ("Mirror across the lanes", "Ctrl+M", Action::Mirror),
                    ("Snap to the grid", "Q", Action::Quantize),
                    (
                        "Lengthen by the snap",
                        "]",
                        Action::ResizeNotes(Step::Later),
                    ),
                    (
                        "Shorten by the snap",
                        "[",
                        Action::ResizeNotes(Step::Earlier),
                    ),
                ],
            );
            ui.separator();
            let item =
                egui::Button::new("Settings…").shortcut_text(ui.ctx().format_shortcut(&SETTINGS));
            if ui.add(item).clicked() {
                actions.push(Action::Settings);
            }
        });
        ui.menu_button("Timing", |ui| {
            menu_items(
                ui,
                song_open,
                actions,
                [
                    ("Timing window", "Ctrl+T", Action::ToggleSync),
                    (
                        "Previous section",
                        "Alt+Up",
                        Action::JumpSection(Step::Earlier),
                    ),
                    ("Next section", "Alt+Down", Action::JumpSection(Step::Later)),
                    ("Jump to the start", "Home", Action::JumpTo(Tick::new(0))),
                    ("Jump to the end", "End", Action::JumpEnd),
                ],
            );
            menu_items(
                ui,
                editable,
                actions,
                [
                    ("Raise the tempo by 1 BPM", "F8", Action::NudgeBpm(1.0)),
                    ("Lower the tempo by 1 BPM", "F7", Action::NudgeBpm(-1.0)),
                    ("Add a timing point here", "", Action::AddBpm),
                    (
                        "Raise the offset by 1 ms",
                        "Alt+Right",
                        Action::NudgeOffset(0.001),
                    ),
                    (
                        "Lower the offset by 1 ms",
                        "Alt+Left",
                        Action::NudgeOffset(-0.001),
                    ),
                    ("Add or remove a bookmark", "Ctrl+B", Action::ToggleBookmark),
                ],
            );
            ui.separator();
            let import = egui::Button::new("Import timing…");
            if ui.add_enabled(editable && importable, import).clicked() {
                actions.push(Action::ShowImport);
            }
            let finders = [
                (
                    "Find timing with Beat This!…",
                    "Beat This! finds the beats; charter fits tempo sections to them",
                    crate::Tracker::BeatThis,
                ),
                (
                    "Find timing with Mapperatorinator…",
                    "Mapperatorinator's timing model writes the timing points itself; it runs on \
                     the graphics card and takes a minute or two",
                    crate::Tracker::Mapperatorinator,
                ),
            ];
            for (label, hover, tracker) in finders {
                if ui
                    .add_enabled(editable && findable, egui::Button::new(label))
                    .on_hover_text(hover)
                    .clicked()
                {
                    actions.push(Action::FindTiming(tracker));
                }
            }
            let reset = egui::Button::new("Reset timing…");
            if ui.add_enabled(editable, reset).clicked() {
                actions.push(Action::AskResetTiming);
            }
        });
        ui.menu_button("View", |ui| {
            let item = egui::Button::new("Song panel").shortcut_text("Ctrl+\\");
            if ui.add(item).clicked() {
                actions.push(Action::ToggleSongPanel);
            }
            let label = if colour_notes_by_layer {
                "✓ Colour notes by layer"
            } else {
                "Colour notes by layer"
            };
            if ui.button(label).clicked() {
                actions.push(Action::ToggleLayerColours);
            }
            ui.separator();
            menu_items(
                ui,
                song_open,
                actions,
                [
                    ("Zoom in", "Ctrl+Plus", Action::ZoomBy(Zoom::STEP)),
                    ("Zoom out", "Ctrl+Minus", Action::ZoomBy(Zoom::STEP.recip())),
                    ("Finer snap", "Right", Action::FinerSnap),
                    ("Coarser snap", "Left", Action::CoarserSnap),
                ],
            );
        });
        ui.menu_button("Playback", |ui| {
            let play = if playing { "Pause" } else { "Play" };
            let item = egui::Button::new(play).shortcut_text("Space");
            if ui.add_enabled(song_open, item).clicked() {
                actions.push(Action::TogglePlay);
            }
            ui.separator();
            menu_items(
                ui,
                true,
                actions,
                [
                    ("Faster", "Shift+Right", Action::NudgeRate(0.1)),
                    ("Slower", "Shift+Left", Action::NudgeRate(-0.1)),
                    ("Faster by 1%", "Ctrl+Shift+Right", Action::NudgeRate(0.01)),
                    ("Slower by 1%", "Ctrl+Shift+Left", Action::NudgeRate(-0.01)),
                    ("Metronome on or off", "F3", Action::ToggleMetronome),
                    ("Note ticks on or off", "F4", Action::ToggleTicks),
                    (
                        "Tuned lanes on or off",
                        "Shift+F4",
                        Action::ToggleTunedLanes,
                    ),
                    ("Calibrate latency…", "", Action::Calibrate),
                ],
            );
            for factor in [0.5, 0.75, 1.0] {
                if ui.button(format!("{:.0}% speed", factor * 100.0)).clicked() {
                    actions.push(Action::SetRate(factor));
                }
            }
        });
        ui.menu_button("Help", |ui| {
            let item = egui::Button::new("Keyboard shortcuts")
                .shortcut_text(ui.ctx().format_shortcut(&HELP));
            if ui.add(item).clicked() {
                actions.push(Action::Help);
            }
            if ui.button("Open the demo song").clicked() {
                actions.push(Action::OpenDemo);
            }
        });
    });
}

/// File's Open recent submenu: the songs opened before, each by its file's name with its folder
/// beside it, and a way to forget them.
pub(super) fn open_recent(ui: &mut Ui, recent: &[std::path::PathBuf], actions: &mut Vec<Action>) {
    ui.add_enabled_ui(!recent.is_empty(), |ui| {
        ui.menu_button("Open recent", |ui| {
            for path in recent {
                let name = path.file_name().map_or_else(
                    || path.display().to_string(),
                    |name| name.to_string_lossy().into_owned(),
                );
                let folder = path.parent().map(|folder| folder.display().to_string());
                let item = egui::Button::new(name).shortcut_text(folder.unwrap_or_default());
                if ui
                    .add(item)
                    .on_hover_text(path.display().to_string())
                    .clicked()
                {
                    actions.push(Action::OpenRecent(path.clone()));
                }
            }
            ui.separator();
            if ui.button("Clear recent songs").clicked() {
                actions.push(Action::ClearRecent);
            }
        });
    });
}

/// Menu items, each with the shortcut that does the same.
fn menu_items<const N: usize>(
    ui: &mut Ui,
    enabled: bool,
    actions: &mut Vec<Action>,
    items: [(&str, &str, Action); N],
) {
    for (label, shortcut, action) in items {
        let item = egui::Button::new(label).shortcut_text(shortcut);
        if ui.add_enabled(enabled, item).clicked() {
            actions.push(action);
        }
    }
}
