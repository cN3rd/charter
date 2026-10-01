//! charter's editor in a native window, opening the song whose path is the first argument, if
//! there is one. Songs of the built-in rulesets can be edited. The window asks this program to
//! read and write files and to show file dialogs, and this program keeps the user's settings and
//! the autosave of unsaved changes between runs.

// A release build on Windows is a window, not a console program with a window beside it.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};

mod disk;
mod finder;
mod host;
mod player;
mod stretch;

use charter_core::RulesetPlugin;
use charter_ui::{Picture, Settings, Window};

use crate::host::Host;
#[cfg(test)]
use crate::{finder::Finder, player::Player};
#[cfg(test)]
use charter_ui::{Request, Saved};

/// The ruleset plugins built into charter.
const PLUGINS: &[&dyn RulesetPlugin] = &[
    &charter_stepmania::Plugin,
    &charter_ttr::Plugin,
    &charter_osu_mania::Plugin,
];

/// Where the settings are kept in eframe's storage: as the changes from the defaults, and in the
/// whole copy older versions kept, read when there are no changes kept yet.
const SETTINGS: &str = "settings.changes";
/// Legacy storage key used when the changes-only settings are absent.
const OLD_SETTINGS: &str = "settings";

/// The app's name, which also names its folder in the user's data.
const APP: &str = "charter";

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

/// File dialog restricted to charter's JSON song files.
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
            let settings = creation
                .storage
                .map_or_else(Settings::default, kept_settings);
            let window = Window::new(PLUGINS).with_settings(settings);
            let window = match eframe::storage_dir(APP) {
                Some(folder) => window.with_data_folder(folder),
                None => window,
            };
            let window = window.with_fallback_fonts(cjk_fonts());
            let window = if on_wayland() {
                window.without_file_drops()
            } else {
                window
            };
            let mut host = Host {
                window,
                recovery: eframe::storage_dir(APP).map(|folder| folder.join("recovery.json")),
                player: player::Player::default(),
                finder: finder::Finder::new(eframe::storage_dir(APP)),
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

/// The settings `storage` keeps: the changes from the defaults, or the whole copy an older version
/// kept, whose note ticks were on as the sound they made and off as `off`.
fn kept_settings(storage: &dyn eframe::Storage) -> Settings {
    #[derive(Default, serde::Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum OldSound {
        #[default]
        Off,
        Click,
        Beep,
        Hit,
        Clap,
        SoftClap,
        Finish,
    }
    #[derive(serde::Deserialize)]
    struct OldTicks {
        #[serde(default)]
        tick_sound: OldSound,
    }
    if let Some(kept) = storage.get_string(SETTINGS) {
        return Settings::from_kept(&kept);
    }
    let mut settings: Settings = eframe::get_value(storage, OLD_SETTINGS).unwrap_or_default();
    let old: Option<OldTicks> = eframe::get_value(storage, OLD_SETTINGS);
    settings.note_ticks = old.is_some_and(|old| !matches!(old.tick_sound, OldSound::Off));
    settings
}

/// `secs`, seconds since 1970, as a date and time in UTC that sorts as it reads:
/// `2026-09-29 05.40.12 UTC`.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "calendar arithmetic is bounded by u64 seconds divided into days"
)]
fn stamp(secs: u64) -> String {
    let days = i64::try_from(secs.div_euclid(86_400)).unwrap_or(i64::MAX);
    let seconds = secs.rem_euclid(86_400);
    // Days to a civil date, after Howard Hinnant's `civil_from_days`.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era.div_euclid(1460) + day_of_era.div_euclid(36_524)
        - day_of_era.div_euclid(146_096))
    .div_euclid(365);
    let day_of_year =
        day_of_era - (365 * year_of_era + year_of_era.div_euclid(4) - year_of_era.div_euclid(100));
    let shifted_month = (5 * day_of_year + 2).div_euclid(153);
    let day = day_of_year - (153 * shifted_month + 2).div_euclid(5) + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}.{:02}.{:02} UTC",
        seconds.div_euclid(3600),
        seconds.div_euclid(60).rem_euclid(60),
        seconds.rem_euclid(60)
    )
}

