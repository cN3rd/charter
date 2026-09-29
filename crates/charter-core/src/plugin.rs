//! What a ruleset plugin tells the editor about its game.

use std::error::Error;
use std::fmt::Debug;

use crate::document::{Metadata, Object, Timing};
use crate::json::ValidDocument;
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

/// A change a plugin offers for the selected notes, such as turning taps into another of its
/// game's kinds of note.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteChange {
    /// What the plugin knows the change by, passed back to [`RulesetPlugin::change_note`].
    pub id: String,
    /// What the editor calls it, such as `Arrow`.
    pub name: String,
    /// What it does, for a tooltip.
    pub about: String,
}

/// Timing read from another program's format, ready to take the place of a song's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportedTiming {
    /// Timing map to substitute for the song's.
    pub timing: Timing,
    /// What the format held that the timing leaves out, in words for the user.
    pub left_out: Vec<String>,
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

    /// `object`, a note of a chart `lanes` wide, as the change `id` makes it, or none when the
    /// change does not apply to it. The editor checks the result as it does any edit.
    fn change_note(&self, id: &str, object: &Object, lanes: LaneCount) -> Option<Object> {
        let _: (&str, &Object, LaneCount) = (id, object, lanes);
        None
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
    fn a_plugin_that_only_checks_offers_no_optional_editor_actions() {
        let plugin: &dyn RulesetPlugin = &Checks;
        assert_eq!(plugin.id(), "checks");
        plugin.check(&ValidDocument::from_json(
            br#"{"format_version":1,"ruleset":{"id":"checks","version":"1"},"metadata":{"title":"t","artist":"a","audio":"a.ogg"},"timing":{"anchors":[{"tick":0,"seconds":0}],"tempo":120,"events":[]}}"#
        ).unwrap()).unwrap();
        assert_eq!(plugin.layout_kind(), "Lanes");
        assert!(plugin.text_settings().is_empty());
        assert!(plugin.note_changes().is_empty());
        let object: Object = serde_json::from_str(
            r#"{"tick":0,"position":{"lane":{"index":0}},"kind":"tap","shape":"instant"}"#,
        )
        .unwrap();
        assert_eq!(plugin.change_note("unknown", &object, LaneCount::ONE), None);
    }
}
