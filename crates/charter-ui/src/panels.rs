//! The panels around the notefield: the menu bar, the song panel, the inspector and the status
//! bar, and what the window shows with no song open.

use std::num::NonZeroU16;

use charter_core::{MarkerKind, Metadata, ResolvedTime, ResourcePath, SettingScope, Tick};
use egui::{self, Align, Layout, RichText, Ui};
use serde_json::Value;

use charter_editor::{Song, Step};

use crate::Action;
use crate::keys::{HELP, NEW_SONG, OPEN, REDO, SAVE, SAVE_AS, SETTINGS, UNDO};
use crate::notefield::Zoom;
use crate::style::{self, fields, heading, icon, table};
use crate::window::Rate;

#[allow(clippy::too_many_lines)] // the menus in the order they show
pub fn menu_bar(
    ui: &mut Ui,
    song: Option<&Song>,
    (playing, importable, findable): (bool, bool, bool),
    actions: &mut Vec<Action>,
) {
    let song_open = song.is_some();
    let editable = song.is_some_and(|song| song.read_only().is_none());
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("File", |ui| {
            let items = [
                ("New song…", NEW_SONG, true, Action::NewSong),
                ("Open…", OPEN, true, Action::Open),
                ("Save", SAVE, song_open, Action::Save),
                ("Save as…", SAVE_AS, song_open, Action::SaveAs),
            ];
            let choose = egui::Button::new("Choose audio…");
            for (label, shortcut, enabled, action) in items {
                let item =
                    egui::Button::new(label).shortcut_text(ui.ctx().format_shortcut(&shortcut));
                if ui.add_enabled(enabled, item).clicked() {
                    actions.push(action);
                }
            }
            if ui.add_enabled(editable, choose).clicked() {
                actions.push(Action::ChooseAudio);
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
                ],
            );
            menu_items(
                ui,
                editable,
                actions,
                [
                    ("Delete the selection", "Delete", Action::DeleteSelection),
                    (
                        "Lengthen by the snap",
                        "[",
                        Action::ResizeNotes(Step::Later),
                    ),
                    (
                        "Shorten by the snap",
                        "]",
                        Action::ResizeNotes(Step::Earlier),
                    ),
                ],
            );
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
            let find = egui::Button::new("Find timing from the audio…");
            if ui
                .add_enabled(editable && findable, find)
                .on_hover_text("Beat This! finds the beats; charter fits tempo sections to them")
                .clicked()
            {
                actions.push(Action::FindTiming);
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
            style::solid_scroll_bars(ui);
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
            (
                "[, ]",
                "Lengthen or shorten the selection, or the notes at the cursor, by the snap",
            ),
        ],
    ),
    (
        "Selection",
        &[
            ("Click, Drag", "Select notes"),
            ("Drag a note", "Move it, or the selection it is in"),
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
            ("Ctrl+T", "Show or hide the timing window"),
            ("Ctrl+B", "Add or remove a bookmark at the cursor"),
            ("F7, F8", "Lower or raise the tempo at the cursor by 1 BPM"),
            (
                "Alt+Left, Alt+Right",
                "Lower or raise the offset by 1 ms, or by 10 ms with Shift",
            ),
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
            ("Ctrl+\\", "Show or hide the song panel"),
            ("F1", "Keyboard shortcuts"),
        ],
    ),
];

pub fn song_panel(
    ui: &mut Ui,
    song: &Song,
    drafts: &mut SongDrafts,
    actions: &mut Vec<Action>,
) -> Option<String> {
    let document = song.document().document();
    let mut refused = None;
    style::section(ui, "Song", |ui| {
        table(ui, "song", |ui| {
            refused = song_rows(ui, song, drafts, actions);
        });
    });
    style::section(ui, "Charts", |ui| {
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
            let offered = setting_rows(ui, song, SettingScope::Chart, drafts, actions);
            refused = refused.take().or(offered);
        });
    });
    refused
}

/// What is being typed into the song section's fields, while one has the keyboard.
#[derive(Debug, Default)]
pub struct SongDrafts {
    title: Option<String>,
    artist: Option<String>,
    genre: Option<String>,
    artwork: Option<String>,
    /// The game's text settings, by whether they are the chart's, and key.
    settings: std::collections::HashMap<(bool, String), Option<String>>,
    preview: Option<String>,
    length: Option<String>,
}

