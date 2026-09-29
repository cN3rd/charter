//! Artwork selection, restoration and save-side bookkeeping.

use super::{Artwork, Path, PathBuf, Picture, Request, Song, Window, refused};

impl Window {
    /// Handles this window operation.
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
    /// Handles this window operation.
    pub fn artwork_embedded(&mut self, file: &Path, extension: &str, picture: Picture) {
        if self
            .dialogs
            .new_song
            .as_ref()
            .is_some_and(|draft| draft.audio() == Some(file))
        {
            self.artwork.embedded = Some((file.to_path_buf(), extension.to_owned(), picture));
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
    /// Handles this window operation.
    pub fn artwork_restored(&mut self, picture: Option<Picture>) {
        let named = self.song.as_ref().and_then(|song| {
            let artwork = song.document().document().metadata.artwork.as_ref();
            artwork.map(ToString::to_string)
        });
        if let (Some(name), Some(picture)) = (named, picture) {
            self.artwork.current = Some(Artwork::taken(name, self.artwork_file(), picture));
        }
    }
    /// Handles this window operation.
    pub fn may_replace(&self, file: &Path) -> bool {
        self.artwork.on_disk.as_deref() == Some(file)
    }
    /// Handles this private window operation.
    pub(super) fn modified(&self) -> bool {
        self.song.as_ref().is_some_and(Song::is_modified)
            || self
                .artwork
                .current
                .as_ref()
                .and_then(Artwork::unsaved)
                .is_some()
    }
    /// Handles this window operation.
    pub fn artwork_read(&mut self, file: &Path, picture: Option<Picture>) {
        if self.artwork_file().as_deref() == Some(file)
            && let Some(artwork) = &mut self.artwork.current
        {
            artwork.read(picture);
        }
    }
    /// Handles this window operation.
    pub fn artwork_write_for(&self, path: &Path) -> Option<(PathBuf, Vec<u8>)> {
        self.artwork
            .current
            .as_ref()?
            .write_for(path, self.artwork_file().as_deref())
    }
    /// Handles this private window operation.
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
    /// Handles this private window operation.
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
            let step = self.song.as_ref().map_or(0, Song::history_step);
            let mut artwork = Artwork::taken(name, self.artwork_file(), picture);
            artwork.set_step(step);
            self.artwork.current = Some(artwork);
            // A picture under the same name leaves the revision as it was, so autosave again.
            self.autosave.autosaved = None;
        }
    }
    /// Handles this private window operation.
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
        let step = self.song.as_ref().map_or(0, Song::history_step);
        let restored_position = self
            .artwork
            .parked
            .iter()
            .position(|parked| parked.name() == named && parked.step() == step);
        // Another picture of the same name may belong to the step undo or redo reached.
        let current = self.artwork.current.as_ref().is_some_and(|artwork| {
            artwork.name() == named
                && artwork.file() == file.as_deref()
                && (artwork.step() == step || restored_position.is_none())
        });
        if current {
            return;
        }
        self.park_artwork();
        // A picture chosen but not saved, which an undo took away, comes back at its step.
        let parked_after_park = self
            .artwork
            .parked
            .iter()
            .position(|parked| parked.name() == named && parked.step() == step);
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
    /// Handles this private window operation.
    pub(super) fn park_artwork(&mut self) {
        let Some(artwork) = self.artwork.current.take() else {
            return;
        };
        if artwork.unsaved().is_some() {
            self.artwork
                .parked
                .retain(|parked| parked.step() != artwork.step());
            self.artwork.parked.push(artwork);
        }
    }
}
