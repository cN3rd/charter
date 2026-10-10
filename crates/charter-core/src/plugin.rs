//! What a ruleset plugin tells the editor about its game.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::Debug;
use std::num::NonZeroU32;

use crate::document::{Metadata, Object, Timing};
use crate::json::ValidDocument;
use crate::path::FileName;
use crate::unit::LaneCount;

/// A lane layout every chart of a new song gets, such as StepMania's `dance-single`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    /// The name the game gives the layout.
    pub name: String,
    /// Number of playable lanes in charts of this layout.
    pub lanes: LaneCount,
}

/// What a new song is made from: its metadata, one of its game's [layouts](RulesetPlugin::layouts)
/// by index, and its charts, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewSong {
    /// Metadata supplied by the editor.
    pub metadata: Metadata,
    /// Index into the plugin's [`RulesetPlugin::layouts`].
    pub layout: usize,
    /// Charts to create in order.
    pub charts: Vec<NewChart>,
}

/// A chart of a new song: its name, and the game's standard
/// [difficulty level](Difficulties::levels) it is, by index, when the game has levels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewChart {
    /// Name shown for this difficulty.
    pub name: String,
    /// Index into [`Difficulties::levels`], when applicable.
    pub level: Option<usize>,
}

impl NewChart {
    /// A chart called `name`, of no standard level.
    #[must_use]
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            level: None,
        }
    }
}

/// The standard difficulty levels a game's charts come in, and sets of them a new song can start
/// with, named as the game or its community knows them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Difficulties {
    /// The standard levels, easiest first, such as `Easy`.
    pub levels: Vec<String>,
    /// The sets a new song can start with, the usual one first.
    pub presets: Vec<DifficultyPreset>,
}

/// A set of standard levels, each with the name its chart gets, such as Tap Tap Reloaded's
/// `Metallica` for the standard `Medium`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DifficultyPreset {
    /// Name shown for this group of difficulties.
    pub name: String,
    /// The levels it has, by index into [`Difficulties::levels`], easiest first, and what their
    /// charts are called.
    pub charts: Vec<(usize, String)>,
}

/// Where a setting sits: on the song, or on each of its charts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingScope {
    /// Store the setting on the song.
    Song,
    /// Store the setting separately on each chart.
    Chart,
}

/// A setting a plugin reads as text, which the editor shows as a field and writes as a string,
/// taking the key away when the field is cleared.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextSetting {
    /// The key in the song's or a chart's `settings`, such as `theme`.
    pub key: String,
    /// What the editor calls it, such as `Theme`.
    pub name: String,
    /// What it does, for a tooltip.
    pub about: String,
    /// Location at which the setting is stored.
    pub scope: SettingScope,
}

/// Which of a plugin's [note changes](RulesetPlugin::note_changes) is meant, by its position in
/// that list. Nothing else indexes by this, so it cannot be confused with a chart or lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChangeIndex(usize);

impl ChangeIndex {
    /// The change at `at` in the plugin's list.
    #[must_use]
    pub const fn new(at: usize) -> Self {
        Self(at)
    }

    /// Its position in that list.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

/// A change a plugin offers for the selected notes, such as turning taps into another of its
/// game's kinds of note.
///
/// Not comparable: it carries the behaviour as a function, and function pointers have no
/// meaningful equality.
#[derive(Clone, Debug)]
pub struct NoteChange {
    /// What the editor calls it, such as `Arrow`.
    pub name: String,
    /// What it does, for a tooltip.
    pub about: String,
    /// `object` as this change makes it, or none when the change does not apply to it. The
    /// editor checks the result as it does any edit.
    pub apply: fn(&Object, LaneCount) -> Option<Object>,
}

/// Timing read from another program's format, ready to take the place of a song's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportedTiming {
    /// Timing map to substitute for the song's.
    pub timing: Timing,
    /// What the format held that the timing leaves out, in words for the user.
    pub left_out: Vec<String>,
}

/// A file format a plugin reads whole songs from and writes them to, such as Tap Tap Revenge's
/// `.ttr2_track`.
///
/// Formats may share extensions, as one file format read the ways different games play it; the
/// editor then asks which is meant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SongFormat {
    /// What its users call it, such as `Tap Tap Revenge track`.
    pub name: String,
    /// The extensions its files end in, without the dot, the usual one first.
    pub extensions: Vec<String>,
    /// Files the format keeps beside the song's, by name, which an import is handed when they are
    /// there, such as a Tap Tap Revenge track's `info.plist`.
    pub companions: Vec<FileName>,
}

/// A song read from another program's format, with what the format held that it leaves out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportedSong {
    /// The song, which the plugin that read it accepts.
    pub document: ValidDocument,
    /// What the file held that the song leaves out, in words for the user.
    pub left_out: Vec<String>,
}

