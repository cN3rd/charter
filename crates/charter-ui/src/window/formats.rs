//! Songs read from and written to other programs' formats, as the ruleset plugins offer them.
//! The window asks the host for the file, and the plugin whose format names its extension reads
//! or writes it; the window never touches the file itself.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use charter_core::{RulesetPlugin, SongFormat};
use charter_editor::Song;

use super::{Discard, Request, Window};
use crate::panels;

/// A format a file may be: its plugin, its index there and its name.
type Choice = (&'static dyn RulesetPlugin, usize, String);

/// A file waiting on which of the formats sharing its extension it is.
#[derive(Debug)]
pub(super) struct Choosing {
    /// The file and what to do with it.
    file: Pending,
    /// The formats it may be.
    formats: Vec<Choice>,
    /// The one picked so far, by index into `formats`.
    chosen: usize,
}

/// A file to import or export once its format is known.
#[derive(Debug)]
enum Pending {
    /// A song read from `path`, with the files found beside it.
    Import {
        /// Where it was read from.
        path: PathBuf,
        /// What it holds.
        bytes: Vec<u8>,
        /// Files beside it, by name.
        companions: BTreeMap<String, Vec<u8>>,
    },
    /// The open song, to write to `path`.
    Export {
        /// Where to write it.
        path: PathBuf,
    },
}

impl Window {
    /// Each song format the plugins offer, with the plugin and its index there.
    fn song_formats(
        &self,
    ) -> impl Iterator<Item = (&'static dyn RulesetPlugin, usize, charter_core::SongFormat)> {
        self.plugins.iter().flat_map(|&plugin| {
            plugin
                .song_formats()
                .into_iter()
                .enumerate()
                .map(move |(index, format)| (plugin, index, format))
        })
    }

    /// Whether any plugin reads songs from another program's format.
    pub(super) fn songs_importable(&self) -> bool {
        self.song_formats().next().is_some()
    }

    /// The plugin for the open song's ruleset, if the song is open and a plugin is its.
    fn song_plugin(&self) -> Option<&'static dyn RulesetPlugin> {
        let song = self.song.as_ref()?;
        let id = song.document().document().ruleset.id.as_str();
        self.plugins
            .iter()
            .copied()
            .find(|plugin| plugin.id() == id)
    }

    /// Whether the open song's plugin writes it in another program's format.
    pub(super) fn song_exportable(&self) -> bool {
        self.song_plugin()
            .is_some_and(|plugin| !plugin.song_formats().is_empty())
    }

    /// Asks the host for a song in any format a plugin reads, with the files any of them reads
    /// beside it.
    pub(super) fn ask_import(&mut self) {
        let mut companions: Vec<String> = Vec::new();
        let formats = self
            .song_formats()
            .map(|(_, _, format)| {
                companions.extend(format.companions);
                (format.name, format.extensions)
            })
            .collect();
        companions.sort();
        companions.dedup();
        self.requests.push(Request::ChooseImport {
            formats,
            companions,
        });
    }

    /// Asks the host where to write the open song in a format its plugin writes.
    pub(super) fn ask_export(&mut self) {
        let (Some(song), Some(plugin)) = (&self.song, self.song_plugin()) else {
            return;
        };
        let formats: Vec<_> = plugin
            .song_formats()
            .into_iter()
            .map(|format| (format.name, format.extensions))
            .collect();
        if formats.is_empty() {
            return;
        }
        let name = song.document().document().metadata.title.clone();
        self.requests.push(Request::ChooseExport { name, formats });
    }

    /// Opens the song in `bytes`, read from `path` with the `companions` beside it, as a new song
    /// with no file of its own, and says what it left out. The plugin whose format names the
    /// extension reads it; when formats of several share it, the user picks which. Its audio is
    /// the file the format names beside `path`.
    pub fn import_song(
        &mut self,
        path: &Path,
        bytes: &[u8],
        companions: &BTreeMap<String, Vec<u8>>,
    ) {
        let extension = extension(path);
        let formats: Vec<Choice> = self
            .song_formats()
            .filter(|(_, _, format)| named(format, extension.as_deref()))
            .map(|(plugin, index, format)| (plugin, index, format.name))
            .collect();
        let file = Pending::Import {
            path: path.to_path_buf(),
            bytes: bytes.to_vec(),
            companions: companions.clone(),
        };
        match formats.as_slice() {
            [] => {
                self.failure = Some(format!(
                    "{} did not import: no game reads that kind of file",
                    path.display()
                ));
            }
            [only] => self.finish(only.clone(), file),
            _ => {
                self.dialogs.choosing = Some(Choosing {
                    file,
                    formats,
                    chosen: 0,
                });
            }
        }
    }

    /// Writes the open song to `path` in the format of its plugin that names the extension, or
    /// its first with that extension added, and says what the file leaves out. When several of
    /// its formats share the extension, the user picks which.
    pub fn export_song(&mut self, path: &Path) {
        let Some(plugin) = self.song_plugin().filter(|_| self.song.is_some()) else {
            return;
        };
        let all = plugin.song_formats();
        let extension = extension(path);
        let mut formats: Vec<Choice> = all
            .iter()
            .enumerate()
            .filter(|(_, format)| named(format, extension.as_deref()))
            .map(|(index, format)| (plugin, index, format.name.clone()))
            .collect();
        let mut path = path.to_path_buf();
        if formats.is_empty()
            && let Some(first) = all.first()
        {
            path.set_extension(first.extensions.first().map_or("", String::as_str));
            formats.push((plugin, 0, first.name.clone()));
        }
        let file = Pending::Export { path };
        match formats.as_slice() {
            [] => {}
            [only] => self.finish(only.clone(), file),
            _ => {
                self.dialogs.choosing = Some(Choosing {
                    file,
                    formats,
                    chosen: 0,
                });
            }
        }
    }

    /// Asks which format the file waiting on it is, and carries on once the user says.
    pub(super) fn choose_format(&mut self, ui: &egui::Ui) {
        let Some(choosing) = &mut self.dialogs.choosing else {
            return;
        };
        let names: Vec<String> = choosing
            .formats
            .iter()
            .map(|(_, _, name)| name.clone())
            .collect();
        let verb = match choosing.file {
            Pending::Import { .. } => "Import",
            Pending::Export { .. } => "Export",
        };
        let Some(answer) = panels::choose_format(ui, verb, &names, &mut choosing.chosen) else {
            return;
        };
        let Some(chosen) = self.dialogs.choosing.take() else {
            return;
        };
        if answer && let Some(choice) = chosen.formats.get(chosen.chosen) {
            self.finish(choice.clone(), chosen.file);
        }
    }

    /// Imports or exports `file` as the format `choice`.
    fn finish(&mut self, choice: Choice, file: Pending) {
        let (plugin, index, name) = choice;
        match file {
            Pending::Import {
                path,
                bytes,
                companions,
            } => {
                let imported = match plugin.import_song(index, &bytes, &companions) {
                    Ok(imported) => imported,
                    Err(why) => {
                        self.failure = Some(format!("{} did not import: {why}", path.display()));
                        return;
                    }
                };
                let audio = path.parent().map(|folder| {
                    folder.join(imported.document.document().metadata.audio.as_str())
                });
                let note = described(
                    &format!("Imported {name} as a {} song", plugin.name()),
                    &imported.left_out,
                );
                self.after_asking(Discard::Open {
                    path: None,
                    song: Box::new(Song::new(imported.document, self.plugins)),
                    restored: false,
                    audio,
                    note: Some(note),
                });
            }
            Pending::Export { path } => {
                let Some(song) = &self.song else {
                    return;
                };
                match plugin.export_song(index, song.document()) {
                    Ok(exported) => {
                        self.failure = Some(described(
                            &format!("Exported {name} to {}", path.display()),
                            &exported.left_out,
                        ));
                        self.requests.push(Request::WriteExport {
                            path,
                            bytes: exported.bytes,
                        });
                    }
                    Err(why) => {
                        self.failure = Some(format!("The song did not export: {why}"));
                    }
                }
            }
        }
    }

    /// Shows why the exported file could not be written to `path`.
    pub fn export_failed(&mut self, path: &Path, why: &str) {
        self.failure = Some(format!("{} was not written: {why}", path.display()));
    }
}

