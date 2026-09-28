//! charter's editor in a native window, opening the song whose path is the first argument, if
//! there is one. Songs of the built-in rulesets can be edited. The window asks this program to
//! read and write files and to show file dialogs, and this program keeps the user's settings and
//! the autosave of unsaved changes between runs.

// A release build on Windows is a window, not a console program with a window beside it.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};

mod disk;
mod finder;
mod player;
mod stretch;

use charter_core::RulesetPlugin;
use charter_ui::{Picture, Request, Saved, Settings, Window};

use crate::disk::Disk;
use crate::finder::Finder;
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
    /// Finds timing from the audio, and downloads the beat tracker it needs.
    finder: Finder,
    disk: Box<dyn Disk>,
}

impl eframe::App for Host {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        if self.player.update(&mut self.window) | self.finder.update(&mut self.window) {
            ui.ctx().request_repaint();
        }
        self.window.ui(ui);
        let requests = self.window.take_requests();
        self.handle_requests(requests);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, SETTINGS, self.window.settings());
    }
}

impl Host {
    fn handle_requests(&mut self, mut requests: Vec<Request>) {
        while !requests.is_empty() {
            for request in requests {
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
                    Request::ChooseTimingFile => {
                        let file = rfd::FileDialog::new()
                            .add_filter("osu! beatmap", &["osu"])
                            .add_filter("text", &["txt"])
                            .pick_file();
                        if let Some(path) = file {
                            self.read_timing(&path);
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
                    Request::ChooseArtwork => {
                        let file = rfd::FileDialog::new()
                            .add_filter("picture", &["png", "jpg", "jpeg"])
                            .pick_file();
                        if let Some(path) = file {
                            let picture = self.disk.read(&path).ok().and_then(picture);
                            self.window.use_artwork(&path, picture);
                        }
                    }
                    Request::FindTiming {
                        file,
                        ticks_per_beat,
                    } => {
                        let onsets = self.window.onsets().map(<[_]>::to_vec);
                        self.finder
                            .find(file, ticks_per_beat, onsets, &mut self.window);
                    }
                    Request::DownloadWeights => self.finder.download(&mut self.window),
                    Request::Play { .. }
                    | Request::Preview { .. }
                    | Request::Volume(_)
                    | Request::Pause
                    | Request::Seek(_)
                    | Request::Rate(_) => {
                        self.player.answer(&request, &mut self.window);
                    }
                    files => self.answer(files),
                }
            }
            requests = self.window.take_requests();
        }
    }

    /// Does what the window asked of the files; dialogs and playback are the caller's.
    fn answer(&mut self, request: Request) {
        match request {
            Request::Write { path, bytes } => self.write(path, &bytes),
            Request::Autosave { bytes } => self.autosave(&bytes),
            Request::CopyAudio { from, to } => {
                if let Err(error) = disk::copy_whole(self.disk.as_ref(), &from, &to) {
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
                    if let Some(artwork) = tags.artwork
                        && let Some(found) = picture(artwork.bytes)
                    {
                        self.window
                            .artwork_embedded(&file, artwork.extension, found);
                    }
                }
            }
            Request::ReadArtwork { file } => {
                let found = self.disk.read(&file).ok().and_then(picture);
                self.window.artwork_read(&file, found);
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
        let copied = match self.window.audio_copy_for(&path) {
            Some((from, to)) => disk::copy_whole(self.disk.as_ref(), &from, &to)
                .map_err(|error| format!("{} to {}: {error}", from.display(), to.display())),
            None => Ok(()),
        };
        let copied = copied.and_then(|()| match self.window.artwork_write_for(&path) {
            Some((to, bytes)) => disk::write_whole(self.disk.as_ref(), &to, &bytes)
                .map_err(|error| format!("{}: {error}", to.display())),
            None => Ok(()),
        });
        let saved = match copied.and_then(|()| {
            disk::write_whole(self.disk.as_ref(), &path, bytes)
                .map_err(|error| format!("{}: {error}", path.display()))
        }) {
            Ok(()) => Saved::To(path),
            Err(error) => Saved::Failed(error),
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

    /// Hands the text of the file at `path` to the import timing window.
    fn read_timing(&mut self, path: &Path) {
        match self.disk.read(path) {
            Ok(bytes) => self
                .window
                .timing_file_read(&String::from_utf8_lossy(&bytes)),
            Err(error) => {
                let why = format!("{}: {error}", path.display());
                self.window.timing_file_failed(&why);
            }
        }
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
        match self.disk.read(recovery) {
            Ok(bytes) => self.window.recover(&bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => self.window.refuse(recovery, &error),
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

/// The system's fonts for Japanese, Chinese and Korean, whatever the locale, since song titles
/// come in any script: each a name and its file's bytes, one of each file, in that order. A font
/// that cannot be read is left out, and a system without any draws those titles as boxes.
fn cjk_fonts() -> Vec<(String, Vec<u8>)> {
    use system_fonts::{FontPreset, FontStyle, FoundFontSource};
    let presets = [
        FontPreset::Japanese,
        FontPreset::SimplifiedChinese,
        FontPreset::TraditionalChinese,
        FontPreset::Korean,
    ];
    let mut seen = std::collections::HashSet::new();
    // ponytail: one CJK collection can run to tens of megabytes, read on the window's thread at
    // start; read it off the thread if startup shows it.
    system_fonts::find_from_presets(presets, FontStyle::Sans)
        .into_iter()
        .filter_map(|font| {
            let bytes = match font.source {
                FoundFontSource::Path(path) => {
                    if !seen.insert(path.clone()) {
                        return None;
                    }
                    std::fs::read(path).ok()?
                }
                FoundFontSource::Bytes(bytes) => bytes.to_vec(),
            };
            Some((font.key, bytes))
        })
        .collect()
}

/// Whether the window opens under Wayland, which winit prefers wherever a Wayland display is set
/// and where it takes no dropped files yet (rust-windowing/winit#1881).
fn on_wayland() -> bool {
    cfg!(target_os = "linux")
        && std::env::var_os("WAYLAND_DISPLAY").is_some_and(|display| !display.is_empty())
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
            let window = Window::new(PLUGINS)
                .with_settings(settings)
                .with_fallback_fonts(cjk_fonts());
            let window = if on_wayland() {
                window.without_file_drops()
            } else {
                window
            };
            let mut host = Host {
                window,
                recovery: eframe::storage_dir(APP).map(|folder| folder.join("recovery.json")),
                player: Player::default(),
                finder: Finder::new(
                    eframe::storage_dir(APP).map(|folder| folder.join("beat_this-final0.ckpt")),
                ),
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

/// The picture `bytes` encode, with a preview of it no larger than a song panel needs, unless
/// they are not a PNG or a JPEG.
fn picture(bytes: Vec<u8>) -> Option<Picture> {
    let preview = image::load_from_memory(&bytes)
        .ok()?
        .thumbnail(512, 512)
        .to_rgba8();
    let size = [preview.width(), preview.height()].map(|side| side as usize);
    let image = eframe::egui::ColorImage::from_rgba_unmultiplied(size, preview.as_raw());
    Some((bytes, image))
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
            finder: Finder::default(),
            disk: Box::new(disk),
        }
    }

    fn files(host: &Host, path: &str) -> bool {
        host.disk.exists(Path::new(path))
    }

    #[test]
    fn audio_is_copied_beside_the_song_but_different_existing_audio_is_reported() {
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
        assert!(
            host.window
                .message()
                .is_some_and(|message| message.contains("already holds different audio"))
        );
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
    fn saving_a_song_handles_its_audio_copy_before_returning_to_the_ui() {
        let mut host = host(Memory::with(&[("/m/a.ogg", b"audio")]));
        host.window.open(Path::new("/m/t.json"), SONG);
        host.handle_requests(vec![Request::Write {
            path: PathBuf::from("/s/Song/Song.json"),
            bytes: SONG.to_vec(),
        }]);
        assert!(files(&host, "/s/Song/Song.json"));
        assert!(files(&host, "/s/Song/a.ogg"));
        assert!(host.window.take_requests().is_empty());
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
    fn saving_as_does_not_commit_a_song_over_different_project_audio() {
        let mut host = host(Memory::with(&[
            ("/m/a.ogg", b"mine"),
            ("/s/Song/a.ogg", b"theirs"),
        ]));
        host.window.open(Path::new("/m/t.json"), SONG);
        host.save_as(Path::new("/s/Song.json"), SONG);
        let requests = host.window.take_requests();
        host.handle_requests(requests);
        assert!(!files(&host, "/s/Song/Song.json"));
        assert_eq!(host.window.title(), "t.json - charter");
        assert!(
            host.window
                .message()
                .is_some_and(|message| message.contains("different audio"))
        );
    }

    #[test]
    fn saving_as_does_not_commit_a_song_when_audio_copy_fails_midway() {
        let mut host = host(Memory {
            partial_copy_fail: true,
            ..Memory::with(&[("/m/a.ogg", b"the whole recording")])
        });
        host.window.open(Path::new("/m/t.json"), SONG);
        host.save_as(Path::new("/s/Song.json"), SONG);
        let requests = host.window.take_requests();
        host.handle_requests(requests);
        assert!(!files(&host, "/s/Song/Song.json"));
        assert!(!files(&host, "/s/Song/a.ogg"));
        assert!(!files(&host, "/s/Song/a.ogg.partial"));
        assert!(
            host.window
                .message()
                .is_some_and(|message| message.contains("the disk is full"))
        );
    }

    #[test]
    fn a_timing_file_is_read_into_the_window_and_one_missing_says_why() {
        let mut host = host(Memory::with(&[(
            "/m/map.osu",
            b"[TimingPoints]\n0,500,4,1,0,100,1,0",
        )]));
        host.read_timing(Path::new("/m/map.osu"));
        assert_eq!(host.window.message(), None);
        host.read_timing(Path::new("/m/gone.osu"));
        assert!(host.window.message().unwrap().contains("gone.osu"));
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
    fn an_unreadable_recovery_file_reports_the_path_and_error() {
        let mut host = host(Memory {
            unreadable: vec![PathBuf::from("/data/recovery.json")],
            ..Memory::with(&[("/data/recovery.json", b"kept")])
        });
        host.recover();
        assert!(host.window.message().is_some_and(|message| {
            message.contains("recovery.json") && message.contains("access denied")
        }));
    }

    /// A PNG of `width` by `height` red pixels.
    fn png(width: u32, height: u32) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(width, height, image::Rgba([255, 0, 0, 255]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    #[test]
    fn a_picture_previews_no_larger_than_512_and_anything_else_is_none() {
        let (bytes, preview) = picture(png(1024, 256)).unwrap();
        assert_eq!(bytes, png(1024, 256));
        assert_eq!(preview.size, [512, 128]);
        assert!(picture(b"not a picture".to_vec()).is_none());
    }

    #[test]
    fn saving_writes_chosen_artwork_beside_the_song() {
        let mut host = host(Memory::with(&[("/m/a.ogg", b"audio")]));
        host.window = Window::new(PLUGINS);
        let metadata = charter_core::Metadata {
            title: "t".to_owned(),
            artist: "a".to_owned(),
            audio: charter_core::ResourcePath::new("a.ogg").unwrap(),
            preview: None,
            artwork: None,
            genre: None,
            ext: charter_core::Extensions::new(),
        };
        let song = charter_stepmania::Plugin
            .new_song(&charter_core::NewSong {
                metadata,
                layout: 0,
                charts: vec![charter_core::NewChart::named("Easy")],
            })
            .unwrap()
            .to_json()
            .unwrap();
        host.window.open(Path::new("/m/t.json"), &song);
        host.window
            .use_artwork(Path::new("/p/art.png"), picture(png(2, 2)));
        assert_eq!(host.window.message(), None);
        host.handle_requests(vec![Request::Write {
            path: PathBuf::from("/s/Song/Song.json"),
            bytes: SONG.to_vec(),
        }]);
        assert_eq!(
            host.disk.read(Path::new("/s/Song/art.png")).unwrap(),
            png(2, 2)
        );
        assert!(files(&host, "/s/Song/Song.json"));
    }

    #[test]
    fn saving_writes_the_cover_new_audio_holds_into_the_song_folder() {
        let mut host = host(Memory::with(&[("/m/track.flac", b"audio")]));
        host.window = Window::new(PLUGINS);
        let metadata = charter_core::Metadata {
            title: "t".to_owned(),
            artist: "a".to_owned(),
            audio: charter_core::ResourcePath::new("a.ogg").unwrap(),
            preview: None,
            artwork: None,
            genre: None,
            ext: charter_core::Extensions::new(),
        };
        let song = charter_stepmania::Plugin
            .new_song(&charter_core::NewSong {
                metadata,
                layout: 0,
                charts: vec![charter_core::NewChart::named("Easy")],
            })
            .unwrap()
            .to_json()
            .unwrap();
        host.window.open(Path::new("/s/Song/Song.json"), &song);
        let audio = Path::new("/m/track.flac");
        host.window.use_audio(audio);
        host.window.take_requests();
        let (bytes, _) = picture(png(2, 2)).unwrap();
        host.window
            .artwork_embedded(audio, "png", picture(bytes).unwrap());
        host.handle_requests(vec![Request::Write {
            path: PathBuf::from("/s/Song/Song.json"),
            bytes: song,
        }]);
        assert_eq!(host.window.message(), None);
        assert_eq!(
            host.disk.read(Path::new("/s/Song/cover.png")).unwrap(),
            png(2, 2)
        );
        assert!(files(&host, "/s/Song/track.flac"));
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
