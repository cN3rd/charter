//! The panels around the notefield: the menu bar, the song panel, the inspector and the status
//! bar, and what the window shows with no song open.

use std::num::NonZeroU16;

use charter_core::{Bpm, MarkerKind, Object, Position, ResolvedTime, Shape, Tick};
use egui::{self, Align, Layout, RichText, Ui};
use serde_json::Value;

use charter_editor::{Song, Step};

use crate::Action;
use crate::keys::{HELP, NEW_SONG, OPEN, REDO, SAVE, SAVE_AS, SETTINGS, UNDO};
use crate::notefield::Zoom;
use crate::style::{fields, heading, table};
use crate::window::Rate;

#[allow(clippy::too_many_lines)] // the menus in the order they show
pub fn menu_bar(ui: &mut Ui, song: Option<&Song>, playing: bool, actions: &mut Vec<Action>) {
    let song_open = song.is_some();
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("File", |ui| {
            let items = [
                ("New song…", NEW_SONG, true, Action::NewSong),
                ("Open…", OPEN, true, Action::Open),
                ("Save", SAVE, song_open, Action::Save),
                ("Save as…", SAVE_AS, song_open, Action::SaveAs),
            ];
            for (label, shortcut, enabled, action) in items {
                let item =
                    egui::Button::new(label).shortcut_text(ui.ctx().format_shortcut(&shortcut));
                if ui.add_enabled(enabled, item).clicked() {
                    actions.push(action);
                }
            }
            ui.separator();
            if ui.button("Open the demo song").clicked() {
                actions.push(Action::OpenDemo);
            }
            ui.separator();
            let item =
                egui::Button::new("Settings…").shortcut_text(ui.ctx().format_shortcut(&SETTINGS));
            if ui.add(item).clicked() {
                actions.push(Action::Settings);
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
                    ("Delete the selection", "Delete", Action::DeleteSelection),
                ],
            );
        });
        ui.menu_button("Timing", |ui| {
            menu_items(
                ui,
                song_open,
                actions,
                [
                    ("Raise the tempo by 1 BPM", "F8", Action::NudgeBpm(1.0)),
                    ("Lower the tempo by 1 BPM", "F7", Action::NudgeBpm(-1.0)),
                    ("Add or remove a bookmark", "Ctrl+B", Action::ToggleBookmark),
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
        });
        ui.menu_button("View", |ui| {
            let items = [
                ("Zoom in", Action::ZoomBy(Zoom::STEP)),
                ("Zoom out", Action::ZoomBy(Zoom::STEP.recip())),
                ("Finer snap", Action::FinerSnap),
                ("Coarser snap", Action::CoarserSnap),
            ];
            for (label, action) in items {
                if ui
                    .add_enabled(song_open, egui::Button::new(label))
                    .clicked()
                {
                    actions.push(action);
                }
            }
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
                    ("Metronome on or off", "F3", Action::ToggleMetronome),
                    ("Note ticks on or off", "F4", Action::ToggleTicks),
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

/// The keyboard shortcuts, grouped, while `open` is set, showing those that match `filter`.
pub fn shortcuts(ctx: &egui::Context, open: &mut bool, filter: &mut String) {
    egui::Window::new("Keyboard shortcuts")
        .open(open)
        .collapsible(false)
        .resizable(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let caption = ui.weak("Filter");
                ui.add(egui::TextEdit::singleline(filter).hint_text("a key or what it does"))
                    .labelled_by(caption.id);
            });
            let wanted = filter.trim().to_lowercase();
            let shown = |entry: &&(&str, &str)| {
                wanted.is_empty()
                    || entry.0.to_lowercase().contains(&wanted)
                    || entry.1.to_lowercase().contains(&wanted)
            };
            egui::ScrollArea::vertical()
                .max_height(480.0)
                .show(ui, |ui| {
                    for (group, keys) in KEYS {
                        let keys: Vec<_> = keys.iter().filter(shown).collect();
                        if keys.is_empty() {
                            continue;
                        }
                        heading(ui, group);
                        table(ui, group, |ui| {
                            for (keys, does) in keys {
                                ui.monospace(*keys);
                                ui.label(*does);
                                ui.end_row();
                            }
                        });
                    }
                });
        });
}

/// The keys and gestures the editor answers to, grouped, and what each does.
const KEYS: [(&str, &[(&str, &str)]); 6] = [
    (
        "Note entry",
        &[
            ("1 to 9, 0", "Place or remove a tap on lanes 1 to 10"),
            ("Hold 1 to 0, move", "Draw a hold"),
            ("Shift+1 to 0", "Place or remove a mine"),
            ("Ctrl+Click", "Place or remove a tap"),
            ("Ctrl+Shift+Click", "Place or remove a mine"),
            ("Ctrl+Drag", "Draw a hold"),
        ],
    ),
    (
        "Selection",
        &[
            ("Click, Drag", "Select notes"),
            ("Shift+Click, Shift+Drag", "Add to the selection"),
            ("Alt+Click, Alt+Drag", "Take from the selection"),
            ("Ctrl+A, Escape", "Select every note, or none"),
            ("Delete, Backspace", "Delete the selection"),
            ("Ctrl+Z", "Undo"),
            ("Ctrl+Shift+Z, Ctrl+Y", "Redo"),
        ],
    ),
    (
        "Moving around",
        &[
            ("Up, Down, Wheel", "Move the cursor by the snap"),
            ("Page Up, Page Down", "Move the cursor by a measure"),
            (
                "Home, End",
                "Jump to the start, or to where the last note ends",
            ),
            ("Alt+Up, Alt+Down", "Jump to the previous or next section"),
            ("Left, Right", "Coarser or finer snap"),
            ("Ctrl+Wheel, Ctrl+Plus, Ctrl+Minus", "Zoom"),
        ],
    ),
    (
        "Timing",
        &[
            ("Ctrl+B", "Add or remove a bookmark at the cursor"),
            ("F7, F8", "Lower or raise the tempo at the cursor by 1 BPM"),
        ],
    ),
    (
        "Playback",
        &[
            ("Space", "Play from the cursor, or pause"),
            ("Shift+Left, Shift+Right", "Play slower or faster by 10%"),
            (
                "Ctrl+Shift+Left, Ctrl+Shift+Right",
                "Play slower or faster by 1%",
            ),
            ("F3, F4", "Turn the metronome or the note ticks on or off"),
        ],
    ),
    (
        "Files and windows",
        &[
            ("Ctrl+N", "New song"),
            ("Ctrl+O, Ctrl+S, Ctrl+Shift+S", "Open, save, save as"),
            ("Ctrl+Comma", "Settings"),
            ("F1", "Keyboard shortcuts"),
        ],
    ),
];

pub fn song_panel(ui: &mut Ui, song: &Song, actions: &mut Vec<Action>) {
    let document = song.document().document();
    let metadata = &document.metadata;
    heading(ui, "Song");
    let preview = metadata.preview.map_or_else(
        || "none".to_owned(),
        |preview| match preview.length_seconds {
            Some(length) => format!("{} s for {length} s", preview.start_seconds),
            None => format!("{} s", preview.start_seconds),
        },
    );
    fields(
        ui,
        "song",
        &[
            ("Title", metadata.title.clone()),
            ("Artist", metadata.artist.clone()),
            ("Audio", metadata.audio.to_string()),
            ("Preview", preview),
            (
                "Ruleset",
                format!("{} {}", document.ruleset.id, document.ruleset.version),
            ),
        ],
        "",
    );
    heading(ui, "Charts");
    if document.charts.is_empty() {
        ui.weak("No charts yet");
    }
    table(ui, "charts", |ui| {
        for (index, chart) in document.charts.iter().enumerate() {
            let open = index == song.chart_index();
            if ui.selectable_label(open, &chart.name).clicked() {
                actions.push(Action::OpenChart(index));
            }
            ui.weak(
                chart
                    .lanes
                    .map_or_else(|| "no lanes".to_owned(), |lanes| format!("{lanes} lanes")),
            );
            ui.end_row();
        }
    });
}

/// What is read more than changed while charting: the open chart's settings and the song's
/// keysound bank, folded away until wanted.
pub fn details(ui: &mut Ui, song: &Song) {
    if let Some(chart) = song.chart() {
        egui::CollapsingHeader::new("Chart settings").show(ui, |ui| {
            let settings: Vec<_> = chart
                .settings
                .iter()
                .map(|(key, value)| (key.as_str(), shown(value)))
                .collect();
            fields(ui, "settings", &settings, "No chart settings");
        });
    }
    egui::CollapsingHeader::new("Samples").show(ui, |ui| {
        let samples: Vec<_> = song
            .document()
            .document()
            .samples
            .iter()
            .map(|(id, path)| (id.as_str(), path.to_string()))
            .collect();
        fields(ui, "samples", &samples, "No samples");
    });
}

/// The song's markers, each of which jumps the cursor to it when clicked. In a song open for
/// editing a name typed and entered names the section at the cursor, starting one there if
/// there is none, and each marker can be removed.
pub fn markers(ui: &mut Ui, song: &Song, draft: &mut Option<String>, actions: &mut Vec<Action>) {
    heading(ui, "Markers");
    let editable = song.read_only().is_none();
    if editable {
        table(ui, "section here", |ui| {
            let caption = ui.weak("Section here");
            let mut text = draft
                .take()
                .unwrap_or_else(|| song.section().unwrap_or_default().to_owned());
            let field = ui
                .add(egui::TextEdit::singleline(&mut text).hint_text("none"))
                .labelled_by(caption.id);
            let entered =
                field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            if field.has_focus() {
                *draft = Some(text);
            } else if entered {
                actions.push(Action::SetSection(text.trim().to_owned()));
            }
            ui.end_row();
        });
    }
    let document = song.document().document();
    if document.markers.is_empty() {
        ui.weak("No markers");
        return;
    }
    let timing = song.timing();
    table(ui, "markers", |ui| {
        for (index, marker) in document.markers.iter().enumerate() {
            ui.monospace(timing.grid_position(marker.tick).to_string());
            let kind = match &marker.kind {
                MarkerKind::Section => "section".to_owned(),
                MarkerKind::Bookmark => "bookmark".to_owned(),
                MarkerKind::Other(key) => key.to_string(),
            };
            ui.weak(kind);
            let text = if marker.text.is_empty() {
                "(unnamed)"
            } else {
                marker.text.as_str()
            };
            let here = marker.tick == song.cursor();
            if ui.selectable_label(here, text).clicked() {
                actions.push(Action::JumpTo(marker.tick));
            }
            if editable {
                let remove = ui
                    .small_button("Remove")
                    .on_hover_text("Remove this marker; Ctrl+Z brings it back");
                let label = format!("Remove marker {text}");
                remove.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label)
                });
                if remove.clicked() {
                    actions.push(Action::RemoveMarker(index));
                }
            }
            ui.end_row();
        }
    });
}

