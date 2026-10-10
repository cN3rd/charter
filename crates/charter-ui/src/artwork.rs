//! The song's cover artwork as the window holds it: what the song names it, the picture's bytes
//! to write beside the song when it is saved, and the picture to preview, as the host decoded it.

use std::path::{Path, PathBuf};

/// A picture the host read and decoded: the file's bytes, and a preview of them.
pub type Picture = (Vec<u8>, egui::ColorImage);

/// The song's artwork, as it stands.
#[derive(Debug)]
pub(crate) struct Artwork {
    /// The file the song names, beside the song.
    name: String,
    /// Where that file is, when the song has been saved: it is read again once this changes.
    file: Option<PathBuf>,
    /// The mark of the song's history that named it, for a picture not yet saved, which an
    /// undo or redo back to that mark brings back.
    mark: u64,
    /// The picture's bytes, once read or chosen.
    bytes: Option<Vec<u8>>,
    /// Whether the bytes are still to be written beside the song.
    unsaved: bool,
    /// The picture waiting to be decoded or uploaded, or the texture already on screen.
    preview: Preview,
}

/// What the song panel shows of the artwork.
enum Preview {
    /// Waiting for the host to read the named file.
    Reading,
    /// Decoded by the host but not yet uploaded as a texture.
    Decoded(egui::ColorImage),
    /// Uploaded texture ready for the song panel.
    Shown(egui::TextureHandle),
    /// Why there is nothing to show.
    Missing(&'static str),
}

impl std::fmt::Debug for Preview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Reading => f.write_str("Reading"),
            Self::Decoded(image) => write!(f, "Decoded({:?})", image.size),
            Self::Shown(texture) => write!(f, "Shown({:?})", texture.size()),
            Self::Missing(why) => write!(f, "Missing({why:?})"),
        }
    }
}

impl Artwork {
    /// The filename named in the song.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// The artwork's path beside a saved song.
    pub(crate) fn file(&self) -> Option<&Path> {
        self.file.as_deref()
    }

    /// The history mark that named this artwork.
    pub(crate) const fn mark(&self) -> u64 {
        self.mark
    }

    /// Records the history mark that named this artwork.
    pub(crate) const fn set_mark(&mut self, mark: u64) {
        self.mark = mark;
    }

    /// Records the path of this artwork beside the song.
    pub(crate) fn set_file(&mut self, file: Option<PathBuf>) {
        self.file = file;
    }

    /// Artwork named `name`, being read from `file`, or not until the song is saved without one.
    pub(crate) fn named(name: &str, file: Option<&Path>) -> Self {
        Self {
            name: name.to_owned(),
            file: file.map(Path::to_path_buf),
            mark: 0,
            bytes: None,
            unsaved: false,
            preview: if file.is_some() {
                Preview::Reading
            } else {
                Preview::Missing("Shown once the song is saved")
            },
        }
    }

    /// Artwork named `name` from `picture`, to be written beside the song when it is saved.
    pub(crate) fn taken(name: String, file: Option<PathBuf>, (bytes, image): Picture) -> Self {
        Self {
            name,
            file,
            mark: 0,
            bytes: Some(bytes),
            unsaved: true,
            preview: Preview::Decoded(image),
        }
    }

    /// Takes what the host read of the file, if it could read and decode it.
    pub(crate) fn read(&mut self, read: Option<Picture>) {
        if self.unsaved {
            return;
        }
        if let Some((bytes, image)) = read {
            self.bytes = Some(bytes);
            self.preview = Preview::Decoded(image);
        } else {
            self.bytes = None;
            self.preview = Preview::Missing("Not a picture beside the song");
        }
    }

    /// Where the artwork is written, and what, when the song is saved to `path` while the
    /// artwork is read from `now`: nothing when it is there already.
    pub(crate) fn write_for(&self, path: &Path, now: Option<&Path>) -> Option<(PathBuf, Vec<u8>)> {
        let to = path.parent()?.join(&self.name);
        let bytes = self.bytes.as_ref()?;
        (self.unsaved || now != Some(to.as_path())).then(|| (to, bytes.clone()))
    }

    /// The picture's bytes while they are still to be written beside the song.
    pub(crate) fn unsaved(&self) -> Option<&[u8]> {
        self.bytes.as_deref().filter(|_| self.unsaved)
    }

    /// Takes that the song was saved, with the artwork beside it.
    pub(crate) const fn saved(&mut self) {
        self.unsaved = false;
    }

