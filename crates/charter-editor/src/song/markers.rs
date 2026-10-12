//! Markers, sections, bookmarks and song end queries.

#[cfg_attr(
    not(test),
    expect(
        clippy::wildcard_imports,
        reason = "song submodules share one private implementation vocabulary"
    )
)]
use super::*;

impl Song {
    /// Moves the song's marker with `key` onto `to`, among the markers there, as one step.
    /// Does nothing when the marker is gone.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song, and when a marker of its kind is already at `to`.
    pub fn move_marker(&mut self, key: &MarkerKey, to: Tick) -> Result<(), Refusal> {
        let markers = &self.document.document().markers;
        let Some(index) = markers
            .iter()
            .position(|marker| marker.tick == key.tick && marker.kind == key.kind)
        else {
            return Ok(());
        };
        if key.tick == to {
            return Ok(());
        }
        let mut new = markers.clone();
        let mut moved_marker = new.remove(index);
        moved_marker.tick = to;
        let at = new.partition_point(|marker| marker.tick <= to);
        new.insert(at, moved_marker);
        self.remark(new)
    }
}

impl Song {
    /// The name of the section that starts at the cursor, if one does.
    #[must_use]
    pub fn section(&self) -> Option<&str> {
        let markers = &self.document.document().markers;
        markers
            .iter()
            .find(|marker| marker.tick == self.cursor && marker.kind == MarkerKind::Section)
            .map(|marker| marker.text.as_str())
    }

    /// Names the section that starts at the cursor `text`, starting one there if none does, as
    /// one step. Does nothing when it already has that name.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song.
    pub fn set_section(&mut self, text: &str) -> Result<(), Refusal> {
        if self.section() == Some(text) {
            return Ok(());
        }
        let at = self.cursor;
        let mut new = self.document.document().markers.clone();
        let here = new
            .iter_mut()
            .find(|marker| marker.tick == at && marker.kind == MarkerKind::Section);
        if let Some(section) = here {
            text.clone_into(&mut section.text);
        } else {
            let index = new.partition_point(|marker| marker.tick <= at);
            let section = Marker {
                tick: at,
                kind: MarkerKind::Section,
                text: text.to_owned(),
            };
            new.insert(index, section);
        }
        self.remark(new)
    }

    /// Removes the song's marker with `key` as one step. Does nothing when it is gone.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song.
    pub fn remove_marker(&mut self, key: &MarkerKey) -> Result<(), Refusal> {
        if self.read_only().is_some() {
            return Err(Refusal::ReadOnly);
        }
        let markers = &self.document.document().markers;
        let Some(index) = markers
            .iter()
            .position(|marker| marker.tick == key.tick && marker.kind == key.kind)
        else {
            return Ok(());
        };
        let mut new = markers.clone();
        new.remove(index);
        self.remark(new)
    }

    /// Gives the song the markers `new`, as one step.
    fn remark(&mut self, new: Vec<Marker>) -> Result<(), Refusal> {
        let old = self.document.document().markers.clone();
        let edit = Edit(vec![Op::Markers { old, new }]);
        self.commit(edit)?;
        Ok(())
    }

    /// Where the open chart's last note ends, or tick 0 when it has none.
    #[must_use]
    pub fn end(&self) -> Tick {
        self.chart()
            .and_then(|chart| chart.objects().map(|object| object.end()).max())
            .unwrap_or(Tick::new(0))
    }

    /// The tick of the nearest section past the cursor that way, if there is one.
    #[must_use]
    pub fn section_from(&self, step: Step) -> Option<Tick> {
        let sections = self
            .document
            .document()
            .markers
            .iter()
            .filter(|marker| marker.kind == MarkerKind::Section)
            .map(|marker| marker.tick);
        match step {
            Step::Later => sections.filter(|tick| *tick > self.cursor).min(),
            Step::Earlier => sections.filter(|tick| *tick < self.cursor).max(),
        }
    }

