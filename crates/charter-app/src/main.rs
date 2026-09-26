//! charter's editor in a native window, opening the song whose path is the first argument, if
//! there is one. Songs of the built-in rulesets can be edited. The window asks this program to
//! read and write files and to show file dialogs, and this program keeps the user's settings and
//! the autosave of unsaved changes between runs.

// A release build on Windows is a window, not a console program with a window beside it.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};

mod disk;
mod player;

use charter_core::RulesetPlugin;
use charter_ui::{Request, Saved, Settings, Window};

use crate::disk::Disk;
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
    disk: Box<dyn Disk>,
}

impl eframe::App for Host {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        if self.player.update(&mut self.window) {
            ui.ctx().request_repaint();
        }
        self.window.ui(ui);
        for request in self.window.take_requests() {
            match request {
                Request::SaveAs { name, bytes } => {
                    // The name the dialog suggests is the project's, which becomes a folder.
                    let name = name.strip_suffix(".json").unwrap_or(&name);
                    match song_dialog().set_file_name(name).save_file() {
                        Some(path) => self.save_as(&path, &bytes),
                        None => self.window.saved(Saved::Cancelled),
                    }
                }
                Request::Open => {
                    if let Some(path) = song_dialog().pick_file() {
                        self.read(&path);
                    }
                }
                Request::ChooseAudio => {
                    let audio = rfd::FileDialog::new()
                        .add_filter("audio", &["ogg", "mp3", "wav", "flac"])
                        .pick_file();
                    if let Some(path) = audio {
                        self.window.use_audio(&path);
                    }
                }
                Request::Play { .. }
                | Request::Preview { .. }
                | Request::Pause
                | Request::Seek(_)
                | Request::Rate(_) => {
                    self.player.answer(&request, &mut self.window);
                }
                files => self.answer(files),
            }
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, SETTINGS, self.window.settings());
    }
}

impl Host {
    /// Does what the window asked of the files; dialogs and playback are the caller's.
    fn answer(&mut self, request: Request) {
        match request {
            Request::Write { path, bytes } => self.write(path, &bytes),
            Request::Autosave { bytes } => self.autosave(&bytes),
            Request::CopyAudio { from, to } => {
                // A file already there is taken to be the song's audio.
                if !self.disk.exists(&to)
                    && let Err(error) = self.disk.copy(&from, &to)
                {
                    let why = format!("{} to {}: {error}", from.display(), to.display());
                    self.window.audio_not_copied(&why);
                }
            }
            Request::ReadTags { file } => {
                // A file that cannot be read has no tags, which leaves the song as it is.
                if let Ok(bytes) = self.disk.read(&file) {
                    let extension = file.extension().and_then(|extension| extension.to_str());
                    let tags = charter_audio::tags(bytes, extension);
                    self.window.tags_read(&file, tags.title, tags.artist);
                }
            }
            Request::DropAutosave => {
                if let Some(recovery) = &self.recovery {
                    // Nothing to remove is fine, and a file left over is offered, then
                    // removed, on the next run.
                    let _ = self.disk.remove(recovery);
                }
            }
            _ => {}
        }
    }

    fn read(&mut self, path: &Path) {
        match self.disk.read(path) {
            Ok(bytes) => self.window.open(path, &bytes),
            Err(error) => self.window.refuse(path, &error),
        }
    }

    fn write(&mut self, path: PathBuf, bytes: &[u8]) {
        let saved = match disk::write_whole(self.disk.as_ref(), &path, bytes) {
            Ok(()) => Saved::To(path),
            Err(error) => Saved::Failed(format!("{}: {error}", path.display())),
        };
        self.window.saved(saved);
    }

    /// Saves a song the user chose to save as `chosen` into a project folder named for it,
    /// where its audio is copied too and, later, anything worked out from it.
    fn save_as(&mut self, chosen: &Path, bytes: &[u8]) {
        let Some(file) = project_file(chosen) else {
            let why = format!("{} cannot name a song", chosen.display());
            self.window.saved(Saved::Failed(why));
            return;
        };
        let folder = file.parent().unwrap_or(Path::new(""));
        if file != chosen && self.disk.exists(&file) {
            let why = format!(
                "{} already holds a song. Choose that file to replace it, or another name",
                folder.display()
            );
            self.window.saved(Saved::Failed(why));
            return;
        }
        if let Err(error) = self.disk.make_folder(folder) {
            let why = format!("{}: {error}", folder.display());
            self.window.saved(Saved::Failed(why));
            return;
        }
        self.write(file, bytes);
    }