/// How wide the song section's text fields are.
const WIDE: f32 = 180.0;

/// The song's title, artist, audio, preview and ruleset, each that can change a field of its
/// own. Says why something typed is not a value.
fn song_rows(
    ui: &mut Ui,
    song: &Song,
    drafts: &mut SongDrafts,
    actions: &mut Vec<Action>,
) -> Option<String> {
    let document = song.document().document();
    let metadata = &document.metadata;
    let editable = song.read_only().is_none();
    let changed = |change: &dyn Fn(&mut Metadata)| changed(metadata, change);
    let mut refused = None;
    let mut text_row = |ui: &mut Ui,
                        label: &str,
                        current: &str,
                        draft: &mut Option<String>,
                        set: fn(&mut Metadata, &str)| {
        let caption = ui.weak(label);
        if editable {
            let read = |typed: &str| Ok(changed(&|new| set(new, typed)));
            let offered = entry_sized(ui, (caption.id, WIDE), current, draft, read, actions);
            refused = refused.take().or(offered);
        } else {
            ui.label(current);
        }
        ui.end_row();
    };
    text_row(
        ui,
        "Title",
        &metadata.title,
        &mut drafts.title,
        |new, typed| {
            typed.clone_into(&mut new.title);
        },
    );
    text_row(
        ui,
        "Artist",
        &metadata.artist,
        &mut drafts.artist,
        |new, typed| {
            typed.clone_into(&mut new.artist);
        },
    );
    text_row(
        ui,
        "Genre",
        metadata.genre.as_deref().unwrap_or_default(),
        &mut drafts.genre,
        |new, typed| new.genre = (!typed.is_empty()).then(|| typed.to_owned()),
    );
    ui.weak("Audio");
    ui.horizontal(|ui| {
        ui.label(metadata.audio.as_str());
        if editable
            && style::quiet_icon(
                ui,
                icon::FOLDER_OPEN,
                "Choose the song's audio",
                "Choose another audio file",
            )
            .clicked()
        {
            actions.push(Action::ChooseAudio);
        }
    });
    ui.end_row();
    let caption = ui
        .weak("Artwork")
        .on_hover_text("The song's picture, a file beside the song, such as artwork.png");
    let artwork = metadata.artwork.as_ref().map(ToString::to_string);
    let artwork = artwork.as_deref().unwrap_or_default();
    if editable {
        let read = |typed: &str| {
            if typed.is_empty() {
                return Ok(changed(&|new| new.artwork = None));
            }
            let path = ResourcePath::new(typed)
                .map_err(|_| format!("{typed} is not a file beside the song"))?;
            Ok(changed(&|new| new.artwork = Some(path.clone())))
        };
        let offered = entry_sized(
            ui,
            (caption.id, WIDE),
            artwork,
            &mut drafts.artwork,
            read,
            actions,
        );
        refused = refused.take().or(offered);
    } else {
        ui.label(if artwork.is_empty() { "none" } else { artwork });
    }
    ui.end_row();
    let offered = preview_rows(ui, (metadata, editable), drafts, actions);
    refused = refused.take().or(offered);
    ui.weak("Ruleset");
    ui.label(format!(
        "{} {}",
        document.ruleset.id, document.ruleset.version
    ));
    ui.end_row();
    let offered = setting_rows(ui, song, SettingScope::Song, drafts, actions);
    refused.take().or(offered)
}

/// A field for each of the game's text settings in `scope`, the open chart's for a chart scope,
/// each clearing its key when cleared. Says why something typed is not a value.
fn setting_rows(
    ui: &mut Ui,
    song: &Song,
    scope: SettingScope,
    drafts: &mut SongDrafts,
    actions: &mut Vec<Action>,
) -> Option<String> {
    let plugin = song.plugin()?;
    let document = song.document().document();
    let settings = match scope {
        SettingScope::Song => &document.settings,
        SettingScope::Chart => &document.charts.get(song.chart_index())?.settings,
    };
    let mut refused = None;
    for setting in plugin
        .text_settings()
        .into_iter()
        .filter(|setting| setting.scope == scope)
    {
        let name = match scope {
            SettingScope::Song => setting.name.clone(),
            SettingScope::Chart => format!("Chart {}", setting.name.to_lowercase()),
        };
        let caption = ui.weak(&name).on_hover_text(&setting.about);
        let current = settings
            .get(&setting.key)
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let draft = drafts
            .settings
            .entry((scope == SettingScope::Chart, setting.key.clone()))
            .or_default();
        let read = |typed: &str| {
            Ok(Action::SetSetting {
                scope,
                key: setting.key.clone(),
                value: (!typed.is_empty()).then(|| typed.to_owned()),
            })
        };
        let offered = entry_sized(ui, (caption.id, WIDE), current, draft, read, actions);
        refused = refused.take().or(offered);
        ui.end_row();
    }
    refused
}

