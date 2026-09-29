//! The autosave file: the open song with its unsaved changes, and the file it belongs to, kept in
//! one file so the two cannot come apart.

use std::path::{Path, PathBuf};

use charter_core::{Document, Error, RulesetPlugin};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use charter_editor::Song;

/// The autosave's serialised song and the files it needs to restore it.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Autosave {
    /// The song's file, left out when its name is not text, so a restored song never saves to a
    /// file named differently.
    path: Option<String>,
    /// The audio file dropped on the song, which a save copies beside it; left out the same way.
    /// Autosaves from before it was kept have none.
    #[serde(default)]
    audio: Option<String>,
    /// The chart document kept with the autosave.
    song: Value,
    /// The song's artwork, when it is not beside the song yet. Autosaves from before it was kept
    /// have none.
    #[serde(default)]
    artwork: Option<Vec<u8>>,
}

/// What an autosave keeps.
#[derive(Debug)]
pub(crate) struct Kept {
    /// The file the song belongs to.
    pub(crate) path: Option<PathBuf>,
    /// The audio file dropped on the song.
    pub(crate) audio: Option<PathBuf>,
    /// The restored chart and its editing state.
    pub(crate) song: Song,
    /// The song's artwork, as the file it names is to hold it once saved.
    pub(crate) artwork: Option<Vec<u8>>,
}

/// The autosave file for `document`, which belongs to the file at `path` and plays the audio
/// dropped on it at `audio`, if either is known, with the bytes of `artwork` not yet beside it.
///
/// # Errors
///
/// Returns [`Error::Json`] if serialization fails, which the document's types rule out.
pub(crate) fn write(
    path: Option<&Path>,
    audio: Option<&Path>,
    document: &Document,
    artwork: Option<&[u8]>,
) -> Result<Vec<u8>, Error> {
    let text = |value: Option<&Path>| value.and_then(Path::to_str).map(str::to_owned);
    let autosave = Autosave {
        path: text(path),
        audio: text(audio),
        song: serde_json::to_value(document)?,
        artwork: artwork.map(<[u8]>::to_vec),
    };
    Ok(serde_json::to_vec(&autosave)?)
}

/// What an autosave file keeps, its song checked against `plugins` as any opened song is.
///
/// # Errors
///
/// Returns whatever keeps the bytes from being an autosave of a valid document.
pub(crate) fn read(bytes: &[u8], plugins: &[&'static dyn RulesetPlugin]) -> Result<Kept, Error> {
    let autosave: Autosave = serde_json::from_slice(bytes)?;
    // The song goes through the reader every song goes through, limits and checks included.
    let song = Song::open(&serde_json::to_vec(&autosave.song)?, plugins)?;
    Ok(Kept {
        path: autosave.path.map(PathBuf::from),
        audio: autosave.audio.map(PathBuf::from),
        song,
        artwork: autosave.artwork,
    })
}

#[cfg(test)]
mod tests {
    use charter_editor::demo;
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn an_autosave_brings_back_the_song_and_the_file_it_belongs_to() {
        let song = demo::song();
        let bytes = write(
            Some(Path::new("songs/demo.json")),
            Some(Path::new("/music/demo.ogg")),
            song.document(),
            None,
        )
        .unwrap();
        let kept = read(&bytes, &[]).unwrap();
        assert_eq!(kept.path.as_deref(), Some(Path::new("songs/demo.json")));
        assert_eq!(kept.audio.as_deref(), Some(Path::new("/music/demo.ogg")));
        assert_eq!(kept.song.layers()[0].name, "Layer 0");
        assert_eq!(
            kept.song.document(),
            charter_editor::Song::new(song, &[]).document()
        );
    }

    #[test]
    fn an_autosave_from_before_audio_was_kept_still_reads() {
        let song = serde_json::to_value(demo::song().document()).unwrap();
        let older = serde_json::json!({ "path": null, "song": song });
        let kept = read(&serde_json::to_vec(&older).unwrap(), &[]).unwrap();
        assert_eq!(kept.audio, None);
    }

    #[test]
    fn an_autosave_keeps_the_artwork_not_yet_beside_the_song() {
        let bytes = write(None, None, demo::song().document(), Some(b"png")).unwrap();
        assert_eq!(
            read(&bytes, &[]).unwrap().artwork.as_deref(),
            Some(&b"png"[..])
        );
    }

    #[test]
    fn an_autosave_of_a_song_without_a_file_belongs_to_none() {
        let bytes = write(None, None, demo::song().document(), None).unwrap();
        assert_eq!(read(&bytes, &[]).unwrap().path, None);
    }

    #[cfg(unix)]
    #[test]
    fn a_file_name_that_is_not_text_is_left_out_rather_than_changed() {
        use std::os::unix::ffi::OsStrExt;
        let odd = Path::new(std::ffi::OsStr::from_bytes(b"songs/\xff.json"));
        let bytes = write(Some(odd), None, demo::song().document(), None).unwrap();
        assert_eq!(read(&bytes, &[]).unwrap().path, None);
    }

    #[test]
    fn an_autosave_with_fields_it_does_not_know_is_refused() {
        let song = demo::song();
        let bytes = write(None, None, song.document(), None).unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        value["later"] = serde_json::json!(true);
        read(&serde_json::to_vec(&value).unwrap(), &[]).unwrap_err();
    }

    #[test]
    fn a_damaged_autosave_is_refused() {
        read(b"", &[]).unwrap_err();
        read(br#"{ "path": null, "song": { "format_version": 0 } }"#, &[]).unwrap_err();
    }

    proptest! {
        #![proptest_config(crate::proptest_config())]

        #[test]
        fn arbitrary_autosave_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
            drop(read(&bytes, &[]));
        }
    }
}
