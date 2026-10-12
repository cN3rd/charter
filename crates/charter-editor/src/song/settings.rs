//! Song metadata, audio and plugin settings.

#[cfg_attr(
    not(test),
    expect(
        clippy::wildcard_imports,
        reason = "song submodules share one private implementation vocabulary"
    )
)]
use super::*;

impl Song {
    /// Makes the song's audio the file at `audio`, as one step.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song.
    pub fn set_audio(&mut self, audio: ResourcePath) -> Result<(), Refusal> {
        let old = &self.document.document().metadata;
        if old.audio == audio {
            return Ok(());
        }
        self.set_metadata(Metadata {
            audio,
            ..old.clone()
        })
    }

    /// The plugin that checks the song's edits, unless the song is read only.
    #[must_use]
    pub fn plugin(&self) -> Option<&'static dyn RulesetPlugin> {
        match self.rules {
            Rules::Checked(plugin) => Some(plugin),
            Rules::ReadOnly(_) => None,
        }
    }

    /// Sets the setting `key` of the song, or of the open chart, to the text `value`, or takes it
    /// away without one, as one step. Does nothing when it is so already.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song, for a chart setting without a chart, and when the song's
    /// game refuses the value.
    pub fn set_setting(
        &mut self,
        scope: SettingScope,
        key: &str,
        value: Option<String>,
    ) -> Result<(), Refusal> {
        let document = self.document.document();
        let (chart, old) = match scope {
            SettingScope::Song => (None, &document.settings),
            SettingScope::Chart => {
                let chart = document.charts.get(self.chart).ok_or(Refusal::NoChart)?;
                (Some(self.chart), &chart.settings)
            }
        };
        match value.as_deref() {
            Some(value) if old.get(key).and_then(serde_json::Value::as_str) == Some(value) => {
                return Ok(());
            }
            None if !old.contains_key(key) => return Ok(()),
            _ => {}
        }
        let mut new = old.clone();
        match value {
            Some(value) => new.insert(key.to_owned(), serde_json::Value::String(value)),
            None => new.remove(key),
        };
        if new == *old {
            return Ok(());
        }
        let edit = Edit(vec![Op::Settings {
            chart,
            old: old.clone(),
            new,
        }]);
        self.commit(edit)?;
        Ok(())
    }

    /// Gives the song `metadata`, its title, artist, audio and preview, as one step. Does
    /// nothing when it has them already.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song, and when the song's game refuses the metadata.
    pub fn set_metadata(&mut self, metadata: Metadata) -> Result<(), Refusal> {
        let old = &self.document.document().metadata;
        if *old == metadata {
            return Ok(());
        }
        let edit = Edit(vec![Op::Metadata {
            old: Box::new(old.clone()),
            new: Box::new(metadata),
        }]);
        self.commit(edit)?;
        Ok(())
    }
}

impl Song {
    /// Names the song `title` by `artist`, as one step.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song.
    pub fn set_song_facts(&mut self, title: &str, artist: &str) -> Result<(), Refusal> {
        let old = &self.document.document().metadata;
        if old.title == title && old.artist == artist {
            return Ok(());
        }
        self.set_metadata(Metadata {
            title: title.to_owned(),
            artist: artist.to_owned(),
            ..old.clone()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::song::tests::*;
    use serde_json::json;

    fn audio(song: &Song) -> &str {
        song.document().document().metadata.audio.as_str()
    }

    #[test]
    fn setting_the_audio_is_a_step_undo_takes_back() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        song.set_audio(ResourcePath::new("track.ogg").unwrap())
            .unwrap();
        assert_eq!(audio(&song), "track.ogg");
        assert!(song.is_modified());
        song.undo().unwrap();
        assert_eq!(audio(&song), "song.ogg");
        assert!(!song.is_modified());
        song.redo().unwrap();
        assert_eq!(audio(&song), "track.ogg");
    }

    #[test]
    fn new_metadata_is_one_step_and_the_same_metadata_is_none() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        let mut metadata = song.document().document().metadata.clone();
        metadata.title = "Mine".to_owned();
        metadata.preview = Some(charter_core::Preview {
            start_seconds: charter_core::AudioTime::try_from(45.0).unwrap(),
            length_seconds: None,
        });
        song.set_metadata(metadata.clone()).unwrap();
        assert_eq!(song.document().document().metadata, metadata);
        song.set_metadata(metadata).unwrap();
        song.undo().unwrap();
        assert_eq!(song.document().document().metadata.title, "t");
        assert!(!song.can_undo());
    }

    #[test]
    fn a_setting_of_the_song_or_the_open_chart_is_one_step() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        song.set_setting(SettingScope::Song, "theme", Some("Delta".to_owned()))
            .unwrap();
        song.set_setting(SettingScope::Chart, "theme", Some("Neon".to_owned()))
            .unwrap();
        let document = song.document().document();
        assert_eq!(document.settings["theme"], json!("Delta"));
        assert_eq!(document.charts[0].settings["theme"], json!("Neon"));
        song.set_setting(SettingScope::Chart, "theme", None)
            .unwrap();
        assert!(song.document().document().charts[0].settings.is_empty());
        song.set_setting(SettingScope::Chart, "theme", None)
            .unwrap();
        for _ in 0..3 {
            song.undo().unwrap();
        }
        assert!(song.document().document().settings.is_empty());
        assert!(!song.can_undo());
    }

    #[test]
    fn setting_the_audio_it_already_has_changes_nothing() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        song.set_audio(ResourcePath::new("song.ogg").unwrap())
            .unwrap();
        assert!(!song.can_undo());
    }

    #[test]
    fn a_read_only_song_refuses_new_audio() {
        let mut song = Song::new(document(json!([])), &[]);
        let refused = song.set_audio(ResourcePath::new("track.ogg").unwrap());
        assert!(matches!(refused, Err(Refusal::ReadOnly)));
        assert_eq!(audio(&song), "song.ogg");
    }

    fn facts(song: &Song) -> (String, String) {
        let metadata = &song.document().document().metadata;
        (metadata.title.clone(), metadata.artist.clone())
    }

    #[test]
    fn the_title_and_artist_change_as_one_step() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        song.set_song_facts("Starlight", "The Lanes").unwrap();
        assert_eq!(
            facts(&song),
            ("Starlight".to_owned(), "The Lanes".to_owned())
        );
        song.undo().unwrap();
        assert_eq!(facts(&song), ("t".to_owned(), "a".to_owned()));
        assert!(!song.can_undo());
    }

    #[test]
    fn setting_the_title_and_artist_the_song_has_changes_nothing() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        song.set_song_facts("t", "a").unwrap();
        assert!(!song.can_undo());
    }
}