/// `metadata` changed by `change`, as the action that gives the song it.
fn changed(metadata: &Metadata, change: &dyn Fn(&mut Metadata)) -> Action {
    let mut new = metadata.clone();
    change(&mut new);
    Action::SetMetadata(Box::new(new))
}

/// Where a song list starts playing the song, and for how long once it has a start. Clearing the
/// start takes the preview away. Says why something typed is not a value.
fn preview_rows(
    ui: &mut Ui,
    (metadata, editable): (&Metadata, bool),
    drafts: &mut SongDrafts,
    actions: &mut Vec<Action>,
) -> Option<String> {
    let changed = |change: &dyn Fn(&mut Metadata)| changed(metadata, change);
    let mut refused = None;
    let caption = ui
        .weak("Preview")
        .on_hover_text("Where a song list starts playing the song. Clear it for none.");
    let start = metadata.preview.map_or_else(String::new, |preview| {
        style::clock(ResolvedTime::from(preview.start_seconds).seconds())
    });
    if editable {
        let read = |typed: &str| {
            if typed.is_empty() {
                return Ok(changed(&|new| new.preview = None));
            }
            let start = crate::timing_panel::moment(typed)
                .and_then(|at| charter_core::AudioTime::try_from((at * 1e6).round() / 1e6).ok())
                .ok_or_else(|| format!("{typed} is not a time in the audio, such as 00:45.000"))?;
            Ok(changed(&|new| {
                let length_seconds = new.preview.and_then(|preview| preview.length_seconds);
                new.preview = Some(charter_core::Preview {
                    start_seconds: start,
                    length_seconds,
                });
            }))
        };
        let offered = entry_sized(
            ui,
            (caption.id, 72.0),
            &start,
            &mut drafts.preview,
            read,
            actions,
        );
        refused = refused.take().or(offered);
    } else {
        ui.label(if start.is_empty() {
            "none".to_owned()
        } else {
            start
        });
    }
    ui.end_row();
    if let Some(preview) = metadata.preview {
        let caption = ui.weak("Preview length");
        let length = preview
            .length_seconds
            .map_or_else(String::new, |length| length.to_string());
        if editable {
            ui.horizontal(|ui| {
                let read = |typed: &str| {
                    if typed.is_empty() {
                        return Ok(changed(&|new| {
                            new.preview = Some(charter_core::Preview {
                                length_seconds: None,
                                ..preview
                            });
                        }));
                    }
                    let length = typed
                        .parse::<f64>()
                        .ok()
                        .and_then(|seconds| charter_core::Seconds::try_from(seconds).ok())
                        .ok_or_else(|| format!("{typed} is not a length in seconds"))?;
                    Ok(changed(&|new| {
                        new.preview = Some(charter_core::Preview {
                            length_seconds: Some(length),
                            ..preview
                        });
                    }))
                };
                let offered = entry_sized(
                    ui,
                    (caption.id, 72.0),
                    &length,
                    &mut drafts.length,
                    read,
                    actions,
                );
                refused = refused.take().or(offered);
                ui.weak("s");
            });
        } else {
            ui.label(if length.is_empty() {
                "to the end".to_owned()
            } else {
                format!("{length} s")
            });
        }
        ui.end_row();
    }
    refused
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
    style::section(ui, "Markers", |ui| markers_body(ui, song, draft, actions));
}

