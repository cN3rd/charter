//! Songs read from and written to other programs' formats, as the ruleset plugins offer them.
//! The window asks the host for the file, and the plugin whose format names its extension reads
//! or writes it; the window never touches the file itself.

use std::path::Path;

use charter_core::RulesetPlugin;
use charter_editor::Song;

use super::{Discard, Request, Window};

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

    /// Asks the host for a song in any format a plugin reads.
    pub(super) fn ask_import(&mut self) {
        let formats = self
            .song_formats()
            .map(|(_, _, format)| (format.name, format.extensions))
            .collect();
        self.requests.push(Request::ChooseImport { formats });
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

    /// Opens the song in `bytes`, read from `path` by the plugin whose format names its
    /// extension, as a new song with no file of its own, and says what it left out. Its audio is
    /// the file the format names beside `path`.
    pub fn import_song(&mut self, path: &Path, bytes: &[u8]) {
        let extension = extension(path);
        let found = self.song_formats().find(|(_, _, format)| {
            format
                .extensions
                .iter()
                .any(|known| Some(known.to_lowercase()) == extension)
        });
        let Some((plugin, index, format)) = found else {
            self.failure = Some(format!(
                "{} did not import: no game reads that kind of file",
                path.display()
            ));
            return;
        };
        let imported = match plugin.import_song(index, bytes) {
            Ok(imported) => imported,
            Err(why) => {
                self.failure = Some(format!("{} did not import: {why}", path.display()));
                return;
            }
        };
        let audio = path
            .parent()
            .map(|folder| folder.join(imported.document.document().metadata.audio.as_str()));
        let note = described(
            &format!("Imported {} as a {} song", format.name, plugin.name()),
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

    /// Writes the open song to `path` in the format of its plugin that names the extension, or
    /// its first with that extension added, and says what the file leaves out.
    pub fn export_song(&mut self, path: &Path) {
        let (Some(song), Some(plugin)) = (&self.song, self.song_plugin()) else {
            return;
        };
        let formats = plugin.song_formats();
        let extension = extension(path);
        let named = formats.iter().position(|format| {
            format
                .extensions
                .iter()
                .any(|known| Some(known.to_lowercase()) == extension)
        });
        let (index, path) = match (named, formats.first()) {
            (Some(index), _) => (index, path.to_path_buf()),
            (None, Some(first)) => {
                let added = first.extensions.first().map_or("", String::as_str);
                (0, path.with_extension(added))
            }
            (None, None) => return,
        };
        let name = formats.get(index).map_or("", |format| format.name.as_str());
        match plugin.export_song(index, song.document()) {
            Ok(exported) => {
                self.failure = Some(described(
                    &format!("Exported {} to {}", name, path.display()),
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

    /// Shows why the exported file could not be written to `path`.
    pub fn export_failed(&mut self, path: &Path, why: &str) {
        self.failure = Some(format!("{} was not written: {why}", path.display()));
    }
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
    use std::path::PathBuf;

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

    #[test]
    fn importing_asks_for_any_format_a_game_reads() {
        let mut app = Window::new(GAMES);
        assert!(app.songs_importable());
        assert!(!Window::new(PLUGINS).songs_importable());
        app.apply(Action::ImportSong);
        let requests1 = app.take_requests();
        let [Request::ChooseImport { formats }] = requests1.as_slice() else {
            panic!("an import asks for a file");
        };
        let names: Vec<&str> = formats.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["Tap Tap Revenge track", "Editor on Fire MIDI"]);
    }

    #[test]
    fn an_imported_song_opens_unsaved_with_its_audio_beside_the_file() {
        let track = charter_ttr::Plugin
            .export_song(0, &ttr_song())
            .unwrap()
            .bytes;
        let mut app = Window::new(GAMES);
        let path = PathBuf::from("/tracks/sudo.track/taptrack.TTR2_TRACK");
        app.import_song(&path, &track);
        let song = app.song.as_ref().unwrap();
        assert_eq!(song.document().document().metadata.title, "sudo");
        assert!(song.read_only().is_none());
        assert_eq!(app.path, None);
        assert_eq!(
            app.audio_file(),
            Some(PathBuf::from("/tracks/sudo.track/audio.m4a"))
        );
        assert_eq!(
            app.message(),
            Some("Imported Tap Tap Revenge track as a Tap Tap Revenge song.")
        );
    }

    #[test]
    fn a_file_no_game_reads_or_that_does_not_read_is_refused_and_leaves_the_song_open() {
        let mut app = opened(&ttr_song());
        app.import_song(std::path::Path::new("/a/b.osu"), b"");
        assert_eq!(
            app.message(),
            Some("/a/b.osu did not import: no game reads that kind of file")
        );
        app.import_song(std::path::Path::new("/a/b.mid"), b"not midi");
        assert!(
            app.message()
                .unwrap()
                .starts_with("/a/b.mid did not import: not a MIDI file")
        );
        assert_eq!(app.path, Some(PathBuf::from("/songs/sudo.json")));
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
        let midi = charter_ttr::Plugin.export_song(1, app.song.as_ref().unwrap().document());
        let mut harness = window(app);
        harness
            .state_mut()
            .import_song(std::path::Path::new("/eof/notes.mid"), &midi.unwrap().bytes);
        harness.run();
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
    fn exporting_writes_the_chosen_format_and_says_what_the_file_leaves_out() {
        let mut app = opened(&ttr_song());
        assert!(app.song_exportable());
        app.apply(Action::ExportSong);
        let requests2 = app.take_requests();
        let [Request::ChooseExport { name, formats }] = requests2.as_slice() else {
            panic!("an export asks where");
        };
        assert_eq!(name, "sudo");
        assert_eq!(formats.len(), 2);
        app.export_song(std::path::Path::new("/out/notes.mid"));
        let requests3 = app.take_requests();
        let [Request::WriteExport { path, bytes }] = requests3.as_slice() else {
            panic!("an export writes a file");
        };
        assert_eq!(path, &PathBuf::from("/out/notes.mid"));
        assert!(bytes.starts_with(b"MThd"));
        assert_eq!(
            app.message(),
            Some(
                "Exported Editor on Fire MIDI to /out/notes.mid, leaving out the artist, which EOF \
                 keeps in song.ini rather than the MIDI file; the theme, which a track's \
                 info.plist holds, not this file."
            )
        );
        app.export_song(std::path::Path::new("/out/taptrack"));
        let requests4 = app.take_requests();
        let [
            Request::WriteExport {
                path: named,
                bytes: track,
            },
        ] = requests4.as_slice()
        else {
            panic!("an export writes a file");
        };
        assert_eq!(named, &PathBuf::from("/out/taptrack.ttr2_track"));
        assert!(track.starts_with(b"bplist00"));
        app.export_failed(named, "disk full");
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