/// A setting's value as text: strings bare, anything else as JSON.
fn shown(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

/// What the selected notes are: their count, then each field they share, or `mixed` where they
/// differ.
pub fn inspector(ui: &mut Ui, song: &Song) {
    heading(ui, "Inspector");
    let Some(chart) = song.chart() else {
        ui.weak("Nothing selected");
        return;
    };
    let selected: Vec<&Object> = chart
        .objects
        .iter()
        .filter(|object| song.is_selected(object))
        .collect();
    if selected.is_empty() {
        ui.weak("Nothing selected");
        ui.weak("Click or drag over notes to select them; Delete removes them.");
        return;
    }
    let count = match selected.len() {
        1 => "1 note".to_owned(),
        n => format!("{n} notes"),
    };
    ui.label(count);
    let timing = chart.timing();
    let shared = |field: &dyn Fn(&Object) -> String| {
        let mut values = selected.iter().map(|object| field(object));
        let first = values.next().unwrap_or_default();
        if values.all(|value| value == first) {
            first
        } else {
            "mixed".to_owned()
        }
    };
    let lane = |object: &Object| match object.position {
        Position::Lane { index, .. } => (u16::from(index) + 1).to_string(),
        Position::Plane { .. } => "plane".to_owned(),
    };
    let length = |object: &Object| match &object.shape {
        Shape::Instant => "instant".to_owned(),
        Shape::Sustain { duration } => format!("{duration} ticks"),
        Shape::Path { .. } => "path".to_owned(),
    };
    fields(
        ui,
        "inspector",
        &[
            (
                "At",
                shared(&|object| timing.grid_position(object.tick).to_string()),
            ),
            ("Lane", shared(&lane)),
            ("Kind", shared(&|object| object.kind.to_string())),
            ("Length", shared(&length)),
        ],
        "",
    );
}

/// The tempo at the cursor. In a song open for editing a BPM typed and entered sets the tempo
/// from the cursor on, and the change at the cursor can be removed. Says why a BPM typed is not
/// one.
pub fn tempo(
    ui: &mut Ui,
    song: &Song,
    draft: &mut Option<String>,
    actions: &mut Vec<Action>,
) -> Option<String> {
    heading(ui, "Tempo");
    let Some((at, bpm)) = song.bpm() else {
        ui.weak("No BPM");
        return None;
    };
    let editable = song.read_only().is_none();
    let mut refused = None;
    table(ui, "tempo", |ui| {
        let caption = ui.weak("BPM");
        if editable {
            let mut text = draft.take().unwrap_or_else(|| bpm.to_string());
            let field = ui
                .add(egui::TextEdit::singleline(&mut text).desired_width(72.0))
                .labelled_by(caption.id);
            let entered =
                field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            if field.has_focus() {
                *draft = Some(text);
            } else if entered {
                let typed = text.trim();
                match typed
                    .parse::<f64>()
                    .map_err(|_| ())
                    .and_then(|bpm| Bpm::try_from(bpm).map_err(|_| ()))
                {
                    Ok(bpm) => actions.push(Action::SetBpm(bpm)),
                    Err(()) => refused = Some(format!("{typed} is not a tempo")),
                }
            }
        } else {
            ui.label(bpm.to_string());
        }
        ui.weak(format!("since {}", song.timing().grid_position(at)));
        ui.end_row();
    });
    ui.add_space(4.0);
    if editable && at == song.cursor() && ui.small_button("Remove this BPM change").clicked() {
        actions.push(Action::RemoveBpm);
    }
    refused
}

pub fn status_bar(
    ui: &mut Ui,
    song: Option<&Song>,
    zoom: Zoom,
    rate: Rate,
    (audio, playing): (&str, bool),
) {
    ui.horizontal(|ui| {
        let Some(song) = song else {
            ui.weak("No song open");
            return;
        };
        let timing = song.timing();
        let at = timing.grid_position(song.cursor()).to_string();
        let cursor = ui.monospace(&at);
        cursor.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Label, true, format!("Cursor at {at}"))
        });
        ui.monospace(clock(timing.time_at(song.cursor())));
        ui.separator();
        if let Some((_, bpm)) = song.bpm() {
            ui.label(format!("{bpm} BPM"));
        }
        ui.label(format!("Snap {}", ordinal(song.snap())));
        ui.label(format!("Zoom {:.0}%", zoom.percent()));
        ui.separator();
        ui.label(if playing { "Playing" } else { "Paused" });
        ui.label(format!("Speed {:.0}%", rate.percent()));
        ui.weak(audio);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if song.read_only().is_some() {
                ui.weak("Read only");
            } else {
                ui.label("Editing");
            }
        });
    });
}