fn markers_body(ui: &mut Ui, song: &Song, draft: &mut Option<String>, actions: &mut Vec<Action>) {
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
            let at = timing.grid_position(marker.tick).to_string();
            ui.label(RichText::new(at).monospace().color(style::WEAK_TEXT));
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
                let remove = style::quiet_icon(
                    ui,
                    style::icon::TRASH,
                    &format!("Remove marker {text}"),
                    "Remove this marker; Ctrl+Z brings it back",
                );
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

/// A value typed into a field named by `caption`, showing `current` until typing starts and
/// kept in `draft` while the field has the keyboard. Entered, `read` turns it into an action,
/// or into why it is not a value; Escape leaves it.
pub(crate) fn entry(
    ui: &mut Ui,
    caption: egui::Id,
    current: &str,
    draft: &mut Option<String>,
    read: impl FnOnce(&str) -> Result<Action, String>,
    actions: &mut Vec<Action>,
) -> Option<String> {
    entry_sized(ui, (caption, 72.0), current, draft, read, actions)
}

/// [`entry`] `width` points wide.
pub(crate) fn entry_sized(
    ui: &mut Ui,
    (caption, width): (egui::Id, f32),
    current: &str,
    draft: &mut Option<String>,
    read: impl FnOnce(&str) -> Result<Action, String>,
    actions: &mut Vec<Action>,
) -> Option<String> {
    let mut text = draft.take().unwrap_or_else(|| current.to_owned());
    let empty = current.is_empty();
    let field = egui::TextEdit::singleline(&mut text).desired_width(width);
    let field = if empty {
        field.hint_text("none")
    } else {
        field
    };
    let field = ui.add(field).labelled_by(caption);
    // Leaving the field takes what was typed, as Enter does; only Escape drops it. A field left
    // as it was does nothing.
    let left = field.lost_focus()
        && !ui.input(|input| input.key_pressed(egui::Key::Escape))
        && text.trim() != current;
    if field.has_focus() {
        *draft = Some(text);
    } else if left {
        match read(text.trim()) {
            Ok(action) => actions.push(action),
            Err(why) => return Some(why),
        }
    }
    None
}

#[allow(clippy::too_many_lines)] // the toolbar's groups in the order they show
pub fn transport_bar(
    ui: &mut Ui,
    song: Option<&Song>,
    (zoom, rate, playing, panel, grid): (Zoom, Rate, bool, bool, NonZeroU16),
    (metronome, ticks, volumes): (bool, bool, charter_audio::Volumes),
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let (name, tip) = if panel {
            ("Hide the song panel", "Hide the song panel (Ctrl+\\)")
        } else {
            ("Show the song panel", "Show the song panel (Ctrl+\\)")
        };
        if style::tool(ui, icon::SIDEBAR_SIMPLE, name, tip, panel).clicked() {
            actions.push(Action::ToggleSongPanel);
        }
        ui.add_enabled_ui(song.is_some(), |ui| {
            style::chip(ui, |ui| {
                let start = Action::JumpTo(Tick::new(0));
                if style::tool(ui, icon::SKIP_BACK, "Jump to the start", "Home", false).clicked() {
                    actions.push(start);
                }
                let (symbol, name) = if playing {
                    (icon::PAUSE, "Pause")
                } else {
                    (icon::PLAY, "Play")
                };
                let tip = format!("{name} from the cursor (Space)");
                if style::tool(ui, symbol, name, &tip, playing).clicked() {
                    actions.push(Action::TogglePlay);
                }
                if style::tool(ui, icon::SKIP_FORWARD, "Jump to the end", "End", false).clicked() {
                    actions.push(Action::JumpEnd);
                }
            });
        });
        if let Some(song) = song {
            readout(ui, song);
            style::chip(ui, |ui| {
                ui.label(style::glyph(ui, icon::METRONOME).color(style::WEAK_TEXT));
                let tempo = song
                    .bpm()
                    .map_or_else(|| "no BPM".to_owned(), |(_, bpm)| format!("{bpm} BPM"));
                ui.label(tempo);
            });
            style::chip(ui, |ui| {
                ui.label(style::glyph(ui, icon::GRID_FOUR).color(style::WEAK_TEXT));
                if style::quiet_icon(ui, icon::CARET_LEFT, "Coarser snap", "Coarser snap (Left)")
                    .clicked()
                {
                    actions.push(Action::CoarserSnap);
                }
                ui.label(format!("Snap {}", ordinal(song.snap())));
                if style::quiet_icon(ui, icon::CARET_RIGHT, "Finer snap", "Finer snap (Right)")
                    .clicked()
                {
                    actions.push(Action::FinerSnap);
                }
                let mut chosen = grid;
                let caption = ui.weak("Grid");
                crate::settings::grid_choice(ui, &mut chosen, "toolbar grid lines")
                    .on_hover_text("Lines between the beats down to this note")
                    .labelled_by(caption.id)
                    .widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, "Grid lines")
                    });
                if chosen != grid {
                    actions.push(Action::SetGrid(chosen));
                }
            });
            style::chip(ui, |ui| {
                ui.label(style::glyph(ui, icon::GAUGE).color(style::WEAK_TEXT));
                if style::quiet_icon(ui, icon::MINUS, "Slower", "Play slower (Shift+Left)")
                    .clicked()
                {
                    actions.push(Action::NudgeRate(-0.1));
                }
                ui.label(format!("Speed {:.0}%", rate.percent()));
                if style::quiet_icon(ui, icon::PLUS, "Faster", "Play faster (Shift+Right)")
                    .clicked()
                {
                    actions.push(Action::NudgeRate(0.1));
                }
            });
            style::chip(ui, |ui| {
                if style::quiet_icon(
                    ui,
                    icon::MAGNIFYING_GLASS_MINUS,
                    "Zoom out",
                    "Zoom out (Ctrl+Minus)",
                )
                .clicked()
                {
                    actions.push(Action::ZoomBy(Zoom::STEP.recip()));
                }
                ui.label(format!("Zoom {:.0}%", zoom.percent()));
                if style::quiet_icon(
                    ui,
                    icon::MAGNIFYING_GLASS_PLUS,
                    "Zoom in",
                    "Zoom in (Ctrl+Plus)",
                )
                .clicked()
                {
                    actions.push(Action::ZoomBy(Zoom::STEP));
                }
            });
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if style::tool(
                ui,
                icon::GEAR_SIX,
                "Open settings",
                "Settings (Ctrl+Comma)",
                false,
            )
            .clicked()
            {
                actions.push(Action::Settings);
            }
            style::chip(ui, |ui| {
                if style::tool(
                    ui,
                    icon::MUSIC_NOTE_SIMPLE,
                    "Note ticks",
                    "Note ticks (F4)",
                    ticks,
                )
                .clicked()
                {
                    actions.push(Action::ToggleTicks);
                }
                if style::tool(
                    ui,
                    icon::METRONOME,
                    "Metronome",
                    "Metronome (F3)",
                    metronome,
                )
                .clicked()
                {
                    actions.push(Action::ToggleMetronome);
                }
                let button = style::tool(ui, icon::SPEAKER_HIGH, "Volume", "Volume", false);
                let popup = egui::Popup::menu(&button)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
                popup.show(|ui| {
                    let mut changed = volumes;
                    style::table(ui, "volumes", |ui| {
                        for (name, gain) in [
                            ("Music volume", &mut changed.music),
                            ("Note tick volume", &mut changed.ticks),
                            ("Metronome volume", &mut changed.metronome),
                        ] {
                            let caption = ui.weak(name);
                            crate::settings::volume(ui, gain).labelled_by(caption.id);
                            ui.end_row();
                        }
                    });
                    if changed != volumes {
                        actions.push(Action::SetVolumes(changed));
                    }
                });
            });
        });
    });
}