/// Whether `file` is a snapshot of the song `name`: its name, a date and time, `.json`.
fn is_copy_of(file: &str, name: &str) -> bool {
    file.strip_prefix(name)
        .and_then(|rest| rest.strip_prefix(' '))
        .and_then(|rest| rest.strip_suffix(" UTC.json"))
        .is_some_and(|date| {
            date.len() == 19
                && date
                    .bytes()
                    .all(|b| b.is_ascii_digit() || b" -.".contains(&b))
        })
}

/// The picture `bytes` encode, with a preview of it no larger than a song panel needs, unless
/// they are not a PNG or a JPEG.
fn picture(bytes: Vec<u8>) -> Option<Picture> {
    let preview = image::load_from_memory(&bytes)
        .ok()?
        .thumbnail(512, 512)
        .to_rgba8();
    let size =
        [preview.width(), preview.height()].map(|side| usize::try_from(side).unwrap_or(usize::MAX));
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
        let requests = host.window.take_requests();
        host.handle_requests(requests);
        assert!(!files(&host, "/data/recovery.json"));
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

    #[test]
    fn no_recovery_location_or_file_does_not_offer_a_missing_autosave() {
        let mut host = host(Memory::default());
        host.recover();
        assert_eq!(host.window.message(), None);
        host.recovery = None;
        host.recover();
        assert_eq!(host.window.message(), None);
        assert_eq!(host.window.title(), "charter");
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

    /// An empty StepMania song the plugin accepts, as its file holds it.
    fn sm_song() -> Vec<u8> {
        sm_song_with(None)
    }

    /// An empty StepMania song naming `artwork` beside it.
    fn sm_song_with(artwork: Option<&str>) -> Vec<u8> {
        let metadata = charter_core::Metadata {
            title: "t".to_owned(),
            artist: "a".to_owned(),
            audio: charter_core::ResourcePath::new("a.ogg").unwrap(),
            preview: None,
            artwork: artwork.map(|name| charter_core::ResourcePath::new(name).unwrap()),
            genre: None,
            ext: charter_core::Extensions::new(),
        };
        charter_stepmania::Plugin
            .new_song(&charter_core::NewSong {
                metadata,
                layout: 0,
                charts: vec![charter_core::NewChart::named("Easy")],
            })
            .unwrap()
            .to_json()
            .unwrap()
    }

    #[test]
    fn saving_writes_chosen_artwork_beside_the_song() {
        let mut host = host(Memory::with(&[("/m/a.ogg", b"audio")]));
        host.window = Window::new(PLUGINS);
        let song = sm_song();
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
        let song = sm_song();
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
    fn saving_leaves_another_picture_of_the_artwork_s_name_alone() {
        let mut host = host(Memory::with(&[
            ("/m/track.flac", b"audio"),
            ("/s/Song/cover.png", b"theirs"),
        ]));
        host.window = Window::new(PLUGINS);
        let song = sm_song();
        host.window.open(Path::new("/s/Song/Song.json"), &song);
        let audio = Path::new("/m/track.flac");
        host.window.use_audio(audio);
        host.window.take_requests();
        host.window
            .artwork_embedded(audio, "png", picture(png(2, 2)).unwrap());
        host.handle_requests(vec![Request::Write {
            path: PathBuf::from("/s/Song/Song.json"),
            bytes: song,
        }]);
        assert_eq!(
            host.disk.read(Path::new("/s/Song/cover.png")).unwrap(),
            b"theirs"
        );
        assert!(
            host.window
                .message()
                .is_some_and(|message| message.contains("another picture named cover.png"))
        );
    }

    /// eframe's storage, in memory.
    #[derive(Default)]
    struct Kept(std::collections::HashMap<String, String>);

    impl eframe::Storage for Kept {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }

        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.to_owned(), value);
        }

        fn remove_string(&mut self, key: &str) {
            self.0.remove(key);
        }

        fn flush(&mut self) {}
    }

    #[test]
    fn settings_an_older_version_kept_whole_keep_their_note_ticks_on_or_off() {
        #[derive(Clone, Copy, Debug, serde::Serialize)]
        #[serde(rename_all = "snake_case")]
        enum Sound {
            Beep,
            Off,
        }
        #[derive(serde::Serialize)]
        struct Old {
            tick_sound: Sound,
            metronome: bool,
        }
        for (sound, on) in [(Sound::Beep, true), (Sound::Off, false)] {
            let mut storage = Kept::default();
            let old = Old {
                tick_sound: sound,
                metronome: true,
            };
            eframe::set_value(&mut storage, OLD_SETTINGS, &old);
            let settings = kept_settings(&storage);
            assert_eq!(settings.note_ticks, on, "{sound:?}");
            assert!(settings.metronome);
        }
        let mut storage = Kept::default();
        eframe::Storage::set_string(&mut storage, SETTINGS, r#"{ "zoom": 150.0 }"#.to_owned());
        assert!((kept_settings(&storage).zoom - 150.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_save_that_fails_puts_the_artwork_file_back_as_it_was() {
        let mut host = host(Memory {
            unwritable: vec![PathBuf::from("/s/Song/Song.json.partial")],
            ..Memory::with(&[("/s/Song/art.png", b"the old picture")])
        });
        host.window = Window::new(PLUGINS);
        let song = sm_song_with(Some("art.png"));
        host.window.open(Path::new("/s/Song/Song.json"), &song);
        host.window
            .use_artwork(Path::new("/p/art.png"), picture(png(2, 2)));
        host.window.take_requests();
        host.write(PathBuf::from("/s/Song/Song.json"), &song);
        assert!(
            host.window
                .message()
                .is_some_and(|message| message.contains("disk is full"))
        );
        assert_eq!(
            host.disk.read(Path::new("/s/Song/art.png")).unwrap(),
            b"the old picture"
        );
        host.window
            .use_artwork(Path::new("/p/new.png"), picture(png(2, 2)));
        host.write(PathBuf::from("/s/Song/Song.json"), &song);
        assert!(!files(&host, "/s/Song/new.png"));
    }

    #[test]
    fn a_stamp_reads_as_the_date_and_time_in_utc() {
        assert_eq!(stamp(0), "1970-01-01 00.00.00 UTC");
        assert_eq!(stamp(951_782_400), "2000-02-29 00.00.00 UTC");
        assert_eq!(stamp(1_790_142_012), "2026-09-23 05.40.12 UTC");
        assert!(is_copy_of("Song 2026-09-23 05.40.12 UTC.json", "Song"));
        assert!(!is_copy_of("Song B 2026-09-23 05.40.12 UTC.json", "Song"));
        assert!(!is_copy_of("Song.json", "Song"));
    }

    #[test]
    fn snapshots_go_into_each_folder_and_keep_only_the_newest() {
        let mut host = host(Memory::with(&[
            ("/snaps/Song/Song 2026-01-01 00.00.00 UTC.json", b"oldest"),
            ("/snaps/Song/Song 2026-01-02 00.00.00 UTC.json", b"older"),
            (
                "/snaps/Song/Other 2020-01-01 00.00.00 UTC.json",
                b"another song",
            ),
            ("/snaps/Song/notes.txt", b"theirs"),
        ]));
        let folders = [
            PathBuf::from("/snaps/Song"),
            PathBuf::from("/s/Song/autosaves"),
        ];
        host.snapshot("Song", b"now", (&folders, 2), 1_790_142_012);
        let copy = "Song 2026-09-23 05.40.12 UTC.json";
        assert_eq!(host.disk.read(&folders[0].join(copy)).unwrap(), b"now");
        assert_eq!(host.disk.read(&folders[1].join(copy)).unwrap(), b"now");
        assert!(!files(
            &host,
            "/snaps/Song/Song 2026-01-01 00.00.00 UTC.json"
        ));
        assert!(files(
            &host,
            "/snaps/Song/Song 2026-01-02 00.00.00 UTC.json"
        ));
        assert!(files(
            &host,
            "/snaps/Song/Other 2020-01-01 00.00.00 UTC.json"
        ));
        assert!(files(&host, "/snaps/Song/notes.txt"));
        assert_eq!(host.window.message(), None);
    }

    #[test]
    fn a_snapshot_that_cannot_be_written_says_where() {
        let copy = "/snaps/Song/Song 1970-01-01 00.00.00 UTC.json.partial";
        let mut host = host(Memory {
            unwritable: vec![PathBuf::from(copy)],
            ..Memory::default()
        });
        host.snapshot("Song", b"now", (&[PathBuf::from("/snaps/Song")], 20), 0);
        assert!(
            host.window
                .message()
                .is_some_and(|message| message.contains("/snaps/Song"))
        );
    }

    #[test]
    fn a_recent_song_opens_through_the_host_and_one_gone_leaves_the_list() {
        let mut host = host(Memory::with(&[("/s/song.json", SONG)]));
        host.answer(Request::OpenPath {
            path: PathBuf::from("/s/song.json"),
        });
        assert_eq!(host.window.title(), "song.json - charter");
        assert_eq!(
            host.window.settings().recent,
            [PathBuf::from("/s/song.json")]
        );
        host.answer(Request::OpenPath {
            path: PathBuf::from("/s/gone.json"),
        });
        assert!(host.window.message().unwrap().contains("did not open"));
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
    #[expect(
        clippy::little_endian_bytes,
        reason = "Vorbis comments encode lengths in little-endian order"
    )]
    #[expect(
        clippy::big_endian_bytes,
        reason = "FLAC metadata and frame fields encode lengths and values in big-endian order"
    )]
    fn tagged_audio_offers_its_embedded_cover_beside_the_song() {
        let comment = b"TITLE=Track";
        let mut vorbis = Vec::new();
        vorbis.extend_from_slice(&0_u32.to_le_bytes());
        vorbis.extend_from_slice(&1_u32.to_le_bytes());
        vorbis.extend_from_slice(&u32::try_from(comment.len()).unwrap().to_le_bytes());
        vorbis.extend_from_slice(comment);
        let mut audio = b"fLaC".to_vec();
        audio.extend_from_slice(&[0, 0, 0, 34]);
        let mut info = [0; 34];
        info[..2].copy_from_slice(&4096_u16.to_be_bytes());
        info[2..4].copy_from_slice(&4096_u16.to_be_bytes());
        info[10..18].copy_from_slice(&((8000_u64 << 44) | (15_u64 << 36)).to_be_bytes());
        audio.extend_from_slice(&info);
        audio.extend_from_slice(&[4, 0, 0, u8::try_from(vorbis.len()).unwrap()]);
        audio.extend(vorbis);
        let cover = png(2, 3);
        let mut picture_block = 3_u32.to_be_bytes().to_vec();
        picture_block.extend_from_slice(&9_u32.to_be_bytes());
        picture_block.extend_from_slice(b"image/png");
        picture_block.extend_from_slice(&0_u32.to_be_bytes());
        picture_block.extend_from_slice(&[0; 16]);
        picture_block.extend_from_slice(&u32::try_from(cover.len()).unwrap().to_be_bytes());
        picture_block.extend_from_slice(&cover);
        let length = u32::try_from(picture_block.len()).unwrap().to_be_bytes();
        audio.extend_from_slice(&[0x86, length[1], length[2], length[3]]);
        audio.extend(picture_block);
        let mut frame = vec![0xFF, 0xF8, 0x60, 0x08, 0x00, 15];
        let crc8 = frame.iter().fold(0_u8, |crc, byte| {
            (0..8).fold(crc ^ byte, |crc, _| {
                if crc & 0x80 == 0 {
                    crc << 1
                } else {
                    (crc << 1) ^ 0x07
                }
            })
        });
        frame.push(crc8);
        frame.extend_from_slice(&[0, 0, 0]);
        let crc16 = frame.iter().fold(0_u16, |crc, byte| {
            (0..8).fold(crc ^ (u16::from(*byte) << 8), |crc, _| {
                if crc & 0x8000 == 0 {
                    crc << 1
                } else {
                    (crc << 1) ^ 0x8005
                }
            })
        });
        frame.extend_from_slice(&crc16.to_be_bytes());
        audio.extend(frame);
        assert_eq!(
            charter_audio::tags(audio.clone(), Some("flac"))
                .artwork
                .map(|artwork| artwork.bytes),
            Some(cover.clone())
        );
        let mut host = host(Memory::with(&[("/m/track.flac", &audio)]));
        host.window = Window::new(PLUGINS);
        host.window.open(Path::new("/s/Song/Song.json"), &sm_song());
        host.window.use_audio(Path::new("/m/track.flac"));
        host.window.take_requests();

        host.answer(Request::ReadTags {
            file: PathBuf::from("/m/track.flac"),
        });

        assert_eq!(
            host.window
                .artwork_write_for(Path::new("/s/Song/Song.json")),
            Some((PathBuf::from("/s/Song/cover.png"), cover))
        );
        assert_eq!(host.window.message(), None);
    }

    #[test]
    fn a_song_read_from_the_disk_opens_and_one_missing_says_why() {
        let mut host = host(Memory::with(&[("/s/song.json", SONG)]));
        host.read(Path::new("/s/song.json"));
        assert_eq!(host.window.title(), "song.json - charter");
        host.read(Path::new("/s/other.json"));
        assert!(host.window.message().unwrap().contains("did not open"));
    }

    #[test]
    fn an_unreadable_song_does_not_replace_the_open_song() {
        let mut host = host(Memory {
            unreadable: vec![PathBuf::from("/s/other.json")],
            ..Memory::with(&[("/s/song.json", SONG), ("/s/other.json", SONG)])
        });
        host.read(Path::new("/s/song.json"));
        host.read(Path::new("/s/other.json"));
        assert_eq!(host.window.title(), "song.json - charter");
        assert!(host.window.message().unwrap().contains("access denied"));
    }

    #[test]
    fn saving_as_a_name_that_cannot_name_a_song_leaves_no_project_file() {
        let mut host = host(Memory::default());
        host.save_as(Path::new("/s/.json"), b"mine");
        assert!(!files(&host, "/s/.json"));
        assert!(
            host.window
                .message()
                .unwrap()
                .contains("cannot name a song")
        );
    }

    #[test]
    fn saving_as_when_the_project_folder_cannot_be_made_preserves_the_song() {
        let mut host = host(Memory {
            uncreatable: vec![PathBuf::from("/s/Song")],
            ..Memory::default()
        });
        host.save_as(Path::new("/s/Song.json"), b"new");
        assert!(!files(&host, "/s/Song/Song.json"));
        assert!(
            host.window
                .message()
                .unwrap()
                .contains("folder cannot be created")
        );
    }

    #[test]
    fn saving_as_a_project_with_a_stuck_move_keeps_the_old_song() {
        let mut host = host(Memory {
            stuck: true,
            ..Memory::with(&[("/s/Song/Song.json", b"old")])
        });
        host.save_as(Path::new("/s/Song/Song.json"), b"new");
        assert_eq!(
            host.disk.read(Path::new("/s/Song/Song.json")).unwrap(),
            b"old"
        );
        assert!(!files(&host, "/s/Song/Song.json.partial"));
        assert!(host.window.message().unwrap().contains("move failed"));
    }

    #[test]
    fn an_unreadable_existing_artwork_is_not_overwritten_by_a_new_picture() {
        let mut host = host(Memory {
            unreadable: vec![PathBuf::from("/s/Song/art.png")],
            ..Memory::with(&[("/s/Song/art.png", b"their picture")])
        });
        host.window = Window::new(PLUGINS);
        host.window.open(Path::new("/s/Song/Song.json"), &sm_song());
        host.window
            .use_artwork(Path::new("/p/art.png"), picture(png(2, 2)));
        host.write(PathBuf::from("/s/Song/Song.json"), &sm_song());
        assert!(!files(&host, "/s/Song/Song.json"));
        assert!(!files(&host, "/s/Song/art.png.partial"));
        assert!(host.window.message().unwrap().contains("access denied"));
    }

    #[test]
    fn a_save_cannot_replace_unreadable_artwork_it_could_not_back_up() {
        let mut host = host(Memory {
            unreadable: vec![PathBuf::from("/s/Song/art.png")],
            unwritable: vec![PathBuf::from("/s/Song/Song.json.partial")],
            ..Memory::with(&[
                ("/s/Song/Song.json", b"old song"),
                ("/s/Song/art.png", b"old picture"),
            ])
        });
        host.window = Window::new(PLUGINS);
        let song = sm_song_with(Some("art.png"));
        host.window.open(Path::new("/s/Song/Song.json"), &song);
        host.window
            .use_artwork(Path::new("/p/art.png"), picture(png(2, 2)));
        host.write(PathBuf::from("/s/Song/Song.json"), &song);
        assert_eq!(
            host.disk.read(Path::new("/s/Song/Song.json")).unwrap(),
            b"old song"
        );
        assert!(files(&host, "/s/Song/art.png"));
        assert!(host.window.message().unwrap().contains("access denied"));
    }

    #[test]
    fn an_unwritable_artwork_prevents_the_song_from_being_saved() {
        let mut host = host(Memory {
            unwritable: vec![PathBuf::from("/s/Song/art.png.partial")],
            ..Memory::default()
        });
        host.window = Window::new(PLUGINS);
        host.window.open(Path::new("/s/Song/Song.json"), &sm_song());
        host.window
            .use_artwork(Path::new("/p/art.png"), picture(png(2, 2)));
        host.write(PathBuf::from("/s/Song/Song.json"), &sm_song());
        assert!(!files(&host, "/s/Song/Song.json"));
        assert!(!files(&host, "/s/Song/art.png"));
        assert!(host.window.message().unwrap().contains("disk is full"));
    }

    #[test]
    fn a_failed_song_write_removes_new_artwork_and_preserves_the_old_song() {
        let mut host = host(Memory {
            unwritable: vec![PathBuf::from("/s/Song/Song.json.partial")],
            ..Memory::with(&[("/s/Song/Song.json", b"old song")])
        });
        host.window = Window::new(PLUGINS);
        host.window.open(Path::new("/s/Song/Song.json"), &sm_song());
        host.window
            .use_artwork(Path::new("/p/art.png"), picture(png(2, 2)));
        host.write(PathBuf::from("/s/Song/Song.json"), &sm_song());
        assert_eq!(
            host.disk.read(Path::new("/s/Song/Song.json")).unwrap(),
            b"old song"
        );
        assert!(!files(&host, "/s/Song/art.png"));
        assert!(!files(&host, "/s/Song/Song.json.partial"));
        assert!(host.window.message().unwrap().contains("disk is full"));
    }

    #[test]
    fn an_autosave_with_a_stuck_move_preserves_the_last_recovery() {
        let mut host = host(Memory {
            stuck: true,
            ..Memory::with(&[("/data/recovery.json", b"older changes")])
        });
        host.autosave(b"new changes");
        assert_eq!(
            host.disk.read(Path::new("/data/recovery.json")).unwrap(),
            b"older changes"
        );
        assert!(!files(&host, "/data/recovery.json.partial"));
        assert!(host.window.message().unwrap().contains("move failed"));
    }

    #[test]
    fn autosaving_without_a_recovery_folder_reports_the_missing_location() {
        let mut host = host(Memory::default());
        host.recovery = None;
        host.autosave(b"changes");
        assert_eq!(
            host.window.message(),
            Some("Autosave failed: there is no folder to keep it in")
        );
    }

    #[test]
    fn an_unreadable_timing_file_reports_the_failure_without_importing_it() {
        let mut host = host(Memory {
            unreadable: vec![PathBuf::from("/m/map.osu")],
            ..Memory::with(&[("/m/map.osu", b"[TimingPoints]\n0,500,4,1,0,100,1,0")])
        });
        host.read_timing(Path::new("/m/map.osu"));
        let message = host.window.message().unwrap();
        assert!(message.contains("Could not read the timing"));
        assert!(message.contains("map.osu") && message.contains("access denied"));
    }

    /// A new Tap Tap Revenge song as its game's own track file.
    fn ttr2_track() -> Vec<u8> {
        let song = charter_core::NewSong {
            metadata: charter_core::Metadata {
                title: "sudo".to_owned(),
                artist: "Mr. Bill".to_owned(),
                audio: charter_core::ResourcePath::new("audio.ogg").unwrap(),
                preview: None,
                artwork: None,
                genre: None,
                ext: charter_core::Extensions::new(),
            },
            layout: 0,
            charts: vec![charter_core::NewChart {
                name: "Extreme".to_owned(),
                level: Some(3),
            }],
        };
        let document = charter_ttr::Plugin.new_song(&song).unwrap();
        charter_ttr::Plugin
            .export_song(0, &document, None)
            .unwrap()
            .bytes
    }

    /// A plugin with one song format, read with an `info.plist` beside it, whose import fails
    /// naming the companions it was handed.
    #[derive(Debug)]
    struct Companions;

    #[expect(
        clippy::missing_trait_methods,
        reason = "a test plugin: the trait's defaults for everything but importing"
    )]
    impl charter_core::RulesetPlugin for Companions {
        fn id(&self) -> &'static str {
            "companions"
        }

        fn check(
            &self,
            _: &charter_core::ValidDocument,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        fn song_formats(&self) -> Vec<charter_core::SongFormat> {
            vec![charter_core::SongFormat {
                name: "track".to_owned(),
                extensions: vec!["ttr2_track".to_owned()],
                companions: vec!["info.plist".to_owned(), "missing.plist".to_owned()],
            }]
        }

        fn import_song(
            &self,
            _: usize,
            bytes: &[u8],
            companions: &std::collections::BTreeMap<String, Vec<u8>>,
        ) -> Result<charter_core::ImportedSong, Box<dyn std::error::Error + Send + Sync>> {
            let found: Vec<String> = companions
                .iter()
                .map(|(name, beside)| format!("{name} {}", String::from_utf8_lossy(beside)))
                .collect();
            Err(format!("{} with {found:?}", String::from_utf8_lossy(bytes)).into())
        }
    }

    #[test]
    fn an_import_is_read_from_disk_with_the_companions_found_beside_it() {
        let path = "/t/sudo.track/taptrack.ttr2_track";
        let mut host = host(Memory::with(&[
            (path, b"notes"),
            ("/t/sudo.track/info.plist", b"columns"),
        ]));
        host.window = Window::new(&[&Companions]);
        host.read_import(
            Path::new(path),
            &["info.plist".to_owned(), "missing.plist".to_owned()],
        );
        assert_eq!(
            host.window.message(),
            Some(
                "/t/sudo.track/taptrack.ttr2_track did not import: notes with [\"info.plist columns\"]"
            )
        );
    }

    #[test]
    fn a_track_several_games_read_waits_on_which_before_opening() {
        let path = "/t/sudo.track/taptrack.ttr2_track";
        let mut host = host(Memory::with(&[(path, &ttr2_track())]));
        host.window = Window::new(PLUGINS);
        host.read_import(Path::new(path), &["info.plist".to_owned()]);
        assert_eq!(host.window.message(), None);
        assert_eq!(host.window.title(), "charter");
    }

    #[test]
    fn an_unreadable_import_says_why_and_opens_nothing() {
        let path = "/t/notes.mid";
        let mut host = host(Memory {
            unreadable: vec![PathBuf::from(path)],
            ..Memory::with(&[(path, b"MThd")])
        });
        host.window = Window::new(PLUGINS);
        host.read_import(Path::new(path), &[]);
        let message = host.window.message().unwrap();
        assert!(message.contains("notes.mid did not open") && message.contains("access denied"));
        assert_eq!(host.window.title(), "charter");
    }

    #[test]
    fn an_export_is_written_whole_or_its_failure_reported() {
        let mut host = host(Memory {
            unwritable: vec![PathBuf::from("/full/notes.mid.partial")],
            ..Memory::default()
        });
        let export = |path: &str| Request::WriteExport {
            path: PathBuf::from(path),
            bytes: b"MThd".to_vec(),
            beside: Vec::new(),
            copies: Vec::new(),
        };
        host.handle_requests(vec![export("/out/notes.mid")]);
        assert_eq!(
            host.disk.read(Path::new("/out/notes.mid")).unwrap(),
            b"MThd"
        );
        host.handle_requests(vec![export("/full/notes.mid")]);
        assert!(
            host.window
                .message()
                .unwrap()
                .starts_with("/full/notes.mid was not written")
        );
    }

    #[test]
    fn a_track_folder_is_made_with_its_files_and_copies_replacing_old_ones() {
        let mut host = host(Memory::with(&[
            ("/songs/song.mp3", b"new audio"),
            ("/out/sudo.track/audio.m4a", b"old audio"),
        ]));
        let folder = PathBuf::from("/out/sudo.track");
        host.handle_requests(vec![Request::WriteExport {
            path: folder.join("taptrack.ttr2_track"),
            bytes: b"bplist00".to_vec(),
            beside: vec![(folder.join("info.plist"), b"<plist/>".to_vec())],
            copies: vec![
                (PathBuf::from("/songs/song.mp3"), folder.join("audio.m4a")),
                (PathBuf::from("/songs/gone.png"), folder.join("artwork.png")),
            ],
        }]);
        let read = |name: &str| host.disk.read(&folder.join(name)).unwrap();
        assert_eq!(read("taptrack.ttr2_track"), b"bplist00");
        assert_eq!(read("info.plist"), b"<plist/>");
        assert_eq!(read("audio.m4a"), b"new audio");
        assert!(
            host.window
                .message()
                .unwrap()
                .starts_with("/out/sudo.track/artwork.png was not written")
        );
    }

    #[test]
    fn project_file_keeps_names_with_dots_and_does_not_nest_an_existing_project() {
        assert_eq!(
            project_file(Path::new("/s/A.B.json")),
            Some(PathBuf::from("/s/A.B/A.B.json"))
        );
        assert_eq!(
            project_file(Path::new("/s/A.B/A.B.json")),
            Some(PathBuf::from("/s/A.B/A.B.json"))
        );
        assert_eq!(project_file(Path::new("/s/.json")), None);
        assert_eq!(project_file(Path::new("/")), None);
    }

    #[test]
    fn reading_artwork_from_disk_makes_its_original_bytes_available_for_a_new_project() {
        let image = png(2, 3);
        let mut host = host(Memory::with(&[("/s/Song/art.png", &image)]));
        host.window = Window::new(PLUGINS);
        host.window.open(
            Path::new("/s/Song/Song.json"),
            &sm_song_with(Some("art.png")),
        );
        host.window
            .use_artwork(Path::new("/p/art.png"), picture(png(1, 1)));
        host.window
            .saved(Saved::To(PathBuf::from("/s/Song/Song.json")));
        host.answer(Request::ReadArtwork {
            file: PathBuf::from("/s/Song/art.png"),
        });
        assert_eq!(
            host.window.artwork_write_for(Path::new("/other/Song.json")),
            Some((PathBuf::from("/other/art.png"), image))
        );
    }

    #[test]
    fn unreadable_artwork_is_not_offered_for_copying_to_a_new_project() {
        let mut host = host(Memory {
            unreadable: vec![PathBuf::from("/s/Song/art.png")],
            ..Memory::with(&[("/s/Song/art.png", b"not readable")])
        });
        host.window = Window::new(PLUGINS);
        host.window.open(
            Path::new("/s/Song/Song.json"),
            &sm_song_with(Some("art.png")),
        );
        host.window
            .use_artwork(Path::new("/p/art.png"), picture(png(1, 1)));
        host.window
            .saved(Saved::To(PathBuf::from("/s/Song/Song.json")));
        host.answer(Request::ReadArtwork {
            file: PathBuf::from("/s/Song/art.png"),
        });
        assert_eq!(
            host.window.artwork_write_for(Path::new("/other/Song.json")),
            None
        );
    }

    #[test]
    fn decoding_autosaved_artwork_restores_its_pending_file_and_bytes() {
        let image = png(2, 3);
        let mut host = host(Memory::default());
        host.window = Window::new(PLUGINS);
        host.window.open(
            Path::new("/s/Song/Song.json"),
            &sm_song_with(Some("art.png")),
        );
        host.answer(Request::DecodeArtwork {
            bytes: image.clone(),
        });
        assert_eq!(
            host.window
                .artwork_write_for(Path::new("/s/Song/Song.json")),
            Some((PathBuf::from("/s/Song/art.png"), image))
        );
    }

    #[test]
    fn changed_settings_take_precedence_over_the_older_whole_copy() {
        let mut storage = Kept::default();
        eframe::Storage::set_string(
            &mut storage,
            OLD_SETTINGS,
            r#"{"zoom":125.0,"tick_sound":"beep"}"#.to_owned(),
        );
        eframe::Storage::set_string(&mut storage, SETTINGS, r#"{"zoom":175.0}"#.to_owned());
        let settings = kept_settings(&storage);
        assert!((settings.zoom - 175.0).abs() < f32::EPSILON);
        assert!(!settings.note_ticks);
    }

    #[test]
    fn saving_settings_keeps_only_changes_for_the_next_run() {
        let mut host = host(Memory::default());
        host.window = Window::new(&[]).with_settings(Settings {
            zoom: 175.0,
            ..Settings::default()
        });
        let mut storage = Kept::default();
        eframe::App::save(&mut host, &mut storage);
        assert_eq!(
            eframe::Storage::get_string(&storage, SETTINGS).as_deref(),
            Some(r#"{"zoom":175.0}"#)
        );
        assert!((kept_settings(&storage).zoom - 175.0).abs() < f32::EPSILON);
    }
}