/// Whether `format` names `extension` among its own.
fn named(format: &SongFormat, extension: Option<&str>) -> bool {
    format
        .extensions
        .iter()
        .any(|known| Some(known.to_lowercase().as_str()) == extension)
}

/// `path`'s extension, in lower case.
fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_lowercase)
}

/// `done`, and what it left out when it left anything.
fn described(done: &str, left_out: &[String]) -> String {
    if left_out.is_empty() {
        format!("{done}.")
    } else {
        format!("{done}, leaving out {}.", left_out.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use egui_kittest::Harness;

    use charter_core::{Metadata, NewChart, NewSong, ResourcePath, RulesetPlugin};

    use crate::window::test_support::*;
    use crate::window::{Action, Note, Request, Window};

    /// A new Tap Tap Revenge song on its Extreme level, as its plugin starts one.
    fn ttr_song() -> charter_core::ValidDocument {
        let song = NewSong {
            metadata: Metadata {
                title: "sudo".to_owned(),
                artist: "Mr. Bill".to_owned(),
                audio: ResourcePath::new("audio.ogg").unwrap(),
                preview: None,
                artwork: None,
                genre: None,
                ext: charter_core::Extensions::new(),
            },
            layout: 0,
            charts: vec![NewChart {
                name: "Extreme".to_owned(),
                level: Some(3),
            }],
        };
        charter_ttr::Plugin.new_song(&song).unwrap()
    }

    /// A window of the games that start songs, with `song` open from `/songs/sudo.json`.
    fn opened(song: &charter_core::ValidDocument) -> Window {
        let mut app = Window::new(GAMES);
        app.open(
            std::path::Path::new("/songs/sudo.json"),
            &song.to_json().unwrap(),
        );
        app
    }

    /// Picks `format` in the dialog asking which game a file is for, then `verb`.
    fn pick(harness: &mut Harness<'_, Window>, format: &str, verb: &str) {
        harness.run();
        harness.get_by_label(format).click();
        harness.run();
        harness.get_by_label(verb).click();
        harness.run();
    }

    /// The open song's first chart's note kinds, as the document names them.
    fn kinds(app: &Window) -> Vec<String> {
        let song = app.song.as_ref().unwrap();
        let chart = song.document().charts().next().unwrap();
        chart
            .objects()
            .map(|object| object.kind.to_string())
            .collect()
    }

    #[test]
    fn importing_asks_for_any_format_a_game_reads_and_the_files_beside_it() {
        let mut app = Window::new(GAMES);
        assert!(app.songs_importable());
        assert!(!Window::new(PLUGINS).songs_importable());
        app.apply(Action::ImportSong);
        let requests = app.take_requests();
        let [
            Request::ChooseImport {
                formats,
                companions,
            },
        ] = requests.as_slice()
        else {
            panic!("an import asks for a file");
        };
        let names: Vec<&str> = formats.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Tap Tap Revenge track",
                "Tap Tap Revenge: Tour track",
                "Tap Tap Revenge track, custom tap theme",
                "Editor on Fire MIDI",
                "Tap Tap Reloaded MIDI",
            ]
        );
        assert_eq!(companions, &["info.plist"]);
    }

    #[test]
    fn a_track_opens_as_the_game_picked_for_it_unsaved_with_its_audio_beside_it() {
        let mut song = opened(&ttr_song());
        song.apply(Action::Place {
            tick: charter_core::Tick::new(0),
            lane: charter_core::LaneIndex::new(2),
            note: Note::Tap,
        });
        song.apply(Action::Place {
            tick: charter_core::Tick::new(960),
            lane: charter_core::LaneIndex::new(1),
            note: Note::Tap,
        });
        let track = charter_ttr::Plugin
            .export_song(0, song.song.as_ref().unwrap().document())
            .unwrap()
            .bytes;
        let mut harness = window(Window::new(GAMES));
        let path = PathBuf::from("/tracks/sudo.track/taptrack.TTR2_TRACK");
        harness
            .state_mut()
            .import_song(&path, &track, &BTreeMap::new());
        assert!(harness.state().song.is_none(), "it waits on the game");
        pick(&mut harness, "Tap Tap Revenge: Tour track", "Import");
        let app = harness.state();
        let opened = app.song.as_ref().unwrap();
        assert_eq!(opened.document().document().metadata.title, "sudo");
        assert!(opened.read_only().is_none());
        assert_eq!(app.path, None);
        assert_eq!(
            app.audio_file(),
            Some(PathBuf::from("/tracks/sudo.track/audio.m4a"))
        );
        assert_eq!(
            app.message(),
            Some("Imported Tap Tap Revenge: Tour track as a Tap Tap Revenge song.")
        );
        assert_eq!(kinds(app), ["tap", "tap"]);
    }

    #[test]
    fn cancelling_the_game_leaves_the_song_open() {
        let mut harness = window(opened(&ttr_song()));
        harness.state_mut().import_song(
            std::path::Path::new("/a/b.mid"),
            b"MThd",
            &BTreeMap::new(),
        );
        harness.run();
        harness.get_by_label("Cancel").click();
        harness.run();
        assert!(harness.state().dialogs.choosing.is_none());
        assert_eq!(
            harness.state().path,
            Some(PathBuf::from("/songs/sudo.json"))
        );
        assert_eq!(harness.state().message(), None);
    }

    #[test]
    fn a_file_no_game_reads_or_that_does_not_read_is_refused_and_leaves_the_song_open() {
        let mut harness = window(opened(&ttr_song()));
        harness
            .state_mut()
            .import_song(std::path::Path::new("/a/b.osu"), b"", &BTreeMap::new());
        assert_eq!(
            harness.state().message(),
            Some("/a/b.osu did not import: no game reads that kind of file")
        );
        harness.state_mut().import_song(
            std::path::Path::new("/a/b.mid"),
            b"not midi",
            &BTreeMap::new(),
        );
        pick(&mut harness, "Tap Tap Reloaded MIDI", "Import");
        assert!(
            harness
                .state()
                .message()
                .unwrap()
                .starts_with("/a/b.mid did not import: not a MIDI file")
        );
        assert_eq!(
            harness.state().path,
            Some(PathBuf::from("/songs/sudo.json"))
        );
    }

    #[test]
    fn an_import_says_what_it_left_out_even_after_asking_about_unsaved_changes() {
        let mut app = opened(&ttr_song());
        app.apply(Action::Place {
            tick: charter_core::Tick::new(0),
            lane: charter_core::LaneIndex::new(0),
            note: Note::Tap,
        });
        assert!(app.modified());
        let midi = charter_ttr::Plugin.export_song(3, app.song.as_ref().unwrap().document());
        let mut harness = window(app);
        harness.state_mut().import_song(
            std::path::Path::new("/eof/notes.mid"),
            &midi.unwrap().bytes,
            &BTreeMap::new(),
        );
        pick(&mut harness, "Editor on Fire MIDI", "Import");
        harness.get_by_label("Don't save").click();
        harness.run();
        assert_eq!(
            harness.state().message(),
            Some("Imported Editor on Fire MIDI as a Tap Tap Revenge song.")
        );
        let song = harness.state().song.as_ref().unwrap();
        assert_eq!(song.document().document().metadata.artist, "");
    }

    #[test]
    fn exporting_writes_the_picked_format_and_says_what_the_file_leaves_out() {
        let mut app = opened(&ttr_song());
        assert!(app.song_exportable());
        app.apply(Action::ExportSong);
        let requests = app.take_requests();
        let [Request::ChooseExport { name, formats }] = requests.as_slice() else {
            panic!("an export asks where");
        };
        assert_eq!(name, "sudo");
        assert_eq!(formats.len(), 5);
        let mut harness = window(app);
        harness
            .state_mut()
            .export_song(std::path::Path::new("/out/notes.mid"));
        pick(&mut harness, "Editor on Fire MIDI", "Export");
        let taken = harness.state_mut().take_requests();
        let written: Vec<_> = taken
            .iter()
            .filter_map(|request| {
                if let Request::WriteExport { path, bytes } = request {
                    Some((path, bytes))
                } else {
                    None
                }
            })
            .collect();
        let [(path, bytes)] = written.as_slice() else {
            panic!("an export writes a file: {taken:?}");
        };
        assert_eq!(*path, &PathBuf::from("/out/notes.mid"));
        assert!(bytes.starts_with(b"MThd"));
        assert_eq!(
            harness.state().message(),
            Some(
                "Exported Editor on Fire MIDI to /out/notes.mid, leaving out the artist, which EOF \
                 keeps in song.ini rather than the MIDI file; the theme, which a track's \
                 info.plist holds, not this file."
            )
        );
    }

    #[test]
    fn a_file_named_for_no_format_is_written_as_the_first_with_its_extension() {
        let mut app = opened(&ttr_song());
        app.export_song(std::path::Path::new("/out/taptrack"));
        let requests = app.take_requests();
        let [Request::WriteExport { path, bytes }] = requests.as_slice() else {
            panic!("an export writes a file");
        };
        assert_eq!(path, &PathBuf::from("/out/taptrack.ttr2_track"));
        assert!(bytes.starts_with(b"bplist00"));
        app.export_failed(path, "disk full");
        assert_eq!(
            app.message(),
            Some("/out/taptrack.ttr2_track was not written: disk full")
        );
    }

    #[test]
    fn a_song_whose_game_writes_no_other_format_does_not_export() {
        let mut app = Window::new(PLUGINS);
        app.open_demo();
        assert!(!app.song_exportable());
        app.apply(Action::ExportSong);
        app.export_song(std::path::Path::new("/out/song.mid"));
        assert_eq!(app.take_requests(), []);
    }
}
