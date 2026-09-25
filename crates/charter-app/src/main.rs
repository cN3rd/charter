//! charter's editor in a native window, opening the song whose path is the first argument, if
//! there is one. Songs of the built-in rulesets can be edited. The window asks this program to
//! read and write files and to show file dialogs, and this program keeps the user's settings and
//! the autosave of unsaved changes between runs.

use std::path::{Path, PathBuf};

mod player;

use charter_core::RulesetPlugin;
use charter_ui::{Request, Saved, Settings, Window};

use crate::player::Player;

/// The ruleset plugins built into charter.
const PLUGINS: &[&dyn RulesetPlugin] = &[
    &charter_stepmania::Plugin,
    &charter_ttr::Plugin,
    &charter_osu_mania::Plugin,
];

/// Where the settings are kept in eframe's storage.
const SETTINGS: &str = "settings";

/// The app's name, which also names its folder in the user's data.
const APP: &str = "charter";

/// The editor window, and the files and dialogs it asks for.
struct Host {
    window: Window,
    /// Where the autosave is kept, when the user's data folder is known.
    recovery: Option<PathBuf>,
    player: Player,
}

impl eframe::App for Host {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        if self.player.update(&mut self.window) {
            ui.ctx().request_repaint();
        }
        self.window.ui(ui);
        for request in self.window.take_requests() {
            match request {
                Request::Play { .. } | Request::Pause | Request::Seek(_) | Request::Rate(_) => {
                    self.player.answer(&request, &mut self.window);
                }
                Request::Write { path, bytes } => self.write(path, &bytes),
                Request::SaveAs { name, bytes } => {
                    match song_dialog().set_file_name(name).save_file() {
                        Some(path) => self.write(path, &bytes),
                        None => self.window.saved(Saved::Cancelled),
                    }
                }
                Request::Open => {
                    if let Some(path) = song_dialog().pick_file() {
                        self.read(&path);
                    }
                }
                Request::Autosave { bytes } => self.autosave(&bytes),
                Request::CopyAudio { from, to } => {
                    // A file already there is taken to be the song's audio.
                    if !to.exists()
                        && let Err(error) = std::fs::copy(&from, &to)
                    {
                        let why = format!("{} to {}: {error}", from.display(), to.display());
                        self.window.audio_not_copied(&why);
                    }
                }
                Request::DropAutosave => {
                    if let Some(recovery) = &self.recovery {
                        // Nothing to remove is fine, and a file left over is offered, then
                        // removed, on the next run.
                        let _ = std::fs::remove_file(recovery);
                    }
                }
            }
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, SETTINGS, self.window.settings());
    }
}

impl Host {
    fn read(&mut self, path: &Path) {
        match std::fs::read(path) {
            Ok(bytes) => self.window.open(path, &bytes),
            Err(error) => self.window.refuse(path, &error),
        }
    }

    fn write(&mut self, path: PathBuf, bytes: &[u8]) {
        let saved = match write_whole(&path, bytes) {
            Ok(()) => Saved::To(path),
            Err(error) => Saved::Failed(format!("{}: {error}", path.display())),
        };
        self.window.saved(saved);
    }

    fn autosave(&mut self, bytes: &[u8]) {
        let Some(recovery) = &self.recovery else {
            self.window
                .autosave_failed("there is no folder to keep it in");
            return;
        };
        let kept = recovery
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| write_whole(recovery, bytes));
        if let Err(error) = kept {
            let why = format!("{}: {error}", recovery.display());
            self.window.autosave_failed(&why);
        }
    }

    /// Offers to restore the autosave an earlier run left, if there is one.
    fn recover(&mut self) {
        let Some(recovery) = &self.recovery else {
            return;
        };
        if let Ok(bytes) = std::fs::read(recovery) {
            self.window.recover(&bytes);
        }
    }
}

/// Writes `bytes` beside `path`, then moves them over the file at `path`, so a failed write never
/// leaves the file half written.
fn write_whole(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut beside = path.as_os_str().to_owned();
    beside.push(".partial");
    let beside = PathBuf::from(beside);
    let written = write_then_move(&beside, path, bytes);
    if written.is_err() {
        // The write already failed; a partial file left over is the lesser problem.
        let _ = std::fs::remove_file(&beside);
    }
    written
}

// ponytail: no fsync, since some filesystems refuse it. The autosave covers a crash; sync here if
// saves on power loss turn out to matter.
fn write_then_move(beside: &Path, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(beside, bytes)?;
    std::fs::rename(beside, path)
}

/// A native file dialog for charter songs.
fn song_dialog() -> rfd::FileDialog {
    rfd::FileDialog::new().add_filter("charter song", &["json"])
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]),
        ..eframe::NativeOptions::default()
    };
    eframe::run_native(
        APP,
        options,
        Box::new(|creation| {
            let kept = creation
                .storage
                .and_then(|storage| eframe::get_value(storage, SETTINGS));
            let settings: Settings = kept.unwrap_or_default();
            let mut host = Host {
                window: Window::new(PLUGINS).with_settings(settings),
                recovery: eframe::storage_dir(APP).map(|folder| folder.join("recovery.json")),
                player: Player::default(),
            };
            if let Some(path) = std::env::args_os().nth(1) {
                host.read(Path::new(&path));
            }
            host.recover();
            Ok(Box::new(host))
        }),
    )
}