    /// Adds a bookmark at the cursor, or removes the one there, as one step.
    ///
    /// # Errors
    ///
    /// Refuses on a read-only song.
    pub fn toggle_bookmark(&mut self) -> Result<(), Refusal> {
        let at = self.cursor;
        let mut new = self.document.document().markers.clone();
        let here = |marker: &Marker| marker.tick == at && marker.kind == MarkerKind::Bookmark;
        if let Some(index) = new.iter().position(here) {
            new.remove(index);
        } else {
            let index = new.partition_point(|marker| marker.tick <= at);
            let bookmark = Marker {
                tick: at,
                kind: MarkerKind::Bookmark,
                text: String::new(),
            };
            new.insert(index, bookmark);
        }
        self.remark(new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::song::tests::*;
    use serde_json::{Value, json};

    /// A song with `markers`.
    fn marked(markers: &Value) -> Song {
        let value = json!({
            "format_version": 0,
            "ruleset": { "id": "test", "version": "1" },
            "metadata": { "title": "t", "artist": "a", "audio": "song.ogg" },
            "timing": { "offset_seconds": 0.0, "events": [ bpm_event(0, 120.0) ] },
            "markers": markers.clone(),
            "charts": [ { "name": "Easy", "lanes": 4 } ]
        });
        let document = ValidDocument::from_json(&serde_json::to_vec(&value).unwrap()).unwrap();
        Song::new(document, EDITABLE)
    }

    fn marker(tick: i32, kind: &str, text: &str) -> Value {
        json!({ "tick": tick, "kind": kind, "text": text })
    }

    fn markers(song: &Song) -> Value {
        serde_json::to_value(&song.document().document().markers).unwrap()
    }

    #[test]
    fn a_section_named_where_there_is_none_is_a_new_one_undo_takes_back() {
        let mut song = marked(&json!([
            marker(0, "section", "Intro"),
            marker(480, "bookmark", "b")
        ]));
        song.jump_to(Tick::new(480));
        assert_eq!(song.section(), None);
        song.set_section("Verse").unwrap();
        let expected = json!([
            marker(0, "section", "Intro"),
            marker(480, "bookmark", "b"),
            marker(480, "section", "Verse")
        ]);
        assert_eq!(markers(&song), expected);
        assert_eq!(song.section(), Some("Verse"));
        song.undo().unwrap();
        assert_eq!(song.section(), None);
    }

    #[test]
    fn a_section_named_where_one_is_renames_it() {
        let mut song = marked(&json!([marker(0, "section", "Intro")]));
        song.set_section("Start").unwrap();
        assert_eq!(markers(&song), json!([marker(0, "section", "Start")]));
        song.set_section("Start").unwrap();
        assert!(song.can_undo());
        song.undo().unwrap();
        assert!(!song.can_undo());
    }

    #[test]
    fn a_marker_removed_goes_as_one_step() {
        let mut song = marked(&json!([
            marker(0, "section", "Intro"),
            marker(480, "bookmark", "b")
        ]));
        let first = song.document().document().markers[0].key();
        song.remove_marker(&first).unwrap();
        assert_eq!(markers(&song), json!([marker(480, "bookmark", "b")]));
        song.remove_marker(&first).unwrap();
        song.undo().unwrap();
        assert_eq!(markers(&song).as_array().unwrap().len(), 2);
    }

    #[test]
    fn jumping_puts_the_cursor_on_the_tick() {
        let mut song = marked(&json!([]));
        song.jump_to(Tick::new(1234));
        assert_eq!(song.cursor(), Tick::new(1234));
    }

    #[test]
    fn a_read_only_song_refuses_marker_edits() {
        let document = marked(&json!([marker(0, "section", "Intro")]))
            .document()
            .clone();
        let mut song = Song::new(document, &[]);
        let key = song.document().document().markers[0].key();
        assert!(matches!(song.set_section("x"), Err(Refusal::ReadOnly)));
        assert!(matches!(song.remove_marker(&key), Err(Refusal::ReadOnly)));
    }

    #[test]
    fn the_end_is_where_the_open_charts_last_note_ends() {
        let taps = json!([
            object(0, 0, "tap"),
            hold(480, 1, 960),
            object(960, 2, "tap")
        ]);
        let song = Song::new(document(taps), EDITABLE);
        assert_eq!(song.end(), Tick::new(1440));
        let empty = Song::new(document(json!([])), EDITABLE);
        assert_eq!(empty.end(), Tick::new(0));
    }

    #[test]
    fn the_next_and_previous_sections_are_found_from_the_cursor() {
        let mut song = marked(&json!([
            marker(0, "section", "A"),
            marker(480, "bookmark", "b"),
            marker(960, "section", "B")
        ]));
        assert_eq!(song.section_from(Step::Later), Some(Tick::new(960)));
        assert_eq!(song.section_from(Step::Earlier), None);
        song.jump_to(Tick::new(600));
        assert_eq!(song.section_from(Step::Earlier), Some(Tick::new(0)));
        song.jump_to(Tick::new(960));
        assert_eq!(song.section_from(Step::Later), None);
    }

    #[test]
    fn a_bookmark_toggled_at_the_cursor_comes_and_goes() {
        let mut song = marked(&json!([marker(0, "section", "A")]));
        song.toggle_bookmark().unwrap();
        assert_eq!(
            markers(&song),
            json!([marker(0, "section", "A"), marker(0, "bookmark", "")])
        );
        song.toggle_bookmark().unwrap();
        assert_eq!(markers(&song), json!([marker(0, "section", "A")]));
    }

    #[test]
    fn a_marker_moved_lands_in_order_and_undo_puts_it_back() {
        let mut song = marked(&json!([
            marker(0, "section", "A"),
            marker(480, "section", "B")
        ]));
        let key = song.document().document().markers[0].key();
        song.move_marker(&key, Tick::new(960)).unwrap();
        assert_eq!(
            markers(&song),
            json!([marker(480, "section", "B"), marker(960, "section", "A")])
        );
        song.undo().unwrap();
        assert_eq!(markers(&song)[0], marker(0, "section", "A"));
    }

    #[test]
    fn a_read_only_song_refuses_to_move_handles() {
        let document = marked(&json!([marker(0, "section", "A")]))
            .document()
            .clone();
        let mut song = Song::new(document, &[]);
        let key = song.document().document().markers[0].key();
        assert!(matches!(
            song.move_marker(&key, Tick::new(480)),
            Err(Refusal::ReadOnly)
        ));
        assert!(matches!(
            song.move_timing_at(Tick::new(0), Tick::new(480)),
            Err(Refusal::ReadOnly)
        ));
    }
    #[test]
    fn removing_a_marker_no_longer_in_the_song_changes_nothing() {
        let mut song = marked(&json!([marker(0, "section", "A")]));
        let key = MarkerKey {
            tick: Tick::new(960),
            kind: MarkerKind::Section,
        };
        song.remove_marker(&key).unwrap();
        assert!(!song.can_undo());
        assert!(!song.is_modified());
    }
}
