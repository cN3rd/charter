//! Opening, saving, autosaving, snapshots and recent files.

use super::{
    Arc, Artwork, Autosave, Discard, Leaving, NO_AUDIO, Offer, Path, PathBuf, Peaks, RECENT,
    Request, Restoring, Saved, Saving, Song, Spectrum, Window, Zoom, demo, file_name, new_song,
    notefield, panels, recovery, refused, shown_strip,
};

impl Window {
    /// Handles this window operation.
    pub fn song_folder(&self) -> Option<PathBuf> {
        self.path.as_ref()?.parent().map(Path::to_path_buf)
    }
    /// Handles this window operation.
    pub fn saved(&mut self, saved: Saved) {
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
                if let Some(discard) = self.dialogs.after_save.take() {
                    self.carry_on(discard);
                }
            }
            Saved::Cancelled => self.dialogs.after_save = None,
            Saved::Failed(why) => {
                self.dialogs.after_save = None;
                self.failure = Some(format!("Could not save: {why}"));
            }
        }
    }
    /// Handles this window operation.
    pub fn recover(&mut self, bytes: &[u8]) {
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
                    "Unsaved changes from last time could not be restored: {error}"
                ));
                self.drop_autosave();
            }
        }
    }
    /// Handles this window operation.
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
    /// Handles this window operation.
    pub fn use_audio(&mut self, file: &Path) {
        if self.dialogs.new_song.is_some() {
            self.draft_audio(file);
        } else {
            self.add_audio(file);
        }
    }
    /// Handles this window operation.
    pub fn tags_read(&mut self, file: &Path, title: Option<String>, artist: Option<String>) {
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
                    draft: true,
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
            self.take_tags(false, named, by);
        } else {
            self.dialogs.offer = Some(Offer {
                draft: false,
                title,
                artist,
            });
        }
    }
    /// Handles this private window operation.
    pub(super) fn take_tags(&mut self, draft: bool, title: Option<&str>, artist: Option<&str>) {
        if draft {
            if let Some(new_song_draft) = &mut self.dialogs.new_song {
                new_song_draft.fill(title, artist);
            }
            return;
        }
        let Some(song) = &mut self.song else {
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
    /// Handles this private window operation.
    pub(super) fn draft_audio(&mut self, file: &Path) {
        if let Some(draft) = &mut self.dialogs.new_song {
            draft.set_audio(Some(file.to_path_buf()));
            self.requests.push(Request::ReadTags {
                file: file.to_path_buf(),
            });
        }
    }
    /// Handles this window operation.
    pub fn waveform_ready(&mut self, file: &Path, peaks: Arc<Peaks>) {
        self.analysis.waveform = Some((file.to_path_buf(), peaks));
    }
    /// Handles this window operation.
    pub fn spectrum_ready(&mut self, file: &Path, spectrum: Arc<Spectrum>) {
        self.analysis.spectrum = Some((file.to_path_buf(), notefield::Tiles::new(spectrum)));
    }
    /// Handles this window operation.
    pub fn onsets_ready(&mut self, file: &Path, onsets: Arc<[charter_audio::Onset]>) {
        self.analysis.onsets = Some((file.to_path_buf(), onsets));
    }
    /// Handles this window operation.
    pub fn onsets(&self) -> Option<&[charter_audio::Onset]> {
        let file = self.audio_file();
        let (of, onsets) = self.analysis.onsets.as_ref()?;
        (Some(of.as_path()) == file.as_deref()).then_some(onsets.as_ref())
    }
    /// Handles this window operation.
    pub fn strip(&self) -> Option<notefield::Strip<'_>> {
        shown_strip(
            &self.settings,
            self.analysis.waveform.as_ref(),
            self.analysis.spectrum.as_ref(),
            self.audio_file().as_deref(),
        )
    }
    /// Handles this window operation.
    pub fn message(&self) -> Option<&str> {
        self.failure.as_deref()
    }
    /// Handles this window operation.
    pub fn audio_not_copied(&mut self, why: &str) {
        self.failure = Some(format!("Audio not copied: {why}"));
    }
    /// Handles this window operation.
    pub fn audio_copy_for(&self, path: &Path) -> Option<(PathBuf, PathBuf)> {
        let song = self.song.as_ref()?;
        let from = self.audio_file()?;
        let to = path
            .parent()?
            .join(song.document().document().metadata.audio.as_str());
        (from != to).then_some((from, to))
    }
    /// Handles this private window operation.
    pub(super) fn forget_stale_audio(&mut self) {
        let wanted = self.audio_file();
        if self
            .playback
            .audio
            .as_ref()
            .is_some_and(|(file, _)| Some(file) != wanted.as_ref())
        {
            self.playback.audio = None;
            if std::mem::take(&mut self.playback.playing) {
                self.requests.push(Request::Pause);
            }
        }
    }
    /// Handles this private window operation.
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
    /// Handles this private window operation.
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
    /// Handles this window operation.
    pub fn autosave_failed(&mut self, why: &str) {
        self.autosave.autosaved = None;
        self.failure = Some(format!("Autosave failed: {why}"));
    }
    /// Handles this private window operation.
    pub(super) fn drop_autosave(&mut self) {
        self.autosave.autosaved = None;
        self.autosave.due = None;
        if std::mem::take(&mut self.autosave.kept) {
            self.requests.push(Request::DropAutosave);
        }
    }
    /// Handles this private window operation.
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
        if self.autosave.autosaved == Some(revision) {
            return;
        }
        let now = ctx.input(|input| input.time);
        let Some(after) = self.settings.autosave_after() else {
            self.autosave.due = None;
            return;
        };
        let due = *self.autosave.due.get_or_insert(now + after);
        if now < due {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(due - now));
            return;
        }
        self.autosave.due = None;
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
                self.autosave.autosaved = Some(revision);
            }
            Err(error) => self.failure = Some(format!("Autosave failed: {error}")),
        }
    }
    /// Handles this private window operation.
    pub(super) fn answer_restore(&mut self, restore: bool) {
        let Some(Restoring {
            path,
            audio,
            mut song,
            artwork,
        }) = self.dialogs.restoring.take()
        else {
            return;
        };
        if restore {
            song.mark_unsaved();
            self.artwork.restored = artwork;
            self.after_asking(Discard::Open {
                path,
                song,
                restored: true,
                audio,
            });
        } else {
            self.drop_autosave();
        }
    }
    /// Handles this window operation.
    pub fn title(&self) -> String {
        let Some(song) = &self.song else {
            return "charter".to_owned();
        };
        let star = if self.modified() { "*" } else { "" };
        format!("{star}{} - charter", self.file_name(song))
    }
    /// Handles this private window operation.
    pub(super) fn file_name(&self, song: &Song) -> String {
        file_name(self.path.as_deref(), song)
    }
    /// Handles this private window operation.
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
    /// Handles this window operation.
    pub fn open(&mut self, path: &Path, bytes: &[u8]) {
        match Song::open(bytes, self.plugins) {
            Ok(song) => self.after_asking(Discard::Open {
                path: Some(path.to_path_buf()),
                song: Box::new(song),
                restored: false,
                audio: None,
            }),
            Err(error) => self.refuse(path, &error),
        }
    }
    /// Handles this window operation.
    pub fn refuse(&mut self, path: &Path, why: &dyn std::fmt::Display) {
        self.failure = Some(format!("{} did not open: {why}", path.display()));
        // A song that no longer opens leaves the recent list.
        self.settings.recent.retain(|recent| recent != path);
    }
    /// Handles this window operation.
    pub fn use_snapshot_folder(&mut self, folder: &Path) {
        self.settings.snapshot_folder = Some(folder.to_path_buf());
        self.settings.snapshots_in_folder = true;
    }
    /// Handles this window operation.
    pub fn snapshot_failed(&mut self, why: &str) {
        self.autosave.snapshotted = None;
        self.failure = Some(format!("Snapshot failed: {why}"));
    }
    /// Handles this window operation.
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
    /// Handles this private window operation.
    pub(super) fn remember(&mut self, path: &Path) {
        let recent = &mut self.settings.recent;
        recent.retain(|recent| recent != path);
        recent.insert(0, path.to_path_buf());
        recent.truncate(RECENT);
    }
    /// Handles this private window operation.
    pub(super) fn snapshot(&mut self, ctx: &egui::Context) {
        let Some(song) = &self.song else {
            return;
        };
        let revision = song.revision();
        let Some(after) = self.settings.snapshot_after() else {
            self.autosave.snapshot_due = None;
            return;
        };
        if self.autosave.snapshotted == Some(revision) {
            return;
        }
        let now = ctx.input(|input| input.time);
        let due = *self.autosave.snapshot_due.get_or_insert(now + after);
        if now < due {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(due - now));
            return;
        }
        self.autosave.snapshot_due = None;
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
                    kept: self.settings.snapshots_kept(),
                });
                self.autosave.snapshotted = Some(revision);
            }
            Err(error) => self.failure = Some(format!("Snapshot failed: {error}")),
        }
    }
    /// Handles this window operation.
    pub fn open_demo(&mut self) {
        let song = Box::new(Song::new(demo::song(), self.plugins));
        self.after_asking(Discard::Open {
            path: None,
            song,
            restored: false,
            audio: None,
        });
    }
    /// Handles this private window operation.
    pub(super) fn after_asking(&mut self, discard: Discard) {
        if self.modified() {
            self.dialogs.discard = Some(discard);
        } else {
            self.carry_on(discard);
        }
    }
    /// Handles this private window operation.
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
            } => {
                if restored {
                    self.autosave.due = None;
                    self.autosave.autosaved = Some(song.revision());
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
                self.autosave.snapshotted = self.song.as_ref().map(Song::revision);
                self.autosave.snapshot_due = None;
                self.zoom = Zoom::default().by(self.settings.zoom / 100.0);
                self.failure = None;
                self.typing.placed = false;
                self.typing.holding.clear();
                self.typing.pressed.clear();
                self.artwork.audio_file = audio;
                self.artwork.current = None;
                self.artwork.parked.clear();
                if std::mem::take(&mut self.playback.playing) {
                    self.requests.push(Request::Pause);
                }
                // A restored song names what its unsaved changes chose, not what is on disk, so
                // no file there counts as its own.
                self.artwork.on_disk = if restored { None } else { self.artwork_file() };
                if restored && let Some(bytes) = self.artwork.restored.take() {
                    self.requests.push(Request::DecodeArtwork { bytes });
                }
                let embedded = self.artwork.embedded.take();
                if let Some((file, extension, picture)) = embedded
                    && self.artwork.audio_file.as_ref() == Some(&file)
                {
                    self.artwork_embedded(&file, &extension, picture);
                }
            }
        }
    }
    /// Handles this private window operation.
    pub(super) fn answer(&mut self, answer: panels::Answer) {
        let Some(discard) = self.dialogs.discard.take() else {
            return;
        };
        match answer {
            panels::Answer::Save => {
                self.dialogs.after_save = Some(discard);
                self.saving = Some(Saving::InPlace);
            }
            panels::Answer::DontSave => self.carry_on(discard),
            panels::Answer::Cancel => {}
        }
    }
}
