//! The autosave file: the open song with its unsaved changes, and the file it belongs to, kept in
//! one file so the two cannot come apart.

use std::path::{Path, PathBuf};

use charter_core::{Document, Error, RulesetPlugin};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use charter_editor::Song;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Autosave {
    /// The song's file, left out when its name is not text, so a restored song never saves to a
    /// file named differently.
    path: Option<String>,
    song: Value,
}

/// The autosave file for `document`, which belongs to the file at `path`, if it has one.
///
/// # Errors
///
/// Returns [`Error::Json`] if serialization fails, which the document's types rule out.
pub(crate) fn write(path: Option<&Path>, document: &Document) -> Result<Vec<u8>, Error> {
    let autosave = Autosave {
        path: path.and_then(Path::to_str).map(str::to_owned),
        song: serde_json::to_value(document)?,
    };
    Ok(serde_json::to_vec(&autosave)?)
}

/// The song an autosave file holds, checked against `plugins` as any opened song is, and the
/// file it belongs to.
///
/// # Errors
///
/// Returns whatever keeps the bytes from being an autosave of a valid document.
pub(crate) fn read(
    bytes: &[u8],
    plugins: &[&'static dyn RulesetPlugin],
) -> Result<(Option<PathBuf>, Song), Error> {
    let autosave: Autosave = serde_json::from_slice(bytes)?;
    // The song goes through the reader every song goes through, limits and checks included.
    let song = Song::open(&serde_json::to_vec(&autosave.song)?, plugins)?;
    Ok((autosave.path.map(PathBuf::from), song))
}

#[cfg(test)]
mod tests {
    use charter_editor::demo;

    use super::*;

    #[test]
    fn an_autosave_brings_back_the_song_and_the_file_it_belongs_to() {
        let song = demo::song();
        let bytes = write(Some(Path::new("songs/demo.json")), song.document()).unwrap();
        let (path, restored) = read(&bytes, &[]).unwrap();
        assert_eq!(path.as_deref(), Some(Path::new("songs/demo.json")));
        assert_eq!(restored.document(), &song);
    }

    #[test]
    fn an_autosave_of_a_song_without_a_file_belongs_to_none() {
        let bytes = write(None, demo::song().document()).unwrap();
        assert_eq!(read(&bytes, &[]).unwrap().0, None);
    }

    #[cfg(unix)]
    #[test]
    fn a_file_name_that_is_not_text_is_left_out_rather_than_changed() {
        use std::os::unix::ffi::OsStrExt;
        let odd = Path::new(std::ffi::OsStr::from_bytes(b"songs/\xff.json"));
        let bytes = write(Some(odd), demo::song().document()).unwrap();
        assert_eq!(read(&bytes, &[]).unwrap().0, None);
    }

    #[test]
    fn a_damaged_autosave_is_refused() {
        assert!(read(b"", &[]).is_err());
        assert!(read(br#"{ "path": null, "song": { "format_version": 0 } }"#, &[]).is_err());
    }
}