    fn autosave(&mut self, bytes: &[u8]) {
        let Some(recovery) = &self.recovery else {
            self.window
                .autosave_failed("there is no folder to keep it in");
            return;
        };
        let kept = recovery
            .parent()
            .map_or(Ok(()), |folder| self.disk.make_folder(folder))
            .and_then(|()| disk::write_whole(self.disk.as_ref(), recovery, bytes));
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
        if let Ok(bytes) = self.disk.read(recovery) {
            self.window.recover(&bytes);
        }
    }
}

/// A native file dialog for charter songs.
/// Where a song saved as `chosen` goes: `<name>/<name>.json` beside it, named after the file
/// chosen without its `.json`, or `chosen` itself when it already is such a file.
fn project_file(chosen: &Path) -> Option<PathBuf> {
    let name = chosen.file_name()?.to_str()?;
    let name = name.strip_suffix(".json").unwrap_or(name);
    if name.is_empty() {
        return None;
    }
    let beside = chosen.parent()?;
    let file = format!("{name}.json");
    if beside.file_name().and_then(|folder| folder.to_str()) == Some(name) {
        return Some(beside.join(file));
    }
    Some(beside.join(name).join(file))
}

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
                disk: Box::new(disk::Real),
            };
            if let Some(path) = std::env::args_os().nth(1) {
                host.read(Path::new(&path));
            }
            host.recover();
            Ok(Box::new(host))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disk::memory::Memory;

    /// A song in `sm`, which needs no plugin to open read-only, naming `a.ogg`.
    const SONG: &[u8] = br#"{ "format_version": 0, "ruleset": { "id": "sm", "version": "0" },
        "metadata": { "title": "t", "artist": "a", "audio": "a.ogg" },
        "timing": { "offset_seconds": 0.0, "events": [ { "tick": 0, "kind": { "bpm": 120.0 } } ] },
        "charts": [ { "name": "Easy", "lanes": 4 } ] }"#;

    fn host(disk: Memory) -> Host {
        Host {
            window: Window::new(&[]),
            recovery: Some(PathBuf::from("/data/recovery.json")),
            player: Player::default(),
            disk: Box::new(disk),
        }
    }

    fn files(host: &Host, path: &str) -> bool {
        host.disk.exists(Path::new(path))
    }

    #[test]
    fn audio_is_copied_beside_the_song_unless_a_file_is_already_there() {
        let mut host = host(Memory::with(&[
            ("/m/a.ogg", b"audio"),
            ("/s/b.ogg", b"mine"),
        ]));
        host.answer(Request::CopyAudio {
            from: PathBuf::from("/m/a.ogg"),
            to: PathBuf::from("/s/a.ogg"),
        });
        assert!(files(&host, "/s/a.ogg"));
        host.answer(Request::CopyAudio {
            from: PathBuf::from("/m/a.ogg"),
            to: PathBuf::from("/s/b.ogg"),
        });
        assert_eq!(host.disk.read(Path::new("/s/b.ogg")).unwrap(), b"mine");
        assert_eq!(host.window.message(), None);
    }

    #[test]
    fn audio_that_cannot_be_copied_says_why() {
        let mut host = host(Memory::default());
        host.answer(Request::CopyAudio {
            from: PathBuf::from("/m/gone.ogg"),
            to: PathBuf::from("/s/gone.ogg"),
        });
        assert!(
            host.window
                .message()
                .unwrap()
                .contains("gone.ogg is not there")
        );
    }

    #[test]
    fn saving_as_a_name_makes_a_project_folder_holding_the_song_and_its_audio() {
        let mut host = host(Memory::with(&[("/m/a.ogg", b"audio")]));
        host.window.open(Path::new("/m/t.json"), SONG);
        host.save_as(Path::new("/s/My Song.json"), b"{}");
        assert!(files(&host, "/s/My Song/My Song.json"));
        assert!(!files(&host, "/s/My Song.json"));
        assert_eq!(host.window.title(), "My Song.json - charter");
        for request in host.window.take_requests() {
            host.answer(request);
        }
        assert!(files(&host, "/s/My Song/a.ogg"));
    }

    #[test]
    fn a_name_without_an_extension_names_the_folder_and_the_file() {
        let mut host = host(Memory::default());
        host.save_as(Path::new("/s/Song"), b"{}");
        assert!(files(&host, "/s/Song/Song.json"));
    }

    #[test]
    fn saving_as_the_song_in_its_own_project_folder_writes_it_there() {
        let mut host = host(Memory::with(&[("/s/Song/Song.json", b"old")]));
        host.save_as(Path::new("/s/Song/Song.json"), b"new");
        assert_eq!(
            host.disk.read(Path::new("/s/Song/Song.json")).unwrap(),
            b"new"
        );
        assert!(!files(&host, "/s/Song/Song/Song.json"));
    }

    #[test]
    fn a_project_folder_already_holding_a_song_is_left_alone() {
        let mut host = host(Memory::with(&[("/s/Song/Song.json", b"theirs")]));
        host.save_as(Path::new("/s/Song.json"), b"mine");
        assert_eq!(
            host.disk.read(Path::new("/s/Song/Song.json")).unwrap(),
            b"theirs"
        );
        assert!(
            host.window
                .message()
                .unwrap()
                .contains("already holds a song")
        );
    }

    #[test]
    fn a_write_that_fails_says_why() {
        let mut host = host(Memory {
            unwritable: vec![PathBuf::from("/s/song.json.partial")],
            ..Memory::default()
        });
        host.answer(Request::Write {
            path: PathBuf::from("/s/song.json"),
            bytes: b"{}".to_vec(),
        });
        assert!(host.window.message().unwrap().contains("the disk is full"));
    }

    #[test]
    fn the_autosave_is_kept_in_the_data_folder_and_dropped_on_request() {
        let mut host = host(Memory::default());
        host.answer(Request::Autosave {
            bytes: b"kept".to_vec(),
        });
        assert!(files(&host, "/data/recovery.json"));
        host.answer(Request::DropAutosave);
        assert!(!files(&host, "/data/recovery.json"));
        host.answer(Request::DropAutosave);
        assert_eq!(host.window.message(), None);
    }

    #[test]
    fn an_autosave_that_cannot_be_kept_says_why() {
        let mut host = host(Memory {
            unwritable: vec![PathBuf::from("/data/recovery.json.partial")],
            ..Memory::default()
        });
        host.answer(Request::Autosave {
            bytes: b"kept".to_vec(),
        });
        assert!(
            host.window
                .message()
                .unwrap()
                .starts_with("Autosave failed")
        );
    }

    #[test]
    fn a_recovery_file_left_behind_is_offered_and_a_damaged_one_says_so() {
        let mut host = host(Memory::with(&[("/data/recovery.json", b"{ damaged")]));
        host.recover();
        assert!(
            host.window
                .message()
                .unwrap()
                .contains("could not be restored")
        );
    }

    #[test]
    fn tags_of_audio_that_cannot_be_read_leave_the_song_alone() {
        let mut host = host(Memory::with(&[("/s/song.json", SONG)]));
        host.read(Path::new("/s/song.json"));
        host.answer(Request::ReadTags {
            file: PathBuf::from("/s/gone.ogg"),
        });
        assert_eq!(host.window.message(), None);
        assert_eq!(host.window.title(), "song.json - charter");
    }

    #[test]
    fn a_song_read_from_the_disk_opens_and_one_missing_says_why() {
        let mut host = host(Memory::with(&[("/s/song.json", SONG)]));
        host.read(Path::new("/s/song.json"));
        assert_eq!(host.window.title(), "song.json - charter");
        host.read(Path::new("/s/other.json"));
        assert!(host.window.message().unwrap().contains("did not open"));
    }
}
