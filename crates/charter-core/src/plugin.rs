//! What a ruleset plugin tells the editor about its game.

use std::error::Error;
use std::fmt::Debug;

use crate::document::{Metadata, Timing};
use crate::json::ValidDocument;
use crate::unit::LaneCount;

/// A lane layout every chart of a new song gets, such as StepMania's `dance-single`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    /// The name the game gives the layout.
    pub name: String,
    pub lanes: LaneCount,
}

/// What a new song is made from: its metadata, one of its game's [layouts](RulesetPlugin::layouts)
/// by index, and its charts, in order.
#[derive(Clone, Debug, PartialEq)]
pub struct NewSong {
    pub metadata: Metadata,
    pub layout: usize,
    pub charts: Vec<NewChart>,
}

/// A chart of a new song: its name, and the game's standard
/// [difficulty level](Difficulties::levels) it is, by index, when the game has levels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewChart {
    pub name: String,
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
    pub name: String,
    /// The levels it has, by index into [`Difficulties::levels`], easiest first, and what their
    /// charts are called.
    pub charts: Vec<(usize, String)>,
}

/// Timing read from another program's format, ready to take the place of a song's.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportedTiming {
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
    #[allow(clippy::unnecessary_literal_bound)] // a plugin may name it from its own data
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

    /// A song with no notes made from `song`, which this plugin accepts.
    ///
    /// # Errors
    ///
    /// Returns why the song cannot be made, such as a layout it does not have.
    fn new_song(&self, song: &NewSong) -> Result<ValidDocument, Box<dyn Error + Send + Sync>> {
        let _ = song;
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
        let _ = (text, into);
        Err(format!("{} imports no timing", self.id()).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::ResourcePath;

    #[derive(Debug)]
    struct Checks;

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
}
