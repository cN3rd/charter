//! What a ruleset plugin tells the editor about its game.

use std::error::Error;
use std::fmt::Debug;

use crate::json::ValidDocument;

/// A game's rules over the documents that name it as their ruleset, past the format's own.
///
/// The editor opens a document for editing only when the plugin for its ruleset accepts it, and
/// checks every edit through the same plugin, so an edit the game cannot represent is refused.
pub trait RulesetPlugin: Debug {
    /// The ruleset id its documents carry, such as `sm`.
    fn id(&self) -> &str;

    /// Checks `document` against the game.
    ///
    /// # Errors
    ///
    /// Returns the first thing the game cannot represent, and where it is.
    fn check(&self, document: &ValidDocument) -> Result<(), Box<dyn Error + Send + Sync>>;
}