/// An audio time as minutes, seconds and milliseconds, such as `01:23.456`.
fn clock(time: ResolvedTime) -> String {
    let seconds = time.seconds();
    let sign = if seconds < 0.0 { "-" } else { "" };
    let millis = (seconds.abs() * 1000.0).round();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // whole, positive, small
    let millis = millis as u64;
    format!(
        "{sign}{:02}:{:02}.{:03}",
        millis / 60_000,
        millis / 1000 % 60,
        millis % 1000
    )
}

/// A snap's name, such as `16th` or `32nd`.
pub fn ordinal(n: NonZeroU16) -> String {
    let n = n.get();
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// What the centre shows with no song open.
pub fn empty(ui: &mut Ui, actions: &mut Vec<Action>) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() / 3.0);
        ui.heading("No song open");
        ui.label(
            "Drop a charter song (.json) on this window, or give its path when you start charter.",
        );
        ui.add_space(8.0);
        if ui.button("Open song…").clicked() {
            actions.push(Action::Open);
        }
        if ui.button("New song").clicked() {
            actions.push(Action::NewSong);
        }
        if ui.button("Open the demo song").clicked() {
            actions.push(Action::OpenDemo);
        }
    });
}

/// That the open song has no audio yet, and how to give it some.
pub fn no_audio(ui: &mut Ui, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        let colour = ui.visuals().warn_fg_color;
        let text = "No audio yet: drop an audio file (.ogg, .mp3, .wav, .flac) on the window, or";
        ui.label(RichText::new(text).color(colour));
        if ui.button("Choose audio…").clicked() {
            actions.push(Action::ChooseAudio);
        }
    });
}