    /// The picture to preview, or why there is none, making the texture once.
    #[expect(
        clippy::non_ascii_literal,
        reason = "the artwork loading state uses an ellipsis in its UI label"
    )]
    pub(crate) fn shown(
        &mut self,
        ctx: &egui::Context,
    ) -> Result<&egui::TextureHandle, &'static str> {
        if let Preview::Decoded(image) = &self.preview {
            let texture = ctx.load_texture("artwork", image.clone(), egui::TextureOptions::LINEAR);
            self.preview = Preview::Shown(texture);
        }
        match &self.preview {
            Preview::Shown(texture) => Ok(texture),
            Preview::Reading | Preview::Decoded(_) => Err("Reading…"),
            Preview::Missing(why) => Err(why),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(bytes: &[u8]) -> Picture {
        (
            bytes.to_vec(),
            egui::ColorImage::filled([2, 2], egui::Color32::RED),
        )
    }

    #[test]
    fn taken_artwork_is_written_on_every_save_until_one_succeeds() {
        let mut artwork = Artwork::taken("cover.jpg".to_owned(), None, picture(b"jpeg"));
        let path = Path::new("/songs/Song/Song.json");
        let at = Some(Path::new("/songs/Song/cover.jpg"));
        assert_eq!(
            artwork.write_for(path, at),
            Some((PathBuf::from("/songs/Song/cover.jpg"), b"jpeg".to_vec()))
        );
        artwork.saved();
        assert_eq!(artwork.write_for(path, at), None);
    }

    #[test]
    fn read_artwork_is_copied_only_to_a_new_folder() {
        let mut artwork = Artwork::named("art.png", Some(Path::new("/a/art.png")));
        assert_eq!(
            artwork.write_for(Path::new("/b/s.json"), Some(Path::new("/a/art.png"))),
            None
        );
        artwork.read(Some(picture(b"png")));
        let now = Some(Path::new("/a/art.png"));
        assert_eq!(artwork.write_for(Path::new("/a/s.json"), now), None);
        assert_eq!(
            artwork.write_for(Path::new("/b/s.json"), now),
            Some((PathBuf::from("/b/art.png"), b"png".to_vec()))
        );
    }

    #[test]
    #[expect(
        clippy::non_ascii_literal,
        reason = "the test compares the artwork loading label with its ellipsis"
    )]
    fn a_file_that_is_not_a_picture_previews_why_and_leaves_taken_artwork_alone() {
        let ctx = egui::Context::default();
        let mut artwork = Artwork::named("art.png", Some(Path::new("/a/art.png")));
        assert_eq!(artwork.shown(&ctx).err(), Some("Reading…"));
        artwork.read(None);
        assert_eq!(
            artwork.shown(&ctx).err(),
            Some("Not a picture beside the song")
        );
        let mut taken = Artwork::taken("cover.png".to_owned(), None, picture(b"png"));
        taken.read(None);
        taken.shown(&ctx).unwrap();
        let unsaved = Artwork::named("art.png", None).shown(&ctx).err();
        assert_eq!(unsaved, Some("Shown once the song is saved"));
    }

    #[test]
    fn a_pending_picture_survives_a_stale_read_until_it_is_saved() {
        let path = Path::new("/songs/song.json");
        let file = Path::new("/songs/cover.png");
        let mut artwork = Artwork::taken(
            "cover.png".to_owned(),
            Some(file.to_path_buf()),
            picture(b"chosen"),
        );
        artwork.mark = 3;
        artwork.read(Some(picture(b"old file")));
        assert_eq!(artwork.unsaved(), Some(b"chosen".as_slice()));
        assert_eq!(artwork.mark, 3);
        assert_eq!(
            artwork.write_for(path, Some(file)),
            Some((file.to_path_buf(), b"chosen".to_vec()))
        );
        artwork.saved();
        assert_eq!(artwork.unsaved(), None);
        artwork.read(Some(picture(b"new file")));
        assert_eq!(
            artwork.write_for(Path::new("/elsewhere/song.json"), Some(file)),
            Some((PathBuf::from("/elsewhere/cover.png"), b"new file".to_vec()))
        );
    }

    #[test]
    fn a_saved_picture_shows_one_texture_until_the_file_is_read_again() {
        let ctx = egui::Context::default();
        let mut artwork = Artwork::taken("cover.png".to_owned(), None, picture(b"chosen"));
        artwork.saved();
        let first = artwork.shown(&ctx).unwrap().id();
        assert_eq!(artwork.shown(&ctx).unwrap().id(), first);
        artwork.read(Some(picture(b"new file")));
        let second = artwork.shown(&ctx).unwrap().id();
        assert_ne!(first, second);
        assert_eq!(artwork.shown(&ctx).unwrap().id(), second);
    }

    #[test]
    #[expect(
        clippy::non_ascii_literal,
        reason = "the test compares the artwork loading label with its ellipsis"
    )]
    fn a_missing_or_unreadable_file_cannot_be_copied_to_a_new_folder() {
        let ctx = egui::Context::default();
        let mut artwork = Artwork::named("cover.png", Some(Path::new("/old/cover.png")));
        assert_eq!(artwork.shown(&ctx).err(), Some("Reading…"));
        artwork.read(None);
        assert_eq!(
            artwork.shown(&ctx).err(),
            Some("Not a picture beside the song")
        );
        assert_eq!(artwork.unsaved(), None);
        assert_eq!(artwork.write_for(Path::new("/new/song.json"), None), None);
        artwork.read(Some(picture(b"found")));
        artwork.shown(&ctx).unwrap();
        assert_eq!(
            artwork.write_for(
                Path::new("/new/song.json"),
                Some(Path::new("/old/cover.png"))
            ),
            Some((PathBuf::from("/new/cover.png"), b"found".to_vec()))
        );
    }
}