/// Where the cursor is, large, as a transport's display shows it: the grid position, and the
/// time under it.
fn readout(ui: &mut Ui, song: &Song) {
    egui::Frame::new()
        .fill(style::WELL)
        .corner_radius(7)
        .inner_margin(egui::Margin::symmetric(12, 3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let timing = song.timing();
                let at = timing.grid_position(song.cursor()).to_string();
                let big = RichText::new(&at)
                    .monospace()
                    .size(17.0)
                    .color(style::ACCENT);
                let cursor = ui.label(big);
                cursor.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Label,
                        true,
                        format!("Cursor at {at}"),
                    )
                });
                let time = style::clock(timing.time_at(song.cursor()).seconds());
                ui.label(
                    RichText::new(time)
                        .monospace()
                        .size(13.0)
                        .color(style::WEAK_TEXT),
                );
            });
        });
}

/// What changes rarely: whether the audio plays and how it stands, and whether the song can be
/// edited.
pub fn status_bar(ui: &mut Ui, song: Option<&Song>, (audio, playing): (&str, bool)) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let Some(song) = song else {
            ui.weak("No song open");
            return;
        };
        let (colour, state) = if playing {
            (style::ACCENT, "Playing")
        } else {
            (style::WEAK_TEXT, "Paused")
        };
        ui.label(RichText::new("●").color(colour).size(9.0));
        ui.label(state);
        ui.weak(audio);
        let selected = song.selection().count();
        if selected > 0 {
            ui.separator();
            let noun = if selected == 1 { "note" } else { "notes" };
            ui.label(format!("{selected} {noun} selected"));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if song.read_only().is_some() {
                ui.weak("Read only");
                ui.label(style::glyph(ui, icon::LOCK_SIMPLE).color(style::WEAK_TEXT));
            } else {
                ui.label("Editing");
                ui.label(style::glyph(ui, icon::PENCIL_SIMPLE).color(style::ACCENT));
            }
        });
    });
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
pub fn empty(ui: &mut Ui, no_drops: bool, actions: &mut Vec<Action>) {
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() / 3.0 - 40.0).max(0.0));
        ui.label(
            RichText::new("charter")
                .family(style::titles(ui))
                .size(34.0),
        );
        ui.add_space(2.0);
        ui.weak("A chart editor for rhythm games");
        ui.add_space(22.0);
        let row = 3.0 * 136.0;
        ui.allocate_ui_with_layout(
            egui::vec2(row, 30.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.spacing_mut().button_padding = egui::vec2(14.0, 7.0);
                if style::icon_button(ui, style::icon::FILE_PLUS, "New song", true).clicked() {
                    actions.push(Action::NewSong);
                }
                if style::icon_button(ui, style::icon::FOLDER_OPEN, "Open song…", false).clicked()
                {
                    actions.push(Action::Open);
                }
                if style::icon_button(ui, style::icon::MUSIC_NOTES, "Open the demo song", false)
                    .clicked()
                {
                    actions.push(Action::OpenDemo);
                }
            },
        );
        if !no_drops {
            ui.add_space(16.0);
            ui.weak("or drop a charter song (.json) on this window");
        }
    });
}

