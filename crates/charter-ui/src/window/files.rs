//! Opening, saving, autosaving, snapshots and recent files.

use super::{
    Arc, Artwork, Autosave, Discard, EmbeddedArtwork, Leaving, NO_AUDIO, OfFile, Offer, Path,
    PathBuf, Peaks, RECENT, Request, Restoring, Saved, Saving, Song, Spectrum, TagTarget,
    UNREADABLE_AUTOSAVE, Waiting, Window, Zoom, demo, file_name, new_song, notefield, panels,
    recovery, refused, shown_strip,
};

impl Window {
    /// Folder containing the open song's chart file.
    pub fn song_folder(&self) -> Option<PathBuf> {
        self.path.as_ref()?.parent().map(Path::to_path_buf)
    }
    /// Accepts the host's answer to a save request.
    pub(crate) fn saved(&mut self, saved: Saved) {
        match saved {
            Saved::To(path) => {
                if let Some(song) = &mut self.song {
                    song.mark_saved();
                }
                self.artwork.audio_file = None;
                self.remember(&path);
                self.path = Some(path);
                if let Some(artwork) = &mut self.artwork.current {
                    artwork.saved();
                }
                self.artwork.on_disk = self.artwork_file();
                self.failure = None;
                if let Some(Waiting::Saving(discard)) = self.dialogs.pending.take() {
                    self.carry_on(discard);
                }
            }
            Saved::Cancelled => self.dialogs.pending = None,
            Saved::Failed(why) => {
                self.dialogs.pending = None;
                self.failure = Some(format!("Could not save: {why}"));
            }
        }
    }
    /// Reads an autosave and offers to restore it.
    pub(crate) fn recover(&mut self, bytes: &[u8]) {
        self.autosave.kept = true;
        match recovery::read(bytes, self.plugins) {
            Ok(recovery::Kept {
                path,
                audio,
                song,
                artwork,
            }) => {
                self.dialogs.restoring = Some(Restoring {
                    path,
                    audio,
                    song: Box::new(song),
                    artwork,
                });
            }
            Err(error) => {
                self.failure = Some(format!(
                    "Unsaved changes from last time could not be restored: {error}. The file is \
                     kept as {UNREADABLE_AUTOSAVE} in the data folder Settings > Autosave names."
                ));
                // Set aside rather than removed, and no longer this run's to drop: the next
                // autosave would write over it, and nothing could restore it again.
                self.autosave.kept = false;
                self.autosave.edits.forget();
                self.requests.push(Request::SetAsideAutosave);
            }
        }
    }
    /// Audio file that should be used for the open song, if one is known.
    pub fn audio_file(&self) -> Option<PathBuf> {
        let song = self.song.as_ref()?;
        let audio = song.document().document().metadata.audio.as_str();
        if audio == NO_AUDIO {
            return None;
        }
        let dropped = self.artwork.audio_file.as_ref().filter(|file| {
            file.file_name()
                .is_some_and(|name| name.to_str() == Some(audio))
        });
        dropped.cloned().or_else(|| {
            let folder = self.path.as_ref()?.parent()?;
            Some(folder.join(audio))
        })
    }
    /// Uses a chosen or dropped audio file, either in the new-song dialog or the open song.
    pub(crate) fn use_audio(&mut self, file: &Path) {
        if self.dialogs.new_song.is_some() {
            self.draft_audio(file);
        } else {
            self.add_audio(file);
        }
    }
    /// Applies audio tags to untouched title and artist fields, or asks before replacing them.
    pub(crate) fn tags_read(&mut self, file: &Path, title: Option<String>, artist: Option<String>) {
        if title.is_none() && artist.is_none() {
            return;
        }
        let (named, by) = (title.as_deref(), artist.as_deref());
        if let Some(draft) = &mut self.dialogs.new_song {
            if draft.audio() != Some(file) || !draft.differs(named, by) {
                return;
            }
            if draft.blank() {
                draft.fill(named, by);
            } else {
                self.dialogs.offer = Some(Offer {
                    target: TagTarget::Draft,
                    title,
                    artist,
                });
            }
            return;
        }
        let Some(song) = &mut self.song else {
            return;
        };
        if self.artwork.audio_file.as_deref() != Some(file) {
            return;
        }
        let metadata = &song.document().document().metadata;
        let untouched =
            metadata.title == new_song::UNTITLED && metadata.artist == new_song::UNKNOWN_ARTIST;
        let differs = named.is_some_and(|named| named != metadata.title)
            || by.is_some_and(|by| by != metadata.artist);
        if !differs {
            return;
        }
        if untouched {
            self.take_tags(TagTarget::Song, named, by);
        } else {
            self.dialogs.offer = Some(Offer {
                target: TagTarget::Song,
                title,
                artist,
            });
        }
    }
    /// Applies accepted title and artist tags.
    pub(super) fn take_tags(
        &mut self,
        target: TagTarget,
        title: Option<&str>,
        artist: Option<&str>,
    ) {
        let song = match target {
            TagTarget::Draft => {
                if let Some(new_song_draft) = &mut self.dialogs.new_song {
                    new_song_draft.fill(title, artist);
                }
                return;
            }
            TagTarget::Song => self.song.as_mut(),
        };
        let Some(song) = song else {
            return;
        };
        let metadata = &song.document().document().metadata;
        let title = title.unwrap_or(&metadata.title).to_owned();
        let artist = artist.unwrap_or(&metadata.artist).to_owned();
        self.failure = song
            .set_song_facts(&title, &artist)
            .err()
            .as_ref()
            .map(refused);
    }
    /// Sets the new-song dialog's audio file and asks the host for its tags.
    pub(super) fn draft_audio(&mut self, file: &Path) {
        if let Some(draft) = &mut self.dialogs.new_song {
            draft.set_audio(Some(file.to_path_buf()));
            self.requests.push(Request::ReadTags {
                file: file.to_path_buf(),
            });
        }
    }
    /// Stores waveform peaks decoded for `file`.
    pub(crate) fn waveform_ready(&mut self, file: &Path, peaks: Arc<Peaks>) {
        self.analysis.waveform = Some(OfFile {
            file: file.to_path_buf(),
            value: peaks,
        });
    }
    /// Stores spectrum frames decoded for `file`.
    pub(crate) fn spectrum_ready(&mut self, file: &Path, spectrum: Arc<Spectrum>) {
        self.analysis.spectrum = Some(OfFile {
            file: file.to_path_buf(),
            value: notefield::Tiles::new(spectrum),
        });
    }
    /// Stores onset detections for `file`.
    pub(crate) fn onsets_ready(&mut self, file: &Path, onsets: Arc<[charter_audio::Onset]>) {
        self.analysis.onsets = Some(OfFile {
            file: file.to_path_buf(),
            value: onsets,
        });
    }
    /// Onsets for the currently selected audio file.
    pub fn onsets(&self) -> Option<&[charter_audio::Onset]> {
        let file = self.audio_file();
        let onsets = self.analysis.onsets.as_ref()?;
        (Some(onsets.file.as_path()) == file.as_deref()).then_some(onsets.value.as_ref())
    }
    /// Waveform or spectrum strip matching the current display settings.
    pub fn strip(&self) -> Option<notefield::Strip<'_>> {
        shown_strip(
            &self.settings,
            self.analysis.waveform.as_ref(),
            self.analysis.spectrum.as_ref(),
            self.audio_file().as_deref(),
        )
    }
    /// Last user-facing message, if any.
    pub fn message(&self) -> Option<&str> {
        self.failure.as_deref()
    }
    /// Shows why the host could not copy audio beside a saved song.
    pub(crate) fn audio_not_copied(&mut self, why: &str) {
        self.failure = Some(format!("Audio not copied: {why}"));
    }
    /// Source and destination audio paths needed when saving the song to `path`.
    pub fn audio_copy_for(&self, path: &Path) -> Option<(PathBuf, PathBuf)> {
        let song = self.song.as_ref()?;
        let from = self.audio_file()?;
        let to = path
            .parent()?
            .join(song.document().document().metadata.audio.as_str());
        (from != to).then_some((from, to))
    }
    /// Drops playback state that belongs to a no-longer-current audio file.
    pub(super) fn forget_stale_audio(&mut self) {
        let wanted = self.audio_file();
        if self
            .playback
            .audio
            .as_ref()
            .is_some_and(|status| Some(&status.file) != wanted.as_ref())
        {
            self.playback.audio = None;
            if self.playback.playing.take().is_some() {
                self.requests.push(Request::Pause);
            }
        }
    }
    /// Names `path` as the open song's audio.
    pub(super) fn add_audio(&mut self, path: &Path) {
        let Some(song) = &mut self.song else {
            self.failure = Some("Open or start a song before adding its audio.".to_owned());
            return;
        };
        let name = path.file_name().map(|name| name.to_string_lossy());
        let audio = match charter_core::ResourcePath::new(name.as_deref().unwrap_or_default()) {
            Ok(audio) => audio,
            Err(error) => {
                self.failure = Some(format!(
                    "{} cannot be a song's audio: {error}",
                    path.display()
                ));
                return;
            }
        };
        self.failure = song.set_audio(audio).err().as_ref().map(refused);
        if self.failure.is_none() {
            self.artwork.audio_file = Some(path.to_path_buf());
            self.requests.push(Request::ReadTags {
                file: path.to_path_buf(),
            });
        }
    }
    /// Creates a new song through its ruleset plugin, after unsaved-change checks.
    pub(super) fn create(
        &mut self,
        plugin: &dyn charter_core::RulesetPlugin,
        song: &charter_core::NewSong,
    ) {
        match plugin.new_song(song) {
            Ok(document) => {
                let audio = self
                    .dialogs
                    .new_song
                    .as_ref()
                    .and_then(|draft| draft.audio().map(std::path::Path::to_path_buf));
                let created_song = Box::new(Song::new(document, self.plugins));
                self.after_asking(Discard::Open {
                    path: None,
                    song: created_song,
                    restored: false,
                    audio,
                    note: None,
                });
            }
            Err(error) => {
                let why = format!("Could not start the song: {error}");
                if let Some(draft) = &mut self.dialogs.new_song {
                    draft.error = Some(why.clone());
                }
                self.failure = Some(why);
            }
        }
    }
    /// Shows why autosaving failed and schedules the next edit for another try.
    pub(crate) fn autosave_failed(&mut self, why: &str) {
        self.autosave.edits.retry();
        self.failure = Some(format!("Autosave failed: {why}"));
    }
    /// Removes the kept autosave, if one exists.
    pub(super) fn drop_autosave(&mut self) {
        self.autosave.edits.forget();
        if std::mem::take(&mut self.autosave.kept) {
            self.requests.push(Request::DropAutosave);
        }
    }
    /// Writes an autosave when the open song has stayed dirty long enough.
    pub(super) fn autosave(&mut self, ctx: &egui::Context) {
        if self.dialogs.leaving != Leaving::Staying || self.dialogs.restoring.is_some() {
            return;
        }
        let Some(song) = &self.song else {
            return;
        };
        if !song.is_modified()
            && self
                .artwork
                .current
                .as_ref()
                .and_then(Artwork::unsaved)
                .is_none()
        {
            self.drop_autosave();
            return;
        }
        let revision = song.revision();
        if !self
            .autosave
            .edits
            .ready(ctx, revision, self.settings.autosave_after())
        {
            return;
        }
        let audio = self.artwork.audio_file.as_deref();
        let artwork = self.artwork.current.as_ref().and_then(Artwork::unsaved);
        match recovery::write(
            self.path.as_deref(),
            audio,
            song.document().document(),
            artwork,
        ) {
            Ok(bytes) => {
                self.requests.push(Request::Autosave { bytes });
                self.autosave.kept = true;
                self.autosave.edits.wrote(revision);
            }
            Err(error) => self.failure = Some(format!("Autosave failed: {error}")),
        }
    }
    /// Restores or discards the autosave offered at startup.
    pub(super) fn answer_restore(&mut self, restore: panels::Restore) {
        let Some(Restoring {
            path,
            audio,
            mut song,
            artwork,
        }) = self.dialogs.restoring.take()
        else {
            return;
        };
        match restore {
            panels::Restore::Restore => {
                song.mark_unsaved();
                self.artwork.restored = artwork;
                self.after_asking(Discard::Open {
                    path,
                    song,
                    restored: true,
                    audio,
                    note: None,
                });
            }
            panels::Restore::Discard => self.drop_autosave(),
        }
    }
    /// Title the host should show for the window.
    pub fn title(&self) -> String {
        let Some(song) = &self.song else {
            return "charter".to_owned();
        };
        let star = if self.modified() { "*" } else { "" };
        format!("{star}{} - charter", self.file_name(song))
    }
    /// Display name for a song, from its file path or title.
    pub(super) fn file_name(&self, song: &Song) -> String {
        file_name(self.path.as_deref(), song)
    }
    /// Asks the host to save the open song in place or elsewhere.
    pub(super) fn save(&mut self, saving: Saving) {
        // An undo in this frame may have named other artwork since the frame began.
        self.follow_artwork();
        let Some(song) = &self.song else {
            return;
        };
        let bytes = match song.document().to_json() {
            Ok(bytes) => bytes,
            Err(error) => {
                self.failure = Some(format!("Could not save: {error}"));
                return;
            }
        };
        let request = match &self.path {
            Some(path) if saving == Saving::InPlace => Request::Write {
                path: path.clone(),
                bytes,
            },
            _ => Request::SaveAs {
                name: self.file_name(song),
                bytes,
            },
        };
        self.requests.push(request);
    }
    /// Opens a song document from bytes supplied by the host.
    pub(crate) fn open(&mut self, path: &Path, bytes: &[u8]) {
        match Song::open(bytes, self.plugins) {
            Ok(song) => self.after_asking(Discard::Open {
                path: Some(path.to_path_buf()),
                song: Box::new(song),
                restored: false,
                audio: None,
                note: None,
            }),
            Err(error) => self.refuse(path, &error),
        }
    }
    /// Shows why opening `path` failed and removes it from recent songs.
    pub(crate) fn refuse(&mut self, path: &Path, why: impl std::fmt::Display) {
        self.failure = Some(format!("{} did not open: {why}", path.display()));
        // A song that no longer opens leaves the recent list.
        self.settings.recent.retain(|recent| recent != path);
    }
    /// Uses `folder` as the global snapshot destination.
    pub(crate) fn use_snapshot_folder(&mut self, folder: &Path) {
        self.settings.snapshot_folder = Some(folder.to_path_buf());
        self.settings.snapshots_in_folder = true;
    }
    /// Shows why a snapshot write failed and allows another try.
    pub(crate) fn snapshot_failed(&mut self, why: &str) {
        self.autosave.snapshots.retry();
        self.failure = Some(format!("Snapshot failed: {why}"));
    }
    /// Returns this window with the host's data folder configured.
    #[must_use]
    pub fn with_data_folder(self, folder: PathBuf) -> Self {
        Self {
            autosave: Autosave {
                data_folder: Some(folder),
                ..self.autosave
            },
            ..self
        }
    }
    /// Moves `path` to the front of the recent-song list.
    pub(super) fn remember(&mut self, path: &Path) {
        let recent = &mut self.settings.recent;
        recent.retain(|recent| recent != path);
        recent.insert(0, path.to_path_buf());
        recent.truncate(RECENT);
    }
    /// Writes dated snapshot copies when snapshot settings say one is due.
    pub(super) fn snapshot(&mut self, ctx: &egui::Context) {
        let Some(song) = &self.song else {
            return;
        };
        let revision = song.revision();
        if !self
            .autosave
            .snapshots
            .ready(ctx, revision, self.settings.snapshot_after())
        {
            return;
        }
        let mut name = self.file_name(song);
        if let Some(bare) = name.strip_suffix(".json") {
            name.truncate(bare.len());
        }
        let mut folders = Vec::new();
        if self.settings.snapshots_in_folder {
            let global = self
                .settings
                .snapshot_folder
                .as_ref()
                .map(|folder| folder.join(&name))
                .or_else(|| {
                    self.autosave
                        .data_folder
                        .as_ref()
                        .map(|data| data.join("snapshots").join(&name))
                });
            folders.extend(global);
        }
        if self.settings.snapshots_beside_song {
            folders.extend(self.song_folder().map(|folder| folder.join("autosaves")));
        }
        if folders.is_empty() {
            return;
        }
        match song.document().to_json() {
            Ok(bytes) => {
                self.requests.push(Request::Snapshot {
                    name,
                    bytes,
                    folders,
                    kept: self.settings.snapshots_kept.into(),
                });
                self.autosave.snapshots.wrote(revision);
            }
            Err(error) => self.failure = Some(format!("Snapshot failed: {error}")),
        }
    }
    /// Opens the built-in demo song after unsaved-change checks.
    pub(crate) fn open_demo(&mut self) {
        let song = Box::new(Song::new(demo::song(), self.plugins));
        self.after_asking(Discard::Open {
            path: None,
            song,
            restored: false,
            audio: None,
            note: None,
        });
    }
    /// Asks before replacing unsaved work, otherwise continues immediately.
    pub(super) fn after_asking(&mut self, discard: Discard) {
        if self.modified() {
            self.dialogs.pending = Some(Waiting::Asking(discard));
        } else {
            self.carry_on(discard);
        }
    }
    /// Continues an open or close operation after changes are dealt with.
    pub(super) fn carry_on(&mut self, discard: Discard) {
        match discard {
            Discard::Close => {
                self.drop_autosave();
                self.dialogs.leaving = Leaving::Now;
            }
            Discard::Open {
                path,
                mut song,
                restored,
                audio,
                note,
            } => {
                if restored {
                    self.autosave.edits.wrote(song.revision());
                } else {
                    self.drop_autosave();
                }
                self.saving = None;
                song.set_snap(self.settings.snap);
                self.dialogs.new_song = None;
                self.song = Some(*song);
                self.import.invalidate();
                if let Some(path) = &path {
                    self.remember(path);
                }
                self.path = path;
                // A song opened is not copied until it changes.
                match self.song.as_ref().map(Song::revision) {
                    Some(revision) => self.autosave.snapshots.wrote(revision),
                    None => self.autosave.snapshots.forget(),
                }
                self.zoom = Zoom::default().by(self.settings.zoom.percent() / 100.0);
                self.failure = note;
                self.typing.placed = false;
                self.typing.holding.clear();
                self.typing.pressed.clear();
                self.artwork.audio_file = audio;
                self.artwork.current = None;
                self.artwork.parked.clear();
                if self.playback.playing.take().is_some() {
                    self.requests.push(Request::Pause);
                }
                // A restored song names what its unsaved changes chose, not what is on disk, so
                // no file there counts as its own.
                self.artwork.on_disk = if restored { None } else { self.artwork_file() };
                if restored && let Some(bytes) = self.artwork.restored.take() {
                    self.requests.push(Request::DecodeArtwork { bytes });
                }
                let embedded = self.artwork.embedded.take();
                if let Some(EmbeddedArtwork {
                    file,
                    extension,
                    picture,
                }) = embedded
                    && self.artwork.audio_file.as_ref() == Some(&file)
                {
                    self.artwork_embedded(&file, &extension, picture);
                }
            }
        }
    }
    /// Applies the answer to the unsaved-changes question.
    pub(super) fn answer_save_changes(&mut self, answer: panels::SaveChanges) {
        let Some(Waiting::Asking(discard)) = self.dialogs.pending.take() else {
            return;
        };
        match answer {
            panels::SaveChanges::Save => {
                self.dialogs.pending = Some(Waiting::Saving(discard));
                self.saving = Some(Saving::InPlace);
            }
            panels::SaveChanges::DontSave => self.carry_on(discard),
            panels::SaveChanges::Cancel => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::window::test_support::*;

    #[test]
    fn panel_layer_edits_each_undo_as_one_step() {
        let edits = [
            Action::RenameLayer(LayerIndex::new(0), "Kick".into()),
            Action::RecolourLayer(LayerIndex::new(0), charter_core::LayerColour::Pink),
            Action::SetLayerView(LayerIndex::new(0), charter_core::LayerView::Ghosted),
            Action::SetLayerLocked(LayerIndex::new(0), true),
            Action::SoloLayer(LayerIndex::new(0)),
            Action::MoveLayer {
                from: LayerIndex::new(0),
                to: LayerIndex::new(1),
            },
            Action::MergeLayer(LayerIndex::new(0)),
            Action::DeleteLayer(LayerIndex::new(0), charter_editor::LayerNotes::Delete),
        ];
        for action in edits {
            let mut app = Window::new(PLUGINS);
            app.open_demo();
            app.apply(Action::AddLayer);
            let before = app.song.as_ref().unwrap().document().to_json().unwrap();
            app.apply(action.clone());
            assert!(app.failure.is_none(), "{action:?}: {:?}", app.failure);
            app.apply(Action::Undo);
            assert_eq!(
                app.song.as_ref().unwrap().document().to_json().unwrap(),
                before,
                "{action:?}"
            );
        }
    }
    #[test]
    fn a_song_its_plugin_accepts_opens_for_editing() {
        let harness = editable_demo();
        assert!(harness.query_by_label("Editing").is_some());
        assert!(harness.query_by_label_contains("Read only").is_none());
        let song = harness.state().song.as_ref().unwrap();
        assert_eq!(song.layers()[0].name, "Layer 0");
        assert!(!song.is_modified());
        assert!(!song.can_undo());
    }
    #[test]
    fn a_song_without_its_plugin_is_read_only_and_names_the_plugin() {
        let mut app = Window::new(PLUGINS);
        app.open(Path::new("example.json"), example());
        let harness = window(app);
        assert!(
            harness
                .query_by_label("Read only: charter has no arcaea plugin")
                .is_some()
        );
        assert!(harness.query_by_label("Read only").is_some());
    }
    #[test]
    fn opening_a_song_with_a_lane_key_down_leaves_its_cursor_alone() {
        let mut harness = editable_demo();
        harness.key_down(egui::Key::Num1);
        harness.run();
        harness.state_mut().open_demo();
        harness.run();
        harness.get_by_label("Don't save").click();
        harness.key_up(egui::Key::Num1);
        harness.run();
        assert!(cursor_shows(&harness, "0:0:0"));
    }
    #[test]
    fn keys_wait_while_the_window_asks_about_unsaved_changes() {
        let mut harness = edited_four_taps();
        harness
            .state_mut()
            .open(Path::new("example.json"), example());
        harness.run();
        harness.key_press(egui::Key::Num3);
        harness.run();
        assert_eq!(note_count(&harness), 5);
    }
    #[test]
    fn brackets_lengthen_the_note_at_the_cursor_to_a_hold_and_shorten_it_back() {
        let mut harness = editable_four_taps();
        let first = |view: &Harness<'_, Window>| {
            let song = view.state().song.as_ref().unwrap();
            song.chart().unwrap().objects.first().unwrap().kind.clone()
        };
        harness.key_press(egui::Key::CloseBracket);
        harness.run();
        assert_eq!(first(&harness), charter_core::Kind::Hold);
        harness.key_press(egui::Key::OpenBracket);
        harness.run();
        assert_eq!(first(&harness), charter_core::Kind::Tap);
    }
    #[test]
    fn clicking_the_minimap_moves_the_cursor_there_and_the_setting_hides_it() {
        let mut harness = editable_four_taps();
        let map = harness.get_by_label("Minimap").rect();
        // The last tap starts at tick 720 of a map running to its end.
        click(
            &mut harness,
            egui::pos2(map.center().x, map.bottom() - 1.0),
            egui::Modifiers::NONE,
        );
        let cursor = harness.state().song.as_ref().unwrap().cursor();
        assert!(cursor > Tick::new(0), "{cursor:?}");
        let mut app = Window::new(PLUGINS).with_settings(Settings {
            minimap: false,
            ..Settings::default()
        });
        app.open(Path::new("four.json"), &four_taps());
        let open_harness = window(app);
        assert!(open_harness.query_by_label("Minimap").is_none());
    }
    #[test]
    fn the_song_panel_renames_the_song_and_sets_its_preview() {
        let mut harness = editable_demo();
        retype(&mut harness, "Title", "Mine");
        retype(&mut harness, "Artist", "Me");
        assert_eq!(
            (metadata(&harness).title, metadata(&harness).artist),
            ("Mine".to_owned(), "Me".to_owned())
        );
        retype(&mut harness, "Preview", "0:45.5");
        retype(&mut harness, "Preview length", "15");
        let preview = metadata(&harness).preview.unwrap();
        assert_eq!(
            serde_json::to_value(preview).unwrap(),
            serde_json::json!({ "start_seconds": 45.5, "length_seconds": 15.0 })
        );
        retype(&mut harness, "Preview", "");
        assert_eq!(metadata(&harness).preview, None);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert!(metadata(&harness).preview.is_some());
        harness.get_by_label("Choose the song's audio").click();
        harness.run();
        assert_eq!(request(&mut harness), Request::ChooseAudio);
    }
    #[test]
    fn a_games_settings_show_as_fields_for_the_song_and_its_open_chart() {
        const TTR: &[&dyn RulesetPlugin] = &[&charter_ttr::Plugin];
        let draft = charter_core::NewSong {
            metadata: charter_editor::demo::song().document().metadata.clone(),
            layout: 0,
            charts: vec![charter_core::NewChart::named("Extreme")],
        };
        let song = charter_ttr::Plugin.new_song(&draft).unwrap();
        let mut app = Window::new(TTR);
        app.open(Path::new("/songs/ttr.json"), &song.to_json().unwrap());
        let mut harness = window(app);
        retype(&mut harness, "Theme", "DeltaHeavyTheme");
        retype(&mut harness, "Chart theme", "Neon");
        let edited_song = harness.state().song.as_ref().unwrap();
        let document = edited_song.document().document();
        assert_eq!(
            document.settings["theme"],
            serde_json::json!("DeltaHeavyTheme")
        );
        assert_eq!(
            document.charts[0].settings["theme"],
            serde_json::json!("Neon")
        );
        retype(&mut harness, "Theme", "");
        let cleared_song = harness.state().song.as_ref().unwrap();
        assert!(cleared_song.document().document().settings.is_empty());
        let sm = editable_demo();
        assert!(sm.query_by_label("Chart theme").is_none());
    }
    #[test]
    fn a_right_click_opens_the_menu_and_leaves_the_cursor_where_it_was() {
        let mut harness = editable_four_taps();
        let pos = on_line(&harness, lane_x(&harness, 2), 480);
        harness.hover_at(pos);
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
        harness.run();
        assert_eq!(
            harness.state().song.as_ref().unwrap().cursor(),
            Tick::new(0)
        );
        assert!(harness.query_by_label("1 note selected").is_some());
        assert!(
            harness
                .query_by_label_contains("Insert pattern here")
                .is_some()
        );
        assert!(harness.query_by_label("Select lane 3").is_some());
        harness
            .get_by_label_contains("Mirror across the lanes")
            .click();
        harness.run();
        assert_eq!(lanes_at(&harness, 480), [1]);
    }
    #[test]
    fn a_pattern_places_its_steps_from_the_lane_and_moves_the_cursor_past_it() {
        let mut app = Window::new(PLUGINS);
        app.open(Path::new("four.json"), &four_taps());
        let song = app.song.as_mut().unwrap();
        song.jump_to(Tick::new(1920));
        let steps: &'static [u16] = &[0, 1, 2, 0, 1, 2];
        assert_eq!(pattern(song, steps, 4, LaneIndex::new(2)), None);
        let lanes: Vec<(i64, u16)> = song
            .chart()
            .unwrap()
            .objects
            .iter()
            .filter(|object| object.tick >= Tick::new(1920))
            .filter_map(|object| match object.position {
                charter_core::Position::Lane { index, .. } => {
                    Some((i64::from(object.tick), u16::from(index)))
                }
                charter_core::Position::Plane { .. } => None,
            })
            .collect();
        assert_eq!(
            lanes,
            [
                (1920, 2),
                (2160, 3),
                (2400, 0),
                (2640, 2),
                (2880, 3),
                (3120, 0)
            ]
        );
        assert_eq!(song.cursor(), Tick::new(3360));
    }
    #[test]
    fn a_pattern_wraps_each_step_around_a_two_lane_chart() {
        let mut document: serde_json::Value = serde_json::from_slice(&four_taps()).unwrap();
        document["charts"][0]["lanes"] = serde_json::json!(2);
        document["charts"][0]["objects"] = serde_json::json!([]);
        let mut app = Window::new(PLUGINS);
        app.open(
            Path::new("two.json"),
            &serde_json::to_vec(&document).unwrap(),
        );
        let song = app.song.as_mut().unwrap();
        assert!(song.read_only().is_none());
        song.jump_to(Tick::new(1920));
        assert_eq!(
            pattern(song, &[0, 1, 2, 0, 1, 2], 4, LaneIndex::new(1)),
            None
        );
        let placed: Vec<_> = song
            .chart()
            .unwrap()
            .objects
            .iter()
            .map(|object| {
                let Position::Lane { index, .. } = object.position else {
                    panic!("the pattern places lane notes");
                };
                (object.tick, u16::from(index))
            })
            .collect();
        assert_eq!(
            placed,
            [
                (Tick::new(1920), 1),
                (Tick::new(2160), 0),
                (Tick::new(2400), 1),
                (Tick::new(2640), 1),
                (Tick::new(2880), 0),
                (Tick::new(3120), 1),
            ]
        );
        assert_eq!(song.cursor(), Tick::new(3360));
    }
    #[test]
    fn the_mouse_places_nothing_in_a_read_only_song() {
        let mut app = Window::default();
        app.open(Path::new("four.json"), &four_taps());
        let mut harness = window(app);
        let pos = spot(&harness, 3, 240, 0.0);
        click(&mut harness, pos, egui::Modifiers::COMMAND);
        assert_eq!(note_at(&harness, 240, 3), None);
        assert!(harness.query_by_label_contains("Refused").is_none());
    }
    #[test]
    fn ctrl_s_writes_the_song_back_to_its_file() {
        let mut harness = editable_four_taps();
        harness.key_press(egui::Key::Num4);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        let Request::Write { path, bytes } = request(&mut harness) else {
            panic!("expected a write");
        };
        assert_eq!(path, Path::new("four.json"));
        assert!(holds_tap(&bytes, 0, 3));
    }
    #[test]
    fn saving_a_song_without_a_file_asks_where() {
        let mut harness = editable_demo();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        let Request::SaveAs { name, .. } = request(&mut harness) else {
            panic!("expected save as");
        };
        assert_eq!(name, "Demo.json");
    }
    #[test]
    fn ctrl_shift_s_asks_where_even_for_a_song_with_a_file() {
        let mut harness = editable_four_taps();
        let command_shift = egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT);
        harness.key_press_modifiers(command_shift, egui::Key::S);
        harness.run();
        let Request::SaveAs { name, .. } = request(&mut harness) else {
            panic!("expected save as");
        };
        assert_eq!(name, "four.json");
    }
    #[test]
    #[expect(
        clippy::non_ascii_literal,
        reason = "the test locates the Open menu label with its ellipsis"
    )]
    fn ctrl_o_and_the_file_menu_ask_for_a_song_to_open() {
        let mut harness = editable_four_taps();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::O);
        harness.run();
        assert_eq!(request(&mut harness), Request::Open);
        harness.get_by_label("File").click();
        harness.run();
        harness.get_by_label_contains("Open…").click();
        harness.run();
        assert_eq!(request(&mut harness), Request::Open);
    }
    #[test]
    fn the_title_names_the_song_and_marks_unsaved_changes() {
        let mut harness = editable_four_taps();
        assert_eq!(harness.state().title(), "four.json - charter");
        harness.key_press(egui::Key::Num4);
        harness.run();
        assert_eq!(harness.state().title(), "*four.json - charter");
        harness
            .state_mut()
            .saved(Saved::To(PathBuf::from("four.json")));
        harness.run();
        assert_eq!(harness.state().title(), "four.json - charter");
    }
    #[test]
    fn a_song_saved_elsewhere_is_saved_there_from_then_on() {
        let mut harness = editable_demo();
        harness
            .state_mut()
            .saved(Saved::To(PathBuf::from("mine.json")));
        harness.run();
        assert_eq!(harness.state().title(), "mine.json - charter");
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        let Request::Write { path, .. } = request(&mut harness) else {
            panic!("expected a write");
        };
        assert_eq!(path, Path::new("mine.json"));
    }
    #[test]
    fn a_failed_save_says_why_and_keeps_the_changes_unsaved() {
        let mut harness = editable_four_taps();
        harness.key_press(egui::Key::Num4);
        harness.run();
        harness
            .state_mut()
            .saved(Saved::Failed("the disk is full".to_owned()));
        harness.run();
        assert!(
            harness
                .query_by_label_contains("the disk is full")
                .is_some()
        );
        assert_eq!(harness.state().title(), "*four.json - charter");
    }
    #[test]
    fn closing_with_unsaved_changes_asks_first() {
        let mut harness = edited_four_taps();
        close(&mut harness);
        assert!(sent(&harness, &egui::ViewportCommand::CancelClose));
        harness.run();
        assert!(asks_to_save(&harness));
    }
    #[test]
    fn closing_without_changes_does_not_ask() {
        let mut harness = editable_four_taps();
        close(&mut harness);
        assert!(!sent(&harness, &egui::ViewportCommand::CancelClose));
        harness.run();
        assert!(!asks_to_save(&harness));
    }
    #[test]
    fn dont_save_closes_and_lets_the_close_through() {
        let mut harness = edited_four_taps();
        close(&mut harness);
        harness.run();
        harness.get_by_label("Don't save").click();
        harness.step();
        harness.step();
        assert!(sent(&harness, &egui::ViewportCommand::Close));
        close(&mut harness);
        assert!(!sent(&harness, &egui::ViewportCommand::CancelClose));
    }
    #[test]
    fn save_writes_and_closes_once_the_host_says_it_saved() {
        let mut harness = edited_four_taps();
        close(&mut harness);
        harness.run();
        harness.get_by_label("Save").click();
        harness.run();
        let Request::Write { path, .. } = request(&mut harness) else {
            panic!("expected a write");
        };
        harness.state_mut().saved(Saved::To(path));
        harness.step();
        assert!(sent(&harness, &egui::ViewportCommand::Close));
    }
    #[test]
    fn a_failed_save_while_closing_keeps_the_dirty_song_and_asks_again() {
        let mut harness = edited_four_taps();
        close(&mut harness);
        harness.run();
        harness.get_by_label("Save").click();
        harness.run();
        assert!(matches!(request(&mut harness), Request::Write { .. }));

        harness
            .state_mut()
            .saved(Saved::Failed("the disk is full".to_owned()));
        harness.run();
        close(&mut harness);
        assert!(sent(&harness, &egui::ViewportCommand::CancelClose));
        harness.run();

        assert!(asks_to_save(&harness));
        assert_eq!(harness.state().title(), "*four.json - charter");
    }

    #[test]
    fn cancel_keeps_the_window_and_its_changes() {
        let mut harness = edited_four_taps();
        close(&mut harness);
        harness.run();
        harness.get_by_label("Cancel").click();
        harness.run();
        assert!(!asks_to_save(&harness));
        assert!(!sent(&harness, &egui::ViewportCommand::Close));
        assert_eq!(harness.state().title(), "*four.json - charter");
    }
    #[test]
    fn opening_another_song_over_unsaved_changes_asks_first() {
        let mut harness = edited_four_taps();
        harness
            .state_mut()
            .open(Path::new("example.json"), example());
        harness.run();
        assert!(asks_to_save(&harness));
        assert_eq!(harness.state().title(), "*four.json - charter");
        harness.get_by_label("Don't save").click();
        harness.run();
        assert_eq!(harness.state().title(), "example.json - charter");
    }
    #[test]
    fn recovering_over_unsaved_changes_asks_before_replacing_them() {
        let (_, bytes) = autosaved_four_taps();
        let mut harness = editable_four_taps();
        harness.key_press(egui::Key::Num2);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [0, 1]);

        harness.state_mut().recover(&bytes);
        harness.run();
        harness.get_by_label("Restore").click();
        harness.run();

        assert!(asks_to_save(&harness));
        assert_eq!(lanes_at(&harness, 0), [0, 1]);
        harness.get_by_label("Don't save").click();
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [0, 3]);
    }

    #[test]
    fn discarding_recovered_changes_keeps_the_open_song() {
        let (_, bytes) = autosaved_four_taps();
        let mut harness = editable_four_taps();
        harness.key_press(egui::Key::Num2);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [0, 1]);

        harness.state_mut().recover(&bytes);
        harness.run();
        harness.get_by_label("Discard").click();
        harness.run();

        assert_eq!(lanes_at(&harness, 0), [0, 1]);
        assert_eq!(harness.state_mut().take_requests(), [Request::DropAutosave]);
    }

    #[test]
    fn audio_tags_change_only_the_target_they_were_taken_for() {
        let mut app = Window::new(PLUGINS);
        app.open_demo();
        let original = app
            .song
            .as_ref()
            .unwrap()
            .document()
            .document()
            .metadata
            .title
            .clone();
        app.dialogs.new_song = Some(new_song::Draft::default());

        app.take_tags(TagTarget::Draft, Some("Draft title"), None);
        assert!(
            !app.dialogs
                .new_song
                .as_ref()
                .unwrap()
                .differs(Some("Draft title"), None)
        );
        assert_eq!(
            app.song
                .as_ref()
                .unwrap()
                .document()
                .document()
                .metadata
                .title,
            original
        );

        app.take_tags(TagTarget::Song, Some("Song title"), Some("Singer"));
        let metadata = &app.song.as_ref().unwrap().document().document().metadata;
        assert_eq!(metadata.title, "Song title");
        assert_eq!(metadata.artist, "Singer");
    }

    #[test]
    fn a_save_that_fails_leaves_everything_as_it_was() {
        let mut harness = edited_four_taps();
        harness.get_by_label("Help").click();
        harness.run();
        harness.get_by_label("Open the demo song").click();
        harness.run();
        harness.get_by_label("Save").click();
        harness.run();
        assert!(matches!(request(&mut harness), Request::Write { .. }));
        harness
            .state_mut()
            .saved(Saved::Failed("the disk is full".to_owned()));
        harness.run();
        assert_eq!(harness.state().title(), "*four.json - charter");
        assert!(!asks_to_save(&harness));
    }
    #[test]
    #[expect(
        clippy::non_ascii_literal,
        reason = "the test locates the Settings menu label with its ellipsis"
    )]
    fn the_settings_window_opens_from_the_edit_menu_and_with_ctrl_comma() {
        let mut harness = editable_demo();
        harness.get_by_label("Edit").click();
        harness.run();
        harness.get_by_label_contains("Settings…").click();
        harness.run();
        assert!(harness.query_all_by_label("Settings").next().is_some());
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
        harness.run();
        assert!(harness.query_all_by_label("Settings").next().is_none());
    }
    #[test]
    fn lane_keys_and_commands_still_work_with_the_settings_open() {
        let mut harness = editable_demo();
        let before = harness
            .state()
            .song
            .as_ref()
            .unwrap()
            .chart()
            .unwrap()
            .objects
            .clone();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
        harness.run();
        harness.key_press(egui::Key::Num1);
        harness.run();
        let after = &harness
            .state()
            .song
            .as_ref()
            .unwrap()
            .chart()
            .unwrap()
            .objects;
        assert_ne!(after, &before);
        harness.key_press(egui::Key::ArrowDown);
        harness.run();
        assert!(harness.state().song.as_ref().unwrap().cursor() > Tick::new(0));
    }
    #[test]
    fn a_song_from_a_file_wants_the_audio_beside_it() {
        let harness = four_taps_at("/songs/four.json");
        assert_eq!(
            harness.state().audio_file(),
            Some(PathBuf::from("/songs/a.ogg"))
        );
    }
    #[test]
    fn a_new_song_wants_no_audio_until_some_is_dropped_on_it() {
        let mut harness = created();
        assert_eq!(harness.state().audio_file(), None);
        drop_file(&mut harness, "/music/track.ogg");
        assert_eq!(
            harness.state().audio_file(),
            Some(PathBuf::from("/music/track.ogg"))
        );
    }

    #[test]
    fn aac_and_mp4_audio_can_be_dropped_on_a_new_song() {
        for path in ["/music/track.aac", "/music/track.m4a", "/music/track.mp4"] {
            let mut harness = created();
            drop_file(&mut harness, path);
            assert_eq!(harness.state().audio_file(), Some(PathBuf::from(path)));
        }
    }

    #[test]
    fn audio_whose_name_cannot_be_a_song_path_is_refused() {
        let mut harness = created();
        drop_file(&mut harness, "/music/a\\b.ogg");
        assert_eq!(audio(&harness), NO_AUDIO);
        assert!(
            harness
                .query_by_label_contains("cannot be a song's audio")
                .is_some()
        );
    }
    #[test]
    fn audio_dropped_then_undone_is_not_copied_on_save() {
        let mut harness = created();
        drop_file(&mut harness, "/music/track.ogg");
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        harness
            .state_mut()
            .saved(Saved::To(PathBuf::from("/songs/mine.json")));
        harness.run();
        assert_eq!(harness.state_mut().take_requests(), []);
    }
    #[test]
    #[expect(
        clippy::non_ascii_literal,
        reason = "the test matches both Choose audio buttons with their ellipses"
    )]
    fn where_files_cannot_be_dropped_the_window_offers_choosing_them_and_says_why() {
        let mut harness = window(Window::new(GAMES).without_file_drops());
        assert!(
            harness
                .query_by_label_contains("drop a charter song")
                .is_none()
        );
        harness.get_by_label("New song").click();
        harness.run();
        assert!(harness.query_by_label_contains("Drop an audio").is_none());
        assert!(
            harness
                .query_by_label_contains("doesn't work under Wayland yet")
                .is_some()
        );
        harness.get_by_label("Create").click();
        harness.run();
        assert!(harness.query_by_label_contains("Drop an audio").is_none());
        harness.get_by_label("Choose audio…").click();
        harness.run();
        assert_eq!(harness.state_mut().take_requests(), [Request::ChooseAudio]);
        harness.get_by_label("File").click();
        harness.run();
        // The banner's button, and now the File menu's.
        assert_eq!(harness.query_all_by_label("Choose audio…").count(), 2);
    }
    #[test]
    fn the_new_song_dialog_says_where_the_audio_comes_from() {
        let harness = new_song_dialog();
        assert!(
            harness
                .query_by_label_contains("Drop an audio file here")
                .is_some()
        );
    }
    #[test]
    fn the_waveform_of_the_songs_audio_shows_and_that_of_other_audio_does_not() {
        let mut harness = four_taps_at("/songs/four.json");
        assert!(harness.state().strip().is_none());
        harness
            .state_mut()
            .waveform_ready(Path::new("/songs/other.ogg"), peaks());
        harness.run();
        assert!(harness.state().strip().is_none());
        harness
            .state_mut()
            .waveform_ready(Path::new("/songs/a.ogg"), peaks());
        harness.run();
        assert!(harness.state().strip().is_some());
        let lanes = field(&harness)
            .columns
            .span(LaneIndex::new(0), LaneCount::ONE);
        let tap = on_line(&harness, lanes.center(), 0);
        click(&mut harness, tap, egui::Modifiers::NONE);
        assert!(harness.query_by_label("1 note selected").is_some());
    }
    #[test]
    fn the_strip_shows_the_frequencies_once_ready_when_the_settings_ask() {
        let mut app = Window::new(PLUGINS).with_settings(Settings {
            frequencies: true,
            ..Settings::default()
        });
        app.open(Path::new("/songs/four.json"), &four_taps());
        let mut harness = window(app);
        harness
            .state_mut()
            .waveform_ready(Path::new("/songs/a.ogg"), peaks());
        harness.run();
        assert!(matches!(
            harness.state().strip(),
            Some(notefield::Strip::Waveform(_))
        ));
        let track = charter_audio::Track::from_frames(1000, vec![[0.25, -0.25]; 4000]);
        harness
            .state_mut()
            .spectrum_ready(Path::new("/songs/a.ogg"), Arc::new(Spectrum::of(&track)));
        harness.run();
        assert!(matches!(
            harness.state().strip(),
            Some(notefield::Strip::Frequencies(_))
        ));
    }
    #[test]
    fn the_waveform_can_be_turned_off_in_the_settings() {
        let mut app = Window::new(PLUGINS).with_settings(Settings {
            waveform: false,
            ..Settings::default()
        });
        app.open(Path::new("/songs/four.json"), &four_taps());
        let mut harness = window(app);
        harness
            .state_mut()
            .waveform_ready(Path::new("/songs/a.ogg"), peaks());
        harness.run();
        assert!(harness.state().strip().is_none());
    }
    #[test]
    fn a_song_opens_on_the_snap_and_zoom_the_settings_choose() {
        let settings = Settings {
            snap: NoteValue::new(32).unwrap(),
            zoom: 200.0.into(),
            ..Settings::default()
        };
        let mut app = Window::new(PLUGINS).with_settings(settings);
        app.open_demo();
        let harness = window(app);
        assert!(harness.query_by_label("Snap 32nd").is_some());
        assert!(harness.query_by_label("Zoom 200%").is_some());
    }
    #[test]
    fn a_save_holds_every_edit_made_in_its_frame() {
        let mut harness = editable_four_taps();
        let key = |key, press: Press, modifiers| egui::Event::Key {
            key,
            pressed: matches!(press, Press::Down),
            repeat: false,
            modifiers,
            physical_key: None,
        };
        let command = egui::Modifiers::COMMAND;
        let events = &mut harness.input_mut().events;
        events.push(key(egui::Key::S, Press::Down, command));
        events.push(key(egui::Key::S, Press::Up, command));
        events.push(key(egui::Key::Num4, Press::Down, egui::Modifiers::NONE));
        events.push(key(egui::Key::Num4, Press::Up, egui::Modifiers::NONE));
        harness.run();
        let Request::Write { path, bytes } = request(&mut harness) else {
            panic!("expected a write");
        };
        assert!(holds_tap(&bytes, 0, 3));
        harness.state_mut().saved(Saved::To(path));
        assert_eq!(harness.state().title(), "four.json - charter");
    }
    #[test]
    fn a_changed_song_is_copied_each_wait_into_both_folders_and_not_again_unchanged() {
        let mut app = Window::new(PLUGINS)
            .with_settings(Settings {
                snapshots_in_folder: true,
                snapshots_beside_song: true,
                snapshot_minutes: 2_u32.into(),
                ..Settings::default()
            })
            .with_data_folder(PathBuf::from("/data"));
        app.open(Path::new("/songs/Four/Four.json"), &four_taps());
        let mut harness = window(app);
        at_time(&mut harness, 0.0);
        at_time(&mut harness, 500.0);
        assert_eq!(
            snapshots(&mut harness),
            [],
            "an unchanged song is not copied"
        );
        harness.key_press(egui::Key::Num4);
        at_time(&mut harness, 501.0);
        at_time(&mut harness, 600.0);
        assert_eq!(snapshots(&mut harness), [], "two minutes have not passed");
        at_time(&mut harness, 622.0);
        let folders = vec![
            PathBuf::from("/data/snapshots/Four"),
            PathBuf::from("/songs/Four/autosaves"),
        ];
        assert_eq!(snapshots(&mut harness), [("Four".to_owned(), folders)]);
        at_time(&mut harness, 900.0);
        assert_eq!(snapshots(&mut harness), [], "nothing changed since");
    }
    #[test]
    fn a_chosen_snapshot_folder_is_used_and_an_unsaved_song_has_no_folder_beside_it() {
        let mut harness = created();
        harness
            .state_mut()
            .use_snapshot_folder(Path::new("/backups"));
        harness.state_mut().settings.snapshots_beside_song = true;
        at_time(&mut harness, 100.0);
        harness.key_press(egui::Key::Num1);
        at_time(&mut harness, 101.0);
        at_time(&mut harness, 500.0);
        let taken = snapshots(&mut harness);
        let [(name, folders)] = taken.as_slice() else {
            panic!("one snapshot, got {taken:?}");
        };
        assert_eq!(folders, &[PathBuf::from("/backups").join(name)]);
    }
    #[test]
    fn an_unsaved_songs_snapshots_stay_in_its_folder_whatever_its_title() {
        for title in ["../../escaped/x", "/etc/passwd", "..", "Re: Zero?"] {
            let mut harness = created();
            harness
                .state_mut()
                .use_snapshot_folder(Path::new("/backups"));
            at_time(&mut harness, 100.0);
            let song = harness.state().song.as_ref().unwrap();
            let mut metadata = song.document().document().metadata.clone();
            metadata.title = title.to_owned();
            harness
                .state_mut()
                .apply(Action::SetMetadata(Box::new(metadata)));
            at_time(&mut harness, 101.0);
            at_time(&mut harness, 500.0);
            let taken = snapshots(&mut harness);
            let [(name, folders)] = taken.as_slice() else {
                panic!("one snapshot of {title:?}, got {taken:?}");
            };
            assert_eq!(folders, &[PathBuf::from("/backups").join(name)]);
            let parts: Vec<_> = Path::new(name).components().collect();
            assert!(
                matches!(parts.as_slice(), [std::path::Component::Normal(_)]),
                "{title:?} named {name:?}"
            );
            assert!(
                !name.contains(['\\', ':', '*', '?', '"', '<', '>', '|']),
                "{title:?} named {name:?}"
            );
        }
    }
    #[test]
    fn an_unavailable_snapshot_folder_does_not_consume_the_songs_revision() {
        let mut harness = four_taps_with(Settings {
            snapshots_in_folder: true,
            snapshots_beside_song: false,
            snapshot_minutes: 1_u32.into(),
            ..Settings::default()
        });
        harness.key_press(egui::Key::Num4);
        at_time(&mut harness, 1.0);
        at_time(&mut harness, 70.0);
        assert!(snapshots(&mut harness).is_empty());

        harness.state_mut().autosave.data_folder = Some(PathBuf::from("/data"));
        at_time(&mut harness, 71.0);
        at_time(&mut harness, 133.0);
        assert_eq!(
            snapshots(&mut harness),
            [(
                "four".to_owned(),
                vec![PathBuf::from("/data/snapshots/four")]
            )]
        );
    }
    #[test]
    fn a_failed_snapshot_is_retried_after_the_wait_without_another_edit() {
        let mut harness = four_taps_with(Settings {
            snapshots_in_folder: false,
            snapshots_beside_song: true,
            snapshot_minutes: 1_u32.into(),
            ..Settings::default()
        });
        harness.key_press(egui::Key::Num4);
        at_time(&mut harness, 1.0);
        at_time(&mut harness, 70.0);
        assert_eq!(snapshots(&mut harness).len(), 1);

        harness.state_mut().snapshot_failed("disk full");
        at_time(&mut harness, 71.0);
        at_time(&mut harness, 133.0);
        assert_eq!(snapshots(&mut harness).len(), 1);
    }
    #[test]
    fn the_empty_window_lists_recent_songs_to_open() {
        let app = Window::new(PLUGINS).with_settings(Settings {
            recent: vec![PathBuf::from("/songs/Starlight/Starlight.json")],
            ..Settings::default()
        });
        let mut harness = window(app);
        harness.get_by_label("Starlight.json").click();
        harness.run();
        assert_eq!(
            harness.state_mut().take_requests(),
            [Request::OpenPath {
                path: PathBuf::from("/songs/Starlight/Starlight.json")
            }]
        );
    }
    #[test]
    fn songs_opened_and_saved_are_remembered_latest_first_and_open_again() {
        let mut harness = four_taps_at("/songs/a.json");
        harness
            .state_mut()
            .open(Path::new("/songs/b.json"), &four_taps());
        harness.run();
        harness
            .state_mut()
            .saved(Saved::To(PathBuf::from("/songs/c.json")));
        harness.run();
        let recent = |view: &Harness<'_, Window>| view.state().settings().recent.clone();
        let paths = ["/songs/c.json", "/songs/b.json", "/songs/a.json"].map(PathBuf::from);
        assert_eq!(recent(&harness), paths);
        harness.get_by_label("File").click();
        harness.run();
        harness.get_by_label_contains("Open recent").click();
        harness.run();
        harness.get_by_label_contains("b.json").click();
        harness.run();
        assert_eq!(
            harness.state_mut().take_requests(),
            [Request::OpenPath {
                path: PathBuf::from("/songs/b.json")
            }]
        );
        harness
            .state_mut()
            .refuse(Path::new("/songs/a.json"), "it is not there");
        assert_eq!(recent(&harness).len(), 2);
        harness.state_mut().apply(Action::ClearRecent);
        assert_eq!(recent(&harness), Vec::<PathBuf>::new());
    }
    #[test]
    fn unsaved_changes_wait_as_long_as_the_settings_say_before_they_are_autosaved() {
        let mut harness = four_taps_with(Settings {
            autosave_seconds: 10_u32.into(),
            ..Settings::default()
        });
        at_time(&mut harness, 0.0);
        harness.key_press(egui::Key::Num4);
        at_time(&mut harness, 1.0);
        at_time(&mut harness, 10.5);
        assert_eq!(harness.state_mut().take_requests(), []);
        at_time(&mut harness, 11.5);
        assert!(matches!(request(&mut harness), Request::Autosave { .. }));
    }
    #[test]
    fn with_autosave_off_nothing_is_autosaved() {
        let mut harness = four_taps_with(Settings {
            autosave: false,
            ..Settings::default()
        });
        at_time(&mut harness, 0.0);
        harness.key_press(egui::Key::Num4);
        at_time(&mut harness, 1.0);
        at_time(&mut harness, 120.0);
        assert_eq!(harness.state_mut().take_requests(), []);
    }
    #[test]
    fn the_settings_window_turns_autosave_off() {
        let mut harness = editable_demo();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
        harness.run();
        settings_page(&mut harness, "Autosave");
        harness
            .get_by_role_and_label(Role::CheckBox, "Autosave unsaved changes")
            .click();
        harness.run();
        assert!(!harness.state().settings().autosave);
    }
    #[test]
    fn saving_removes_the_autosave() {
        let (mut harness, _) = autosaved_four_taps();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        assert!(matches!(request(&mut harness), Request::Write { .. }));
        harness
            .state_mut()
            .saved(Saved::To(PathBuf::from("four.json")));
        harness.run();
        assert_eq!(request(&mut harness), Request::DropAutosave);
    }
    #[test]
    fn undoing_back_to_the_saved_song_removes_the_autosave() {
        let (mut harness, _) = autosaved_four_taps();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(request(&mut harness), Request::DropAutosave);
    }
    #[test]
    fn closing_without_saving_removes_the_autosave() {
        let (mut harness, _) = autosaved_four_taps();
        close(&mut harness);
        harness.run();
        harness.get_by_label("Don't save").click();
        harness.run();
        let requests = harness.state_mut().take_requests();
        assert_eq!(requests, [Request::DropAutosave]);
    }
    #[test]
    fn a_failed_autosave_says_why_and_tries_again_later() {
        let (mut harness, _) = autosaved_four_taps();
        harness.state_mut().autosave_failed("the disk is full");
        harness.run();
        assert!(
            harness
                .query_by_label_contains("Autosave failed: the disk is full")
                .is_some()
        );
        at_time(&mut harness, 70.0);
        assert!(matches!(request(&mut harness), Request::Autosave { .. }));
    }
    #[test]
    fn lane_keys_leave_a_read_only_song_alone() {
        let mut harness = with_demo();
        harness.key_press(egui::Key::Num1);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [3]);
        assert!(harness.query_by_label_contains("Refused").is_none());
    }
    #[test]
    fn an_empty_window_offers_the_demo_song() {
        let mut harness = window(Window::default());
        harness.get_by_label("Open the demo song").click();
        harness.run();
        assert!(harness.query_by_label("Demo").is_some());
    }
    #[test]
    fn an_opened_song_lists_its_charts() {
        let mut app = Window::default();
        app.open(Path::new("example.json"), example());
        let harness = window(app);
        assert!(harness.query_all_by_label("Future").next().is_some());
        assert!(harness.query_all_by_label("Beyond").next().is_some());
    }
    #[test]
    fn a_chart_says_what_it_cannot_draw_without_its_ruleset() {
        let mut app = Window::default();
        app.open(Path::new("example.json"), example());
        let harness = window(app);
        let note = "1 object on the plane needs the arcaea plugin to be drawn";
        assert!(harness.query_by_label(note).is_some());
    }
    #[test]
    fn clicking_a_chart_opens_it() {
        let mut harness = with_demo();
        harness.get_by_label("Hard").click();
        harness.run();
        let song = harness.state().song.as_ref().unwrap();
        assert_eq!(song.chart_index(), ChartIndex::new(1));
    }
    #[test]
    fn a_file_that_breaks_a_rule_says_what_and_where() {
        let broken = String::from_utf8(example().to_vec())
            .unwrap()
            .replace("\"Beyond\"", "\"Future\"");
        let mut app = Window::default();
        app.open(Path::new("broken.json"), broken.as_bytes());
        let harness = window(app);
        assert!(
            harness
                .query_by_label_contains("broken.json did not open")
                .is_some()
        );
        assert!(harness.query_by_label_contains("charts[1]").is_some());
    }
    #[test]
    fn dropping_a_song_on_the_window_opens_it() {
        let mut harness = window(Window::default());
        let file = Arc::new(Dropped("example.json", example().to_vec()));
        harness.input_mut().dropped_files.push(file);
        harness.run();
        assert!(harness.query_all_by_label("Future").next().is_some());
    }
    #[test]
    fn the_empty_window_offers_a_new_song() {
        let mut harness = window(Window::new(GAMES));
        harness.get_by_label("New song").click();
        harness.run();
        assert!(dialog_open(&harness));
    }
    #[test]
    fn a_new_song_starts_as_one_empty_chart_of_the_first_games_usual_layout() {
        let mut harness = new_song_dialog();
        harness.get_by_label("Create").click();
        harness.run();
        assert!(!dialog_open(&harness));
        assert_eq!(charts(&harness), [("Easy".to_owned(), lanes(4))]);
        let song = harness.state().song.as_ref().unwrap();
        assert_eq!(song.document().document().ruleset.id.as_str(), "sm");
        assert_eq!(song.layers()[0].name, "Layer 0");
        assert!(!song.is_modified());
        assert!(!song.can_undo());
        assert!(song.read_only().is_none());
        assert_eq!(harness.state().title(), "Untitled.json - charter");
    }
    #[test]
    fn a_new_tap_tap_revenge_song_takes_any_levels_named_as_its_game_names_them() {
        let mut harness = new_song_dialog();
        harness.get_by_label("Tap Tap Revenge").click();
        harness.run();
        harness
            .get_by_role_and_label(Role::ComboBox, "Lanes")
            .click();
        harness.run();
        harness.get_by_label("5 lanes").click();
        harness.run();
        let title = harness.get_by_role_and_label(Role::TextInput, "Title");
        title.focus();
        title.type_text("Mine");
        harness.run();
        let preset = harness.get_by_role_and_label(Role::ComboBox, "Preset");
        assert_eq!(preset.value().as_deref(), Some("Custom, Extreme only"));
        preset.click();
        harness.run();
        harness.get_by_label("Tap Tap Reloaded").click();
        harness.run();
        harness
            .get_by_role_and_label(Role::CheckBox, "Easy")
            .click();
        harness.run();
        let selected_preset = harness.get_by_role_and_label(Role::ComboBox, "Preset");
        assert_eq!(selected_preset.value().as_deref(), Some("Your own"));
        harness.get_by_label("Create").click();
        harness.run();
        let named = |name: &str| (name.to_owned(), lanes(5));
        assert_eq!(
            charts(&harness),
            [
                named("Easy"),
                named("Metallica"),
                named("Downloads"),
                named("Customs")
            ]
        );
        let song = harness.state().song.as_ref().unwrap();
        let levels: Vec<_> = song
            .document()
            .document()
            .charts
            .iter()
            .map(|chart| chart.settings["difficulty"].clone())
            .collect();
        assert_eq!(levels, ["easy", "medium", "hard", "extreme"]);
        assert_eq!(harness.state().title(), "Mine.json - charter");
    }
    #[test]
    fn the_new_song_dialog_keeps_its_top_and_its_fields_one_edge() {
        let mut harness = new_song_dialog();
        let top =
            |view: &Harness<'_, Window>| view.get_by_label_contains("Pick the game").rect().top();
        let left = |view: &Harness<'_, Window>, label: &str| {
            view.get_by_role_and_label(Role::TextInput, label)
                .rect()
                .left()
        };
        let before = top(&harness);
        let edge = left(&harness, "Title");
        assert!((left(&harness, "Chart 1") - edge).abs() < 0.5);
        harness.get_by_label("Tap Tap Revenge").click();
        harness.run();
        harness.run();
        assert!((top(&harness) - before).abs() < 0.5);
        let extreme = left(&harness, "Extreme chart name");
        assert!((extreme - edge).abs() < 0.5, "{extreme} {edge}");
        let preset = harness.get_by_role_and_label(Role::ComboBox, "Preset");
        assert!((preset.rect().left() - edge).abs() < 0.5);
    }
    #[test]
    fn a_new_song_needs_a_chart() {
        let mut harness = new_song_dialog();
        harness.get_by_label("Remove Easy").click();
        harness.run();
        assert!(harness.query_by_label("Remove Easy").is_none());
        harness.get_by_label("Create").click();
        harness.run();
        assert!(dialog_open(&harness));
        assert!(harness.state().song.is_none());
    }
    #[test]
    fn a_rejected_new_song_keeps_its_fields_and_shows_why() {
        let mut harness = new_song_dialog();
        let title = harness.get_by_role_and_label(Role::TextInput, "Title");
        title.focus();
        title.type_text("Mine");
        harness.run();
        harness.get_by_label("Add a chart").click();
        harness.run();
        let second = harness.get_by_role_and_label(Role::TextInput, "Chart 2");
        second.focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness
            .get_by_role_and_label(Role::TextInput, "Chart 2")
            .type_text("Easy");
        harness.run();

        harness.get_by_label("Create").click();
        harness.run();
        assert!(harness.state().song.is_none());
        assert!(dialog_open(&harness));
        assert_eq!(
            harness
                .get_by_role_and_label(Role::TextInput, "Title")
                .value()
                .as_deref(),
            Some("Mine")
        );
        assert_eq!(
            harness
                .get_by_role_and_label(Role::TextInput, "Chart 2")
                .value()
                .as_deref(),
            Some("Easy")
        );
        assert!(
            harness
                .query_all_by_label_contains("Could not start the song")
                .next()
                .is_some()
        );
    }
    #[test]
    fn cancelling_the_unsaved_song_prompt_keeps_the_new_song_draft() {
        let mut app = Window::new(GAMES);
        app.open(Path::new("/songs/four.json"), &four_taps());
        app.song.as_mut().unwrap().mark_unsaved();
        let mut harness = window(app);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::N);
        harness.run();
        let title = harness.get_by_role_and_label(Role::TextInput, "Title");
        title.focus();
        title.type_text("Mine");
        harness.run();

        harness.get_by_label("Create").click();
        harness.run();
        assert!(asks_to_save(&harness));
        harness
            .state_mut()
            .answer_save_changes(panels::SaveChanges::Cancel);
        harness.run();

        assert!(dialog_open(&harness));
        assert_eq!(
            harness
                .get_by_role_and_label(Role::TextInput, "Title")
                .value()
                .as_deref(),
            Some("Mine")
        );
        assert_eq!(harness.state().title(), "*four.json - charter");
    }
    #[test]
    fn lane_keys_wait_while_the_new_song_dialog_shows() {
        let mut harness = editable_demo();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::N);
        harness.run();
        harness.key_press(egui::Key::Num1);
        harness.run();
        harness.get_by_label("Cancel").click();
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [3]);
    }
    #[test]
    fn audio_dropped_on_the_new_song_dialog_fills_a_blank_title_and_artist_from_its_tags() {
        let mut harness = new_song_dialog();
        drop_keeping_requests(&mut harness, "/music/track.ogg");
        assert_eq!(
            request(&mut harness),
            Request::ReadTags {
                file: PathBuf::from("/music/track.ogg")
            }
        );
        assert!(harness.query_by_label("track.ogg").is_some());
        read_tags(&mut harness, "/music/track.ogg", "Starlight", "The Lanes");
        assert_eq!(field_value(&harness, "Title").as_deref(), Some("Starlight"));
        assert!(harness.query_by_label_contains(OFFER).is_none());
        harness.get_by_label("Create").click();
        harness.run();
        assert_eq!(
            facts(&harness),
            ("Starlight".to_owned(), "The Lanes".to_owned())
        );
        assert_eq!(audio(&harness), "track.ogg");
        assert_eq!(
            harness.state().audio_file(),
            Some(PathBuf::from("/music/track.ogg"))
        );
        assert!(!harness.state().song.as_ref().unwrap().is_modified());
    }
    #[test]
    fn a_new_songs_title_and_artist_follow_the_tags_of_audio_dropped_on_it() {
        let mut harness = created();
        drop_file(&mut harness, "/music/track.ogg");
        read_tags(&mut harness, "/music/track.ogg", "Starlight", "The Lanes");
        assert_eq!(
            facts(&harness),
            ("Starlight".to_owned(), "The Lanes".to_owned())
        );
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        let defaults = ("Untitled".to_owned(), "Unknown artist".to_owned());
        assert_eq!(facts(&harness), defaults);
    }
    #[test]
    fn the_notefield_identifies_an_empty_chart_without_lanes() {
        let mut document = serde_json::to_value(charter_editor::demo::song().document()).unwrap();
        document["charts"][0]["objects"] = serde_json::json!([]);
        document["charts"][0]["lanes"] = serde_json::Value::Null;
        let mut app = Window::default().with_settings(Settings {
            song_panel: false,
            ..Settings::default()
        });
        app.open(
            Path::new("/songs/without-lanes.json"),
            &serde_json::to_vec(&document).unwrap(),
        );
        let harness = window(app);
        assert!(harness.query_by_label("no lanes").is_some());
    }
    #[test]
    fn a_new_song_asks_for_audio_until_a_file_is_dropped_on_it() {
        let mut harness = created();
        assert!(no_audio(&harness));
        drop_file(&mut harness, "/music/track.ogg");
        assert!(!no_audio(&harness));
        assert_eq!(audio(&harness), "track.ogg");
        assert_eq!(harness.state().title(), "*Untitled.json - charter");
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert!(no_audio(&harness));
    }
    #[test]
    fn audio_that_could_not_be_copied_says_why() {
        let mut harness = created();
        harness.state_mut().audio_not_copied("the disk is full");
        harness.run();
        assert!(
            harness
                .query_by_label_contains("Audio not copied: the disk is full")
                .is_some()
        );
    }
    #[test]
    fn audio_dropped_with_no_song_open_says_to_open_one() {
        let mut harness = window(Window::new(GAMES));
        drop_file(&mut harness, "/music/track.ogg");
        assert!(
            harness
                .query_by_label_contains("Open or start a song before adding its audio")
                .is_some()
        );
    }
    #[test]
    fn audio_dropped_on_a_read_only_song_is_refused() {
        let mut harness = window(Window::new(&[]));
        harness.state_mut().open_demo();
        harness.run();
        drop_file(&mut harness, "/music/track.ogg");
        assert_eq!(audio(&harness), "demo.ogg");
        assert!(harness.query_by_label_contains("Refused").is_some());
    }
    #[test]
    fn stale_onsets_do_not_follow_a_new_audio_file() {
        let mut app = Window::new(PLUGINS);
        app.open(Path::new("/songs/four.json"), &four_taps());
        let onsets: Arc<[charter_audio::Onset]> = Arc::from([]);
        app.onsets_ready(Path::new("/songs/a.ogg"), onsets);
        assert_eq!(app.onsets().map(<[charter_audio::Onset]>::len), Some(0));
        app.use_audio(Path::new("/music/other.ogg"));
        assert!(app.onsets().is_none());
        assert_eq!(app.audio_file(), Some(PathBuf::from("/music/other.ogg")));
    }
}