/// What to do with unsaved changes that are about to be lost.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Save,
    DontSave,
    Cancel,
}

/// Asks whether to save the changes to the song in `name` before they are lost. Escape or a
/// click outside cancels.
pub fn ask_to_save(ui: &Ui, name: &str) -> Option<Answer> {
    let mut answer = None;
    let modal = egui::Modal::new(egui::Id::new("save changes")).show(ui.ctx(), |ui| {
        ui.set_max_width(360.0);
        ui.strong(format!("Save changes to {name}?"));
        ui.label("Changes are lost unless you save them.");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            for (label, choice) in [
                ("Save", Answer::Save),
                ("Don't save", Answer::DontSave),
                ("Cancel", Answer::Cancel),
            ] {
                if ui.button(label).clicked() {
                    answer = Some(choice);
                }
            }
        });
    });
    answer.or(modal.should_close().then_some(Answer::Cancel))
}

/// Asks whether to restore the unsaved changes to the song in `name` that an earlier run kept:
/// true to restore them, false to throw them away. Only an answer closes it.
pub fn ask_to_restore(ui: &Ui, name: &str) -> Option<bool> {
    let mut answer = None;
    egui::Modal::new(egui::Id::new("restore changes")).show(ui.ctx(), |ui| {
        ui.set_max_width(360.0);
        ui.strong(format!("Restore unsaved changes to {name}?"));
        ui.label("charter closed before they were saved.");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Restore").clicked() {
                answer = Some(true);
            }
            if ui.button("Discard").clicked() {
                answer = Some(false);
            }
        });
    });
    answer
}

/// Why the last file did not open, or the last edit was refused.
/// Says whether the user dismissed it.
pub fn failure(ui: &mut Ui, message: &str) -> bool {
    ui.horizontal(|ui| {
        let colour = ui.visuals().warn_fg_color;
        ui.label(RichText::new(message).color(colour));
        ui.small_button("Dismiss").clicked()
    })
    .inner
}

/// Why the open song cannot be edited.
pub fn read_only(ui: &mut Ui, why: &str) {
    ui.weak(format!("Read only: {why}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(n: u16) -> NonZeroU16 {
        NonZeroU16::new(n).unwrap()
    }

    #[test]
    fn snaps_read_as_ordinals() {
        let names: Vec<_> = [4, 12, 16, 32, 48, 192].map(|n| ordinal(note(n))).into();
        assert_eq!(names, ["4th", "12th", "16th", "32nd", "48th", "192nd"]);
    }
}