/// That the open song has no audio yet, and how to give it some: choosing a file, or dropping
/// one unless `no_drops`.
pub fn no_audio(ui: &mut Ui, no_drops: bool, actions: &mut Vec<Action>) {
    notice(ui, style::icon::MUSIC_NOTES, |ui| {
        if no_drops {
            ui.label("No audio yet (.ogg, .mp3, .wav, .flac).");
        } else {
            ui.label(
                "No audio yet. Drop an audio file (.ogg, .mp3, .wav, .flac) on the window, or",
            );
        }
        let choose = style::icon_button(ui, style::icon::FOLDER_OPEN, "Choose audio…", true);
        let choose = if no_drops {
            choose.on_hover_text(NO_DROPS)
        } else {
            choose
        };
        if choose.clicked() {
            actions.push(Action::ChooseAudio);
        }
    });
}

/// Why files cannot be dragged onto the window, where they cannot.
pub(crate) const NO_DROPS: &str = "Dragging files in doesn't work under Wayland yet: the windowing library charter uses does \
     not take drops there.";

/// A strip across the top of the centre that says something about the song, led by `symbol`.
fn notice<R>(ui: &mut Ui, symbol: &str, body: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(style::NOTICE)
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(style::glyph(ui, symbol).color(style::WARN));
                body(ui)
            })
            .inner
        })
        .inner
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
        ui.set_max_width(380.0);
        ui.label(
            RichText::new(format!("Save changes to {name}?"))
                .family(style::titles(ui))
                .size(15.0),
        );
        ui.add_space(4.0);
        ui.label("Changes are lost unless you save them.");
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if ui.add(style::primary("Save")).clicked() {
                answer = Some(Answer::Save);
            }
            if ui.button("Don't save").clicked() {
                answer = Some(Answer::DontSave);
            }
            if ui.button("Cancel").clicked() {
                answer = Some(Answer::Cancel);
            }
        });
    });
    answer.or(modal.should_close().then_some(Answer::Cancel))
}

/// Asks whether to take the title and artist an audio file's tags carry over the song's own:
/// true to take them. Only an answer closes it.
pub fn offer_tags(ui: &Ui, title: Option<&str>, artist: Option<&str>) -> Option<bool> {
    let mut answer = None;
    egui::Modal::new(egui::Id::new("offer tags")).show(ui.ctx(), |ui| {
        ui.set_max_width(380.0);
        let heading = RichText::new("Import song metadata?")
            .family(style::titles(ui))
            .size(15.0);
        ui.label(heading);
        ui.add_space(4.0);
        ui.label("Do you want to import song metadata from tags?");
        ui.add_space(8.0);
        let rows: Vec<_> = [("Title", title), ("Artist", artist)]
            .into_iter()
            .filter_map(|(name, value)| value.map(|value| (name, value.to_owned())))
            .collect();
        fields(ui, "offered tags", &rows, "");
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if ui.add(style::primary("Import")).clicked() {
                answer = Some(true);
            }
            if ui.button("Keep mine").clicked() {
                answer = Some(false);
            }
        });
    });
    answer
}

