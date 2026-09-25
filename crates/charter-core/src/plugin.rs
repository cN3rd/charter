//! What a ruleset plugin tells the editor about its game.

use std::error::Error;
use std::fmt::Debug;

use crate::document::Metadata;
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
/// by index, and the names of its charts, in order.
#[derive(Clone, Debug, PartialEq)]
pub struct NewSong {
    pub metadata: Metadata,
    pub layout: usize,
    pub charts: Vec<String>,
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

    /// A song with no notes made from `song`, which this plugin accepts.
    ///
    /// # Errors
    ///
    /// Returns why the song cannot be made, such as a layout it does not have.
    fn new_song(&self, song: &NewSong) -> Result<ValidDocument, Box<dyn Error + Send + Sync>> {
        let _ = song;
        Err(format!("{} cannot start songs", self.name()).into())
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
                ext: crate::Extensions::new(),
            },
            layout: 0,
            charts: vec!["Easy".to_owned()],
        };
        let refused = Checks.new_song(&song).unwrap_err();
        assert_eq!(refused.to_string(), "checks cannot start songs");
        assert_eq!(Checks.name(), "checks");
    }
}