/// A song written in another program's format, with what the song held that the file leaves out.
///
/// A format that keeps a song as a folder, such as a Tap Tap Revenge track, names the main file
/// and writes the files beside it: some it makes, and some it asks the host to write from the
/// song's own resources.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportedSong {
    /// The file's contents.
    pub bytes: Vec<u8>,
    /// The name the main file must have, such as `taptrack.ttr2_track`, in the folder the user
    /// chose its place in; none to keep the name the user gave it.
    pub file_name: Option<FileName>,
    /// Files made beside the main one, by name, such as an `info.plist`.
    pub beside: BTreeMap<FileName, Vec<u8>>,
    /// The song's resources written beside the main file, each under a name, such as its audio
    /// as `audio.m4a`.
    pub copies: Vec<(FileName, SongResource)>,
    /// What the song held that the file leaves out, in words for the user.
    pub left_out: Vec<String>,
}

/// How a file of the song's own is written beside an export's main file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SongResource {
    /// The song's audio, as it is.
    Audio,
    /// The song's audio in an MP4 container with an M4A name: AAC and MP3 streams are kept
    /// without re-encoding; other codecs are encoded as AAC at 320 kbps.
    M4aAudio,
    /// The song's artwork, copied byte-for-byte.
    Artwork,
    /// The song's PNG or JPEG artwork, fitted inside a transparent square PNG of this side
    /// length, without cropping.
    SquareArtwork {
        /// Width and height of the PNG canvas in pixels.
        side: NonZeroU32,
    },
}

/// A game's rules over the documents that name it as their ruleset, past the format's own.
///
/// The editor opens a document for editing only when the plugin for its ruleset accepts it, and
/// checks every edit through the same plugin, so an edit the game cannot represent is refused.
/// A plugin that offers [layouts](Self::layouts) can also start new songs.
pub trait RulesetPlugin: Debug {
    /// The ruleset id its documents carry, such as `sm`.
    fn id(&self) -> &str;

    /// Checks `document` against the game.
    ///
    /// # Errors
    ///
    /// Returns the first thing the game cannot represent, and where it is.
    fn check(&self, document: &ValidDocument) -> Result<(), Box<dyn Error + Send + Sync>>;

    /// The game's name as its players know it.
    fn name(&self) -> &str {
        self.id()
    }

