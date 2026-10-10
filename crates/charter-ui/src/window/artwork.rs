//! Artwork selection, restoration and save-side bookkeeping.

use super::{Artwork, EmbeddedArtwork, Path, PathBuf, Picture, Request, Song, Window, refused};
use charter_editor::HistoryMark;

impl Window {
    /// Takes a chosen PNG or JPEG picture as the song's artwork.
    pub fn use_artwork(&mut self, file: &Path, picture: Option<Picture>) {
        let Some(picture) = picture else {
            self.failure = Some(format!(
                "{} cannot be the song's artwork: it is not a PNG or JPEG picture",
                file.display()
            ));
            return;
        };
        let name = file
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        self.take_artwork(name.unwrap_or_default(), picture);
    }
    /// Takes the cover embedded in `file`, an audio file chosen as the song's artwork, as
    /// `cover.<extension>` beside the song; `cover` is its extension and picture, or `None` when
    /// its tags hold no picture.
    pub fn artwork_ripped(&mut self, file: &Path, cover: Option<(&str, Picture)>) {
        let Some((extension, picture)) = cover else {
            self.failure = Some(format!(
                "{} cannot give the song's artwork: it has no cover in its tags",
                file.display()
            ));
            return;
        };
        self.take_artwork(format!("cover.{extension}"), picture);
    }
    /// Stores cover art embedded in a chosen audio file, or applies it to a song without artwork.
    pub fn artwork_embedded(&mut self, file: &Path, extension: &str, picture: Picture) {
        if self
            .dialogs
            .new_song
            .as_ref()
            .is_some_and(|draft| draft.audio() == Some(file))
        {
            self.artwork.embedded = Some(EmbeddedArtwork {
                file: file.to_path_buf(),
                extension: extension.to_owned(),
                picture,
            });
            return;
        }
        let bare = self
            .song
            .as_ref()
            .is_some_and(|song| song.document().document().metadata.artwork.is_none());
        if bare && self.artwork.audio_file.as_deref() == Some(file) {
            self.take_artwork(format!("cover.{extension}"), picture);
        }
    }
    /// Restores artwork bytes kept in an autosave.
    pub fn artwork_restored(&mut self, picture: Option<Picture>) {
        let named = self.song.as_ref().and_then(|song| {
            let artwork = song.document().document().metadata.artwork.as_ref();
            artwork.map(ToString::to_string)
        });
        if let (Some(name), Some(picture)) = (named, picture) {
            self.artwork.current = Some(Artwork::taken(name, self.artwork_file(), picture));
        }
    }
    /// Whether saving may overwrite `file` because it is the song's current artwork.
    pub fn may_replace(&self, file: &Path) -> bool {
        self.artwork.on_disk.as_deref() == Some(file)
    }
    /// Whether the song JSON or chosen-but-unsaved artwork has changed.
    pub(super) fn modified(&self) -> bool {
        self.song.as_ref().is_some_and(Song::is_modified)
            || self
                .artwork
                .current
                .as_ref()
                .and_then(Artwork::unsaved)
                .is_some()
    }
    /// Accepts the host's decoded picture for the artwork file currently named by the song.
    pub fn artwork_read(&mut self, file: &Path, picture: Option<Picture>) {
        if self.artwork_file().as_deref() == Some(file)
            && let Some(artwork) = &mut self.artwork.current
        {
            artwork.read(picture);
        }
    }
    /// Artwork bytes to write beside the song when saving to `path`.
    pub fn artwork_write_for(&self, path: &Path) -> Option<(PathBuf, Vec<u8>)> {
        self.artwork
            .current
            .as_ref()?
            .write_for(path, self.artwork_file().as_deref())
    }
    /// Absolute path for the artwork named by the open song, beside its chart file.
    pub(super) fn artwork_file(&self) -> Option<PathBuf> {
        let name = self
            .song
            .as_ref()?
            .document()
            .document()
            .metadata
            .artwork
            .as_ref()?;
        Some(self.song_folder()?.join(name.as_str()))
    }
    /// Names `picture` in the song metadata and keeps it pending until saved.
    pub(super) fn take_artwork(&mut self, name: String, picture: Picture) {
        let Some(song) = &mut self.song else {
            self.failure = Some("Open or start a song before adding its artwork.".to_owned());
            return;
        };
        let path = match charter_core::ResourcePath::new(&name) {
            Ok(path) => path,
            Err(error) => {
                self.failure = Some(format!("{name} cannot be a song's artwork: {error}"));
                return;
            }
        };
        let old = &song.document().document().metadata;
        let metadata = charter_core::Metadata {
            artwork: Some(path),
            ..old.clone()
        };
        self.failure = song.set_metadata(metadata).err().as_ref().map(refused);
        if self.failure.is_none() {
            // The picture it replaces comes back with an undo.
            self.park_artwork();
            let mark = self
                .song
                .as_ref()
                .map_or_else(HistoryMark::default, Song::history_mark);
            let mut artwork = Artwork::taken(name, self.artwork_file(), picture);
            artwork.set_mark(mark);
            self.artwork.current = Some(artwork);
            // A picture under the same name leaves the revision as it was, so autosave again.
            self.autosave.autosaved = None;
        }
    }
    /// Keeps the preview in step with undo, redo and artwork metadata changes.
    pub(super) fn follow_artwork(&mut self) {
        let named = self.song.as_ref().and_then(|song| {
            let artwork = song.document().document().metadata.artwork.as_ref();
            artwork.map(ToString::to_string)
        });
        let Some(named) = named else {
            self.park_artwork();
            return;
        };
        let file = self.artwork_file();
        let mark = self
            .song
            .as_ref()
            .map_or_else(HistoryMark::default, Song::history_mark);
        let restored_position = self
            .artwork
            .parked
            .iter()
            .position(|parked| parked.name() == named && parked.mark() == mark);
        // Another picture of the same name may belong to the mark undo or redo reached.
        let current = self.artwork.current.as_ref().is_some_and(|artwork| {
            artwork.name() == named
                && artwork.file() == file.as_deref()
                && (artwork.mark() == mark || restored_position.is_none())
        });
        if current {
            return;
        }
        self.park_artwork();
        // A picture chosen but not saved, which an undo took away, comes back at its mark.
        let parked_after_park = self
            .artwork
            .parked
            .iter()
            .position(|parked| parked.name() == named && parked.mark() == mark);
        if let Some(at) = parked_after_park {
            let mut artwork = self.artwork.parked.swap_remove(at);
            artwork.set_file(file);
            self.artwork.current = Some(artwork);
            return;
        }
        self.artwork.current = Some(Artwork::named(&named, file.as_deref()));
        if let Some(file) = file {
            self.requests.push(Request::ReadArtwork { file });
        }
    }
    /// Sets aside unsaved artwork so undo and redo can bring it back.
    pub(super) fn park_artwork(&mut self) {
        let Some(artwork) = self.artwork.current.take() else {
            return;
        };
        if artwork.unsaved().is_some() {
            self.artwork
                .parked
                .retain(|parked| parked.mark() != artwork.mark());
            self.artwork.parked.push(artwork);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::window::test_support::*;

    #[test]
    fn chosen_artwork_previews_and_is_written_beside_the_song_on_save() {
        let mut harness = four_taps_at("/songs/Four/four.json");
        harness.get_by_label("Choose the song's artwork").click();
        harness.run();
        assert_eq!(request(&mut harness), Request::ChooseArtwork);
        harness
            .state_mut()
            .use_artwork(Path::new("/pictures/art.png"), Some(picture(b"png")));
        harness.run();
        assert_eq!(artwork_name(&harness).as_deref(), Some("art.png"));
        assert!(
            harness
                .query_by_label("Not a picture beside the song")
                .is_none()
        );
        let saving = Path::new("/songs/Four/four.json");
        assert_eq!(
            harness.state().artwork_write_for(saving),
            Some((PathBuf::from("/songs/Four/art.png"), b"png".to_vec()))
        );
        harness.state_mut().saved(Saved::To(saving.to_path_buf()));
        assert_eq!(harness.state().artwork_write_for(saving), None);
    }
    #[test]
    fn artwork_named_before_the_first_save_is_read_once_the_song_is_saved() {
        let mut harness = created();
        retype(&mut harness, "Artwork", "art.png");
        assert!(
            harness
                .query_by_label("Shown once the song is saved")
                .is_some()
        );
        harness.state_mut().take_requests();
        harness
            .state_mut()
            .saved(Saved::To(PathBuf::from("/songs/New/New.json")));
        harness.run();
        let requests = harness.state_mut().take_requests();
        let file = PathBuf::from("/songs/New/art.png");
        assert!(
            requests.contains(&Request::ReadArtwork { file }),
            "{requests:?}"
        );
    }
    #[test]
    fn a_file_that_is_not_a_picture_is_refused_as_artwork() {
        let mut harness = four_taps_at("/songs/four.json");
        harness
            .state_mut()
            .use_artwork(Path::new("/pictures/notes.txt"), None);
        harness.run();
        assert_eq!(artwork_name(&harness), None);
        assert!(
            harness
                .query_by_label_contains("is not a PNG or JPEG picture")
                .is_some()
        );
    }
    #[test]
    fn an_audio_file_chosen_as_artwork_gives_its_cover_saved_beside_the_song() {
        let mut harness = four_taps_at("/songs/Four/four.json");
        harness.state_mut().artwork_ripped(
            Path::new("/music/track.flac"),
            Some(("jpg", picture(b"jpeg"))),
        );
        harness.run();
        assert_eq!(artwork_name(&harness).as_deref(), Some("cover.jpg"));
        assert_eq!(
            harness
                .state()
                .artwork_write_for(Path::new("/songs/Four/four.json")),
            Some((PathBuf::from("/songs/Four/cover.jpg"), b"jpeg".to_vec()))
        );
    }
    #[test]
    fn an_audio_file_without_a_cover_is_refused_as_artwork() {
        let mut harness = four_taps_at("/songs/four.json");
        harness
            .state_mut()
            .artwork_ripped(Path::new("/music/track.flac"), None);
        harness.run();
        assert_eq!(artwork_name(&harness), None);
        assert!(
            harness
                .query_by_label_contains("has no cover in its tags")
                .is_some()
        );
    }
    #[test]
    fn new_audio_brings_its_embedded_cover_unless_the_song_has_artwork() {
        let mut harness = four_taps_at("/songs/four.json");
        let audio = Path::new("/music/song.flac");
        harness.state_mut().use_audio(audio);
        harness.run();
        harness
            .state_mut()
            .artwork_embedded(Path::new("/music/other.flac"), "jpg", picture(b"x"));
        assert_eq!(artwork_name(&harness), None);
        harness
            .state_mut()
            .artwork_embedded(audio, "jpg", picture(b"jpeg"));
        harness.run();
        assert_eq!(artwork_name(&harness).as_deref(), Some("cover.jpg"));
        harness
            .state_mut()
            .artwork_embedded(audio, "png", picture(b"png"));
        assert_eq!(artwork_name(&harness).as_deref(), Some("cover.jpg"));
    }
    #[test]
    #[expect(
        clippy::non_ascii_literal,
        reason = "the test locates the artwork Reading label with its ellipsis"
    )]
    fn a_song_naming_artwork_asks_for_it_beside_the_song_and_previews_it() {
        let mut harness = four_taps_at("/songs/four.json");
        retype(&mut harness, "Artwork", "art.png");
        let requests = harness.state_mut().take_requests();
        let file = PathBuf::from("/songs/art.png");
        assert!(
            requests.contains(&Request::ReadArtwork { file: file.clone() }),
            "{requests:?}"
        );
        assert!(harness.query_by_label("Reading…").is_some());
        harness.state_mut().artwork_read(&file, None);
        harness.run();
        assert!(
            harness
                .query_by_label("Not a picture beside the song")
                .is_some()
        );
    }
    #[test]
    fn the_song_panel_sets_the_genre_and_artwork_and_clears_them() {
        let mut harness = editable_demo();
        retype(&mut harness, "Genre", "Dance");
        retype(&mut harness, "Artwork", "art/artwork.png");
        assert_eq!(metadata(&harness).genre.as_deref(), Some("Dance"));
        assert_eq!(
            metadata(&harness).artwork.map(|path| path.to_string()),
            Some("art/artwork.png".to_owned())
        );
        retype(&mut harness, "Artwork", "../outside.png");
        assert!(
            harness
                .query_by_label_contains("is not a file beside the song")
                .is_some()
        );
        retype(&mut harness, "Genre", "");
        retype(&mut harness, "Artwork", "");
        assert_eq!(
            (metadata(&harness).genre, metadata(&harness).artwork),
            (None, None)
        );
    }
    #[test]
    fn dragging_a_box_selects_the_notes_whose_heads_it_covers() {
        let mut harness = editable_four_taps();
        let (from, to) = (spot(&harness, 1, 180, 0.0), spot(&harness, 2, 540, 0.0));
        drag(&mut harness, from, to, egui::Modifiers::NONE);
        assert!(harness.query_by_label("2 notes selected").is_some());
        let song = harness.state().song.as_ref().unwrap();
        let ticks: Vec<_> = song.selection().map(|note| note.tick).collect();
        assert_eq!(ticks, [Tick::new(240), Tick::new(480)]);
    }
    #[test]
    fn a_restore_offered_while_starting_a_song_can_still_be_answered() {
        let (_, bytes) = autosaved_four_taps();
        let mut harness = new_song_dialog();
        harness.state_mut().recover(&bytes);
        harness.run();
        harness.get_by_label("Restore").click();
        harness.run();
        assert_eq!(harness.state().title(), "*four.json - charter");
    }
    #[test]
    fn unsaved_changes_are_autosaved_half_a_minute_after_they_are_made() {
        let mut harness = editable_four_taps();
        at_time(&mut harness, 0.0);
        harness.key_press(egui::Key::Num4);
        at_time(&mut harness, 1.0);
        at_time(&mut harness, 20.0);
        assert_eq!(harness.state_mut().take_requests(), []);
        at_time(&mut harness, 31.5);
        let Request::Autosave { bytes } = request(&mut harness) else {
            panic!("expected an autosave");
        };
        let kept = recovery::read(&bytes, PLUGINS).unwrap();
        assert_eq!(kept.path.as_deref(), Some(Path::new("four.json")));
        assert!(holds_tap(&kept.song.document().to_json().unwrap(), 0, 3));
        at_time(&mut harness, 90.0);
        assert_eq!(harness.state_mut().take_requests(), []);
    }
    #[test]
    fn an_autosave_restored_remembers_the_audio_dropped_on_the_song() {
        let mut harness = created();
        drop_file(&mut harness, "/music/track.ogg");
        at_time(&mut harness, 100.0);
        at_time(&mut harness, 131.0);
        let Request::Autosave { bytes } = request(&mut harness) else {
            panic!("expected an autosave");
        };
        let mut restored = window(Window::new(GAMES));
        restored.state_mut().recover(&bytes);
        restored.run();
        restored.get_by_label("Restore").click();
        restored.run();
        assert_eq!(
            restored.state().audio_file(),
            Some(PathBuf::from("/music/track.ogg"))
        );
        let path = PathBuf::from("/songs/mine.json");
        assert_eq!(
            restored.state().audio_copy_for(&path),
            Some((
                PathBuf::from("/music/track.ogg"),
                PathBuf::from("/songs/track.ogg"),
            ))
        );
        restored.state_mut().saved(Saved::To(path));
        restored.run();
        let copies: Vec<_> = restored
            .state_mut()
            .take_requests()
            .into_iter()
            .filter(|request| matches!(request, Request::CopyAudio { .. }))
            .collect();
        assert_eq!(copies, []);
    }
    #[test]
    fn an_autosave_restored_brings_the_changes_back_unsaved_for_their_file() {
        let (_, bytes) = autosaved_four_taps();
        let mut harness = window(Window::new(PLUGINS));
        harness.state_mut().recover(&bytes);
        harness.run();
        assert!(restore_prompt(&harness));
        harness.get_by_label("Restore").click();
        harness.run();
        assert_eq!(harness.state().title(), "*four.json - charter");
        assert_eq!(note_at(&harness, 0, 3), Some(tap()));
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        let Request::Write { path, .. } = request(&mut harness) else {
            panic!("expected a write");
        };
        assert_eq!(path, Path::new("four.json"));
        harness.state_mut().saved(Saved::To(path));
        harness.run();
        assert_eq!(request(&mut harness), Request::DropAutosave);
    }
    #[test]
    fn an_autosave_keeps_artwork_not_yet_beside_the_song_and_restores_it() {
        let mut harness = created();
        harness
            .state_mut()
            .use_artwork(Path::new("/pictures/art.png"), Some(picture(b"png")));
        at_time(&mut harness, 100.0);
        at_time(&mut harness, 131.0);
        let requests = harness.state_mut().take_requests();
        let Some(Request::Autosave { bytes }) = requests
            .into_iter()
            .find(|request| matches!(request, Request::Autosave { .. }))
        else {
            panic!("expected an autosave");
        };
        let mut restored = window(Window::new(GAMES));
        restored.state_mut().recover(&bytes);
        restored.run();
        restored.get_by_label("Restore").click();
        restored.run();
        let restored_requests = restored.state_mut().take_requests();
        assert!(restored_requests.contains(&Request::DecodeArtwork {
            bytes: b"png".to_vec()
        }));
        restored.state_mut().artwork_restored(Some(picture(b"png")));
        assert!(
            !restored
                .state()
                .may_replace(Path::new("/songs/New/art.png"))
        );
        let path = Path::new("/songs/New/New.json");
        assert_eq!(
            restored.state().artwork_write_for(path),
            Some((PathBuf::from("/songs/New/art.png"), b"png".to_vec()))
        );
    }
    #[test]
    fn chosen_artwork_survives_being_undone_and_redone() {
        let mut harness = four_taps_at("/songs/Four/four.json");
        harness
            .state_mut()
            .use_artwork(Path::new("/pictures/art.png"), Some(picture(b"png")));
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(artwork_name(&harness), None);
        let redo = egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT);
        harness.key_press_modifiers(redo, egui::Key::Z);
        harness.run();
        assert_eq!(artwork_name(&harness).as_deref(), Some("art.png"));
        assert_eq!(
            harness
                .state()
                .artwork_write_for(Path::new("/songs/Four/four.json")),
            Some((PathBuf::from("/songs/Four/art.png"), b"png".to_vec()))
        );
    }
    #[test]
    fn a_picture_undone_stays_set_aside_when_a_new_edit_names_its_file() {
        let mut harness = four_taps_at("/songs/Four/four.json");
        harness
            .state_mut()
            .use_artwork(Path::new("/pictures/art.png"), Some(picture(b"png")));
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        let song = harness.state().song.as_ref().unwrap();
        let metadata = charter_core::Metadata {
            artwork: Some(charter_core::ResourcePath::new("art.png").unwrap()),
            ..song.document().document().metadata.clone()
        };
        harness
            .state_mut()
            .apply(Action::SetMetadata(Box::new(metadata)));
        harness.run();
        assert_eq!(
            harness
                .state()
                .artwork_write_for(Path::new("/songs/Four/four.json")),
            None
        );
    }
    #[test]
    fn undoing_a_second_chosen_picture_brings_the_first_back() {
        let mut harness = four_taps_at("/songs/Four/four.json");
        let state = harness.state_mut();
        state.use_artwork(Path::new("/pictures/a.png"), Some(picture(b"a")));
        harness.run();
        let updated_state = harness.state_mut();
        updated_state.use_artwork(Path::new("/pictures/b.png"), Some(picture(b"b")));
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(artwork_name(&harness).as_deref(), Some("a.png"));
        assert_eq!(
            harness
                .state()
                .artwork_write_for(Path::new("/songs/Four/four.json")),
            Some((PathBuf::from("/songs/Four/a.png"), b"a".to_vec()))
        );
    }
    #[test]
    fn undo_brings_back_each_picture_of_one_name_at_its_own_step() {
        let mut harness = four_taps_at("/songs/Four/four.json");
        for (file, bytes) in [
            ("/p/cover.png", b"A"),
            ("/p/other.png", b"B"),
            ("/p/cover.png", b"C"),
        ] {
            harness
                .state_mut()
                .use_artwork(Path::new(file), Some(picture(bytes)));
            harness.run();
        }
        let path = Path::new("/songs/Four/four.json");
        let written = |view: &Harness<'_, Window>| {
            view.state().artwork_write_for(path).map(|(_, bytes)| bytes)
        };
        let undo = |view: &mut Harness<'_, Window>| {
            view.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
            view.run();
        };
        undo(&mut harness);
        assert_eq!(written(&harness), Some(b"B".to_vec()));
        undo(&mut harness);
        assert_eq!(written(&harness), Some(b"A".to_vec()));
        let redo = egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT);
        harness.key_press_modifiers(redo, egui::Key::Z);
        harness.key_press_modifiers(redo, egui::Key::Z);
        harness.run();
        assert_eq!(written(&harness), Some(b"C".to_vec()));
    }
    #[test]
    fn a_save_in_the_frame_of_an_undo_writes_the_artwork_the_song_names() {
        let mut harness = four_taps_at("/songs/Four/four.json");
        for (file, bytes) in [("/p/a.png", b"A"), ("/p/b.png", b"B")] {
            harness
                .state_mut()
                .use_artwork(Path::new(file), Some(picture(bytes)));
            harness.run();
        }
        harness.state_mut().take_requests();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        assert_eq!(artwork_name(&harness).as_deref(), Some("a.png"));
        let path = Path::new("/songs/Four/four.json");
        assert_eq!(
            harness.state().artwork_write_for(path),
            Some((PathBuf::from("/songs/Four/a.png"), b"A".to_vec()))
        );
    }
    #[test]
    fn redo_brings_back_a_same_name_picture_chosen_after_another_edit() {
        let mut harness = four_taps_at("/songs/Four/four.json");
        harness
            .state_mut()
            .use_artwork(Path::new("/p/cover.png"), Some(picture(b"A")));
        harness.run();
        select_note(&mut harness, 3, 720);
        harness.key_press(egui::Key::Delete);
        harness.run();
        harness
            .state_mut()
            .use_artwork(Path::new("/q/cover.png"), Some(picture(b"B")));
        harness.run();
        let path = Path::new("/songs/Four/four.json");
        let written = |view: &Harness<'_, Window>| {
            view.state().artwork_write_for(path).map(|(_, bytes)| bytes)
        };
        for _ in 0..2 {
            harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
            harness.run();
        }
        let redo = egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT);
        harness.key_press_modifiers(redo, egui::Key::Z);
        harness.run();
        assert_eq!(written(&harness), Some(b"A".to_vec()));
        harness.key_press_modifiers(redo, egui::Key::Z);
        harness.run();
        assert_eq!(written(&harness), Some(b"B".to_vec()));
    }
    #[test]
    fn another_picture_under_the_same_name_is_autosaved_again() {
        let mut harness = created();
        let chosen = Path::new("/pictures/art.png");
        harness
            .state_mut()
            .use_artwork(chosen, Some(picture(b"one")));
        at_time(&mut harness, 100.0);
        at_time(&mut harness, 131.0);
        let autosaves = |view: &mut Harness<'_, Window>| {
            let requests = view.state_mut().take_requests();
            requests
                .iter()
                .filter(|request| matches!(request, Request::Autosave { .. }))
                .count()
        };
        assert_eq!(autosaves(&mut harness), 1);
        harness
            .state_mut()
            .use_artwork(chosen, Some(picture(b"two")));
        at_time(&mut harness, 140.0);
        at_time(&mut harness, 171.0);
        assert_eq!(autosaves(&mut harness), 1);
    }
    #[test]
    fn a_picture_chosen_under_the_artwork_s_own_name_still_counts_as_unsaved() {
        let mut song: serde_json::Value = serde_json::from_slice(&four_taps()).unwrap();
        song["metadata"]["artwork"] = serde_json::json!("cover.jpg");
        let mut app = Window::new(PLUGINS);
        app.open(
            Path::new("/songs/four.json"),
            &serde_json::to_vec(&song).unwrap(),
        );
        let mut harness = window(app);
        assert_eq!(harness.state().title(), "four.json - charter");
        harness
            .state_mut()
            .use_artwork(Path::new("/downloads/cover.jpg"), Some(picture(b"new")));
        harness.run();
        assert_eq!(harness.state().title(), "*four.json - charter");
        let file = Path::new("/songs/cover.jpg");
        assert!(harness.state().may_replace(file));
        assert!(
            !harness
                .state()
                .may_replace(Path::new("/elsewhere/cover.jpg"))
        );
    }
    #[test]
    fn an_autosave_discarded_is_removed() {
        let (_, bytes) = autosaved_four_taps();
        let mut harness = window(Window::new(PLUGINS));
        harness.state_mut().recover(&bytes);
        harness.run();
        harness.get_by_label("Discard").click();
        harness.run();
        assert!(!restore_prompt(&harness));
        assert_eq!(request(&mut harness), Request::DropAutosave);
        assert!(harness.state().song.is_none());
    }
    #[test]
    fn a_damaged_autosave_says_so_and_is_removed() {
        let mut harness = window(Window::new(PLUGINS));
        harness.state_mut().recover(b"{ not json");
        harness.run();
        assert!(
            harness
                .query_by_label_contains("could not be restored")
                .is_some()
        );
        assert_eq!(request(&mut harness), Request::DropAutosave);
    }
    #[test]
    fn a_new_song_takes_the_cover_its_audio_holds_and_writes_it_into_its_folder() {
        let mut harness = new_song_dialog();
        drop_file(&mut harness, "/music/track.flac");
        let (bytes, image) = (
            b"jpeg".to_vec(),
            egui::ColorImage::filled([2, 2], egui::Color32::RED),
        );
        harness
            .state_mut()
            .artwork_embedded(Path::new("/music/track.flac"), "jpg", (bytes, image));
        harness.get_by_label("Create").click();
        harness.run();
        let song = harness.state().song.as_ref().unwrap();
        let artwork = song.document().document().metadata.artwork.clone();
        assert_eq!(
            artwork.map(|path| path.to_string()).as_deref(),
            Some("cover.jpg")
        );
        assert_eq!(
            harness
                .state()
                .artwork_write_for(Path::new("/s/Track/Track.json")),
            Some((PathBuf::from("/s/Track/cover.jpg"), b"jpeg".to_vec()))
        );
    }
    #[test]
    fn a_failed_autosave_is_retried_after_the_delay_without_losing_the_edit() {
        let (mut harness, _) = autosaved_four_taps();
        harness.state_mut().autosave_failed("out of space");
        at_time(&mut harness, 32.0);
        assert!(
            harness
                .query_by_label_contains("Autosave failed: out of space")
                .is_some()
        );
        assert_eq!(harness.state_mut().take_requests(), []);
        at_time(&mut harness, 62.0);
        let Request::Autosave { bytes } = request(&mut harness) else {
            panic!("expected a retry");
        };
        let kept = recovery::read(&bytes, PLUGINS).unwrap();
        assert!(holds_tap(&kept.song.document().to_json().unwrap(), 0, 3));
    }
}
