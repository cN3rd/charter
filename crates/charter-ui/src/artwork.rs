//! The song's cover artwork as the window holds it: what the song names it, the picture's bytes
//! to write beside the song when it is saved, and the picture to preview, as the host decoded it.

use std::path::{Path, PathBuf};

/// A picture the host read and decoded: the file's bytes, and a preview of them.
pub type Picture = (Vec<u8>, egui::ColorImage);

/// The song's artwork, as it stands.
#[derive(Debug)]
pub struct Artwork {
    /// The file the song names, beside the song.
    pub name: String,
    /// Where that file is, when the song has been saved: it is read again once this changes.
    pub file: Option<PathBuf>,
    /// The picture's bytes, once read or chosen.
    bytes: Option<Vec<u8>>,
    /// Whether the bytes are still to be written beside the song.
    unsaved: bool,
    preview: Preview,
}

/// What the song panel shows of the artwork.
enum Preview {
    Reading,
    Decoded(egui::ColorImage),
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
    /// Artwork named `name`, being read from `file`, or not until the song is saved without one.
    pub fn named(name: &str, file: Option<&Path>) -> Self {
        Self {
            name: name.to_owned(),
            file: file.map(Path::to_path_buf),
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
    pub fn taken(name: String, file: Option<PathBuf>, (bytes, image): Picture) -> Self {
        Self {
            name,
            file,
            bytes: Some(bytes),
            unsaved: true,
            preview: Preview::Decoded(image),
        }
    }

    /// Takes what the host read of the file, if it could read and decode it.
    pub fn read(&mut self, read: Option<Picture>) {
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
    pub fn write_for(&self, path: &Path, now: Option<&Path>) -> Option<(PathBuf, Vec<u8>)> {
        let to = path.parent()?.join(&self.name);
        let bytes = self.bytes.as_ref()?;
        (self.unsaved || now != Some(to.as_path())).then(|| (to, bytes.clone()))
    }

    /// Takes that the song was saved, with the artwork beside it.
    pub fn saved(&mut self) {
        self.unsaved = false;
    }

    /// The picture to preview, or why there is none, making the texture once.
    pub fn shown(&mut self, ctx: &egui::Context) -> Result<&egui::TextureHandle, &'static str> {
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
        assert!(taken.shown(&ctx).is_ok());
        let unsaved = Artwork::named("art.png", None).shown(&ctx).err();
        assert_eq!(unsaved, Some("Shown once the song is saved"));
    }
}