    /// What the game calls a layout, such as `Style` or `Keys`.
    #[expect(
        clippy::unnecessary_literal_bound,
        reason = "plugins may name layouts from their data"
    )]
    fn layout_kind(&self) -> &str {
        "Lanes"
    }

    /// The layouts a new song can have, the usual one first. None means the plugin cannot start
    /// songs.
    fn layouts(&self) -> Vec<Layout> {
        Vec::new()
    }

    /// The standard difficulty levels of the game's charts, if it has them. A new song's charts
    /// can then each be one of them.
    fn difficulties(&self) -> Option<Difficulties> {
        None
    }

    /// The settings, of the song or of each chart, that the editor shows as text fields.
    fn text_settings(&self) -> Vec<TextSetting> {
        Vec::new()
    }

    /// The changes the editor offers for selected notes, which [`RulesetPlugin::change_note`]
    /// makes.
    fn note_changes(&self) -> Vec<NoteChange> {
        Vec::new()
    }

    /// A song with no notes made from `song`, which this plugin accepts.
    ///
    /// # Errors
    ///
    /// Returns why the song cannot be made, such as a layout it does not have.
    fn new_song(&self, song: &NewSong) -> Result<ValidDocument, Box<dyn Error + Send + Sync>> {
        let _: &NewSong = song;
        Err(format!("{} cannot start songs", self.name()).into())
    }

    /// The timing format this plugin reads, named as its users know it, such as `osu! timing
    /// points`. None means it reads none.
    fn timing_format(&self) -> Option<&str> {
        None
    }

    /// Timing read from `text`, in the plugin's [timing format](Self::timing_format), for the
    /// song `into`: at its resolution, and keeping what only this game has only when `into` is
    /// of this game. Timing of any game can come from any plugin's format.
    ///
    /// # Errors
    ///
    /// Returns why `text` holds no timing, such as the first line that does not read.
    fn import_timing(
        &self,
        text: &str,
        into: &ValidDocument,
    ) -> Result<ImportedTiming, Box<dyn Error + Send + Sync>> {
        let _: (&str, &ValidDocument) = (text, into);
        Err(format!("{} imports no timing", self.id()).into())
    }

    /// The file formats this plugin reads songs from and writes them to, the usual one first.
    fn song_formats(&self) -> Vec<SongFormat> {
        Vec::new()
    }

    /// The song in `bytes`, a file of the plugin's [song format](Self::song_formats) at index
    /// `format`, as a song of this game, with the format's `companions` found beside it, by name.
    /// The bytes are untrusted.
    ///
    /// # Errors
    ///
    /// Returns why `bytes` holds no song of the format, or the plugin has no such format.
    fn import_song(
        &self,
        format: usize,
        bytes: &[u8],
        companions: &BTreeMap<FileName, Vec<u8>>,
    ) -> Result<ImportedSong, Box<dyn Error + Send + Sync>> {
        let _: (usize, &[u8], &BTreeMap<FileName, Vec<u8>>) = (format, bytes, companions);
        Err(format!("{} imports no songs", self.id()).into())
    }

    /// `song`, which this plugin accepts, as a file of its [song format](Self::song_formats) at
    /// index `format`. `audio` is how long the song's audio lasts, when the editor knows.
    ///
    /// # Errors
    ///
    /// Returns why the song cannot be written, such as nothing in it the format holds.
    fn export_song(
        &self,
        format: usize,
        song: &ValidDocument,
        audio: Option<std::time::Duration>,
    ) -> Result<ExportedSong, Box<dyn Error + Send + Sync>> {
        let _: (usize, &ValidDocument, Option<std::time::Duration>) = (format, song, audio);
        Err(format!("{} exports no songs", self.id()).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::ResourcePath;

    #[derive(Debug)]
    struct Checks;

    #[expect(
        clippy::missing_trait_methods,
        reason = "a test plugin: the trait's defaults for every optional method"
    )]
    impl RulesetPlugin for Checks {
        fn id(&self) -> &'static str {
            "checks"
        }

        fn check(&self, _: &ValidDocument) -> Result<(), Box<dyn Error + Send + Sync>> {
            Ok(())
        }
    }

    #[test]
    fn a_plugin_that_only_checks_cannot_start_songs() {
        assert!(Checks.layouts().is_empty());
        let song = NewSong {
            metadata: Metadata {
                title: "t".to_owned(),
                artist: "a".to_owned(),
                audio: ResourcePath::new("audio.ogg").unwrap(),
                preview: None,
                artwork: None,
                genre: None,
                ext: crate::Extensions::new(),
            },
            layout: 0,
            charts: vec![NewChart::named("Easy")],
        };
        assert_eq!(Checks.difficulties(), None);
        let refused = Checks.new_song(&song).unwrap_err();
        assert_eq!(refused.to_string(), "checks cannot start songs");
        assert_eq!(Checks.name(), "checks");
    }

    #[test]
    fn a_plugin_that_only_checks_imports_no_timing() {
        assert_eq!(Checks.timing_format(), None);
        let document = ValidDocument::from_json(
            br#"{ "format_version": 0, "ruleset": { "id": "checks", "version": "1" },
            "metadata": { "title": "t", "artist": "a", "audio": "a.ogg" },
            "timing": { "offset_seconds": 0.0, "events": [ { "tick": 0, "kind": { "bpm": 120.0 } } ] },
            "charts": [] }"#,
        )
        .unwrap();
        let refused = Checks.import_timing("", &document).unwrap_err();
        assert_eq!(refused.to_string(), "checks imports no timing");
    }

    #[test]
    fn a_plugin_that_only_checks_reads_and_writes_no_song_format() {
        assert!(Checks.song_formats().is_empty());
        let unread = Checks.import_song(0, b"", &BTreeMap::new()).unwrap_err();
        assert_eq!(unread.to_string(), "checks imports no songs");
        let document = ValidDocument::from_json(
            br#"{"format_version":1,"ruleset":{"id":"checks","version":"1"},"metadata":{"title":"t","artist":"a","audio":"a.ogg"},"timing":{"anchors":[{"tick":0,"seconds":0}],"tempo":120,"events":[]}}"#,
        )
        .unwrap();
        let unwritten = Checks.export_song(0, &document, None).unwrap_err();
        assert_eq!(unwritten.to_string(), "checks exports no songs");
    }

    #[test]
    fn a_plugin_that_only_checks_offers_no_optional_editor_actions() {
        let plugin: &dyn RulesetPlugin = &Checks;
        assert_eq!(plugin.id(), "checks");
        plugin.check(&ValidDocument::from_json(
            br#"{"format_version":1,"ruleset":{"id":"checks","version":"1"},"metadata":{"title":"t","artist":"a","audio":"a.ogg"},"timing":{"anchors":[{"tick":0,"seconds":0}],"tempo":120,"events":[]}}"#
        ).unwrap()).unwrap();
        assert_eq!(plugin.layout_kind(), "Lanes");
        assert!(plugin.text_settings().is_empty());
        assert!(plugin.note_changes().is_empty());
        let _object: Object = serde_json::from_str(
            r#"{"tick":0,"position":{"lane":{"index":0}},"kind":"tap","shape":"instant"}"#,
        )
        .unwrap();
    }
}