/// Asks whether to restore the unsaved changes to the song in `name` that an earlier run kept:
/// true to restore them, false to throw them away. Only an answer closes it.
pub fn ask_to_restore(ui: &Ui, name: &str) -> Option<bool> {
    let mut answer = None;
    egui::Modal::new(egui::Id::new("restore changes")).show(ui.ctx(), |ui| {
        ui.set_max_width(380.0);
        let question = format!("Restore unsaved changes to {name}?");
        ui.label(RichText::new(question).family(style::titles(ui)).size(15.0));
        ui.add_space(4.0);
        ui.label("charter closed before they were saved.");
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if ui.add(style::primary("Restore")).clicked() {
                answer = Some(true);
            }
            if ui.button("Discard").clicked() {
                answer = Some(false);
            }
        });
    });
    answer
}

/// Whether to download the beat tracker's weights, which the user answers: true for Download.
pub fn ask_to_download_weights(ui: &Ui) -> Option<bool> {
    let mut answer = None;
    egui::Modal::new(egui::Id::new("download weights")).show(ui.ctx(), |ui| {
        ui.set_max_width(420.0);
        let question = "Download the beat tracker?";
        ui.label(RichText::new(question).family(style::titles(ui)).size(15.0));
        ui.add_space(4.0);
        ui.label(
            "Finding the timing uses Beat This!, a beat tracker from the Institute of \
             Computational Perception at JKU Linz, released under the MIT licence. Its weights \
             are 81 MB, downloaded once from the authors' server, cloud.cp.jku.at, into \
             charter's data folder. Nothing about the song is sent.",
        );
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if ui.add(style::primary("Download")).clicked() {
                answer = Some(true);
            }
            if ui.button("Cancel").clicked() {
                answer = Some(false);
            }
        });
    });
    answer
}

/// Whether to reset the open chart's timing, which the user answers: true for Reset.
pub fn ask_to_reset_timing(ui: &Ui) -> Option<bool> {
    let mut answer = None;
    egui::Modal::new(egui::Id::new("reset timing")).show(ui.ctx(), |ui| {
        ui.set_max_width(400.0);
        let question = "Reset the timing?";
        ui.label(RichText::new(question).family(style::titles(ui)).size(15.0));
        ui.add_space(4.0);
        ui.label(
            "Every BPM change, meter, stop and game timing event of the open chart's timing goes, \
             and it plays at 120 BPM in 4/4 from 0 ms. Notes keep their beats. Undo brings the \
             timing back.",
        );
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if ui.add(style::primary("Reset")).clicked() {
                answer = Some(true);
            }
            if ui.button("Cancel").clicked() {
                answer = Some(false);
            }
        });
    });
    answer
}

/// Why the last file did not open, or the last edit was refused, floating at the foot of the
/// area `ui` fills. Says whether the user dismissed it.
pub fn toast(ui: &Ui, message: &str) -> bool {
    let area = ui.max_rect();
    let width = (area.width() - 32.0).clamp(160.0, 640.0);
    egui::Area::new(ui.id().with("toast"))
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::CENTER_BOTTOM)
        .fixed_pos(area.center_bottom() - egui::vec2(0.0, 16.0))
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style())
                .fill(style::NOTICE)
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_width(width);
                    ui.horizontal(|ui| {
                        ui.label(style::glyph(ui, icon::WARNING).color(style::WARN));
                        ui.add(egui::Label::new(RichText::new(message).color(style::WARN)).wrap());
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let tip = "Dismiss this message";
                            style::quiet_icon(ui, icon::X, "Dismiss", tip).clicked()
                        })
                        .inner
                    })
                    .inner
                })
                .inner
        })
        .inner
}

/// Why the open song cannot be edited.
pub fn read_only(ui: &mut Ui, why: &str) {
    notice(ui, style::icon::LOCK_SIMPLE, |ui| {
        ui.label(format!("Read only: {why}"));
    });
    ui.add_space(4.0);
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
