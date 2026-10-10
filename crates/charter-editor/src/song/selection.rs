//! Note selection.

#[cfg_attr(
    not(test),
    expect(
        clippy::wildcard_imports,
        reason = "song submodules share one private implementation vocabulary"
    )
)]
use super::*;

impl Song {
    /// The selected notes, earliest first.
    pub fn selection(&self) -> impl Iterator<Item = NoteAt> + '_ {
        self.selection.iter().copied()
    }

    /// Whether an object of the open chart belongs to the selection.
    #[must_use]
    pub fn is_selected(&self, object: &Object) -> bool {
        super::notes::note_at(object).is_some_and(|note| self.selection.contains(&note))
    }

    /// Meets the selection with those of `notes` that the open chart has.
    pub fn select(&mut self, notes: impl IntoIterator<Item = NoteAt>, how: Selecting) {
        let found: Vec<_> = notes.into_iter().filter(|note| self.has(*note)).collect();
        self.meet(found, how);
    }

    /// Meets the selection with the notes of the open chart that start within `ticks` and cover
    /// a lane within `lanes`.
    pub fn select_between(
        &mut self,
        ticks: RangeInclusive<Tick>,
        lanes: RangeInclusive<LaneIndex>,
        how: Selecting,
    ) {
        let (first, last) = (u16::from(*lanes.start()), u16::from(*lanes.end()));
        let width = last
            .checked_sub(first)
            .and_then(|span| LaneCount::new(span.saturating_add(1)));
        let found: Vec<_> = match (width, self.chart()) {
            (Some(width), Some(chart)) => {
                let area = LaneSpan::new(LaneIndex::new(first), width);
                let inside = |object: &&Object| {
                    self.is_editable(object)
                        && ticks.contains(&object.tick)
                        && object
                            .position
                            .lanes()
                            .is_some_and(|span| span.overlaps(area))
                };
                chart
                    .objects
                    .iter()
                    .filter(inside)
                    .filter_map(super::notes::note_at)
                    .collect()
            }
            _ => Vec::new(),
        };
        self.meet(found, how);
    }

    /// Selects every lane note of the open chart.
    pub fn select_all(&mut self) {
        self.selection = self
            .chart()
            .map(|chart| {
                chart
                    .objects
                    .iter()
                    .filter(|object| self.is_editable(object))
                    .filter_map(super::notes::note_at)
                    .collect()
            })
            .unwrap_or_default();
    }

    /// Clears the selected notes without changing the song.
    pub fn clear_selection(&mut self) {
        self.selection.clear();
    }
}

impl Song {
    /// Moves the selection to the notes in `notes` according to `how`.
    fn meet(&mut self, notes: Vec<NoteAt>, how: Selecting) {
        match how {
            Selecting::Replace => self.selection = notes.into_iter().collect(),
            Selecting::Add => self.selection.extend(notes),
            Selecting::Subtract => {
                for note in &notes {
                    self.selection.remove(note);
                }
            }
        }
    }

    /// Whether the open chart has the note `note` names.
    fn has(&self, note: NoteAt) -> bool {
        self.starting(note.tick, note.lane)
            .is_some_and(|(_, object)| {
                super::notes::note_at(object) == Some(note) && self.is_editable(object)
            })
    }

    /// Drops the selected notes an edit took away.
    pub(super) fn prune_selection(&mut self) {
        let selection = std::mem::take(&mut self.selection);
        self.selection = selection
            .into_iter()
            .filter(|note| self.has(*note))
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::super::notes::note;
    use super::*;
    use crate::song::tests::*;
    use charter_core::{LaneOffset, TickOffset};
    use serde_json::json;

    fn four_taps() -> Song {
        let taps = json!([
            object(0, 0, "tap"),
            object(240, 1, "tap"),
            object(480, 2, "tap"),
            object(720, 3, "tap")
        ]);
        Song::new(document(taps), EDITABLE)
    }

    fn note_at(tick: i32, index: u16) -> NoteAt {
        NoteAt {
            tick: at(tick),
            lane: lane(index),
        }
    }

    fn selection(song: &Song) -> Vec<NoteAt> {
        song.selection().collect()
    }

    #[test]
    fn a_marquee_selects_the_notes_that_start_inside_it() {
        let mut song = four_taps();
        song.select_between(at(240)..=at(480), lane(1)..=lane(2), Selecting::Replace);
        assert_eq!(selection(&song), [note_at(240, 1), note_at(480, 2)]);
    }

    #[test]
    fn adding_and_subtracting_meet_the_selection_there_is() {
        let mut song = four_taps();
        song.select_between(at(0)..=at(240), lane(0)..=lane(3), Selecting::Replace);
        song.select([note_at(480, 2)], Selecting::Add);
        song.select_between(at(0)..=at(0), lane(0)..=lane(3), Selecting::Subtract);
        assert_eq!(selection(&song), [note_at(240, 1), note_at(480, 2)]);
        song.select([note_at(720, 3)], Selecting::Replace);
        assert_eq!(selection(&song), [note_at(720, 3)]);
    }

    #[test]
    fn a_wide_note_is_selected_through_any_lane_it_covers() {
        let wide = json!({ "tick": 0, "position": { "lane": { "index": 2, "width": 2 } },
                           "kind": "hold", "shape": { "sustain": { "duration": 480 } } });
        let mut song = Song::new(document(json!([wide])), EDITABLE);
        song.select_between(at(0)..=at(0), lane(3)..=lane(3), Selecting::Replace);
        assert_eq!(selection(&song), [note_at(0, 2)]);
        let chart = song.chart().unwrap();
        assert!(song.is_selected(&chart.objects[0]));
    }

    #[test]
    fn a_note_past_the_last_representable_lane_does_not_fit() {
        let object = note(Tick::new(0), LaneIndex::new(u16::MAX), Kind::Tap);
        assert!(!object.position.fits_in(LaneCount::new(u16::MAX)));
    }

    #[test]
    fn selecting_a_note_that_is_not_there_selects_nothing() {
        let mut song = four_taps();
        song.select([note_at(960, 0), note_at(240, 0)], Selecting::Replace);
        assert_eq!(selection(&song), []);
    }

    #[test]
    fn select_all_takes_the_open_chart_and_opening_another_clears_it() {
        let mut song = four_taps();
        song.select_all();
        assert_eq!(selection(&song).len(), 4);
        song.open_chart(1);
        assert_eq!(selection(&song), []);
    }

    #[test]
    fn deleting_the_selection_is_one_step_that_undo_takes_back() {
        let mut song = four_taps();
        song.select([note_at(0, 0), note_at(480, 2)], Selecting::Replace);
        song.delete_selection().unwrap();
        assert_eq!(
            objects(&song),
            [object(240, 1, "tap"), object(720, 3, "tap")]
        );
        assert_eq!(selection(&song), []);
        song.undo().unwrap();
        assert_eq!(objects(&song).len(), 4);
        assert!(!song.can_undo());
    }

    #[test]
    fn deleting_an_empty_selection_changes_nothing() {
        let mut song = four_taps();
        song.delete_selection().unwrap();
        assert_eq!(objects(&song).len(), 4);
        assert!(!song.can_undo());
    }

    #[test]
    fn moving_the_selection_is_one_step_that_keeps_it_selected() {
        let mut song = four_taps();
        song.select([note_at(0, 0), note_at(480, 2)], Selecting::Replace);
        song.move_selection(TickOffset::new(120), LaneOffset::new(1))
            .unwrap();
        assert_eq!(
            objects(&song),
            [
                object(120, 1, "tap"),
                object(240, 1, "tap"),
                object(600, 3, "tap"),
                object(720, 3, "tap"),
            ]
        );
        assert_eq!(selection(&song), [note_at(120, 1), note_at(600, 3)]);
        song.undo().unwrap();
        assert_eq!(objects(&song), objects(&four_taps()));
        assert!(!song.can_undo());
    }

    #[test]
    fn a_moved_hold_keeps_its_length() {
        let hold = json!({ "tick": 0, "position": { "lane": { "index": 0, "width": 1 } },
                           "kind": "hold", "shape": { "sustain": { "duration": 480 } } });
        let mut song = Song::new(document(json!([hold, object(960, 3, "tap")])), EDITABLE);
        song.select([note_at(0, 0)], Selecting::Replace);
        song.move_selection(TickOffset::new(1920), LaneOffset::new(2))
            .unwrap();
        let moved = json!({ "tick": 1920, "position": { "lane": { "index": 2 } },
                            "kind": "hold", "shape": { "sustain": { "duration": 480 } } });
        assert_eq!(objects(&song), [object(960, 3, "tap"), moved]);
    }

    #[test]
    fn a_moved_note_takes_its_children_along() {
        let hold = |tick: i32, lane: u16| {
            json!({ "tick": tick, "position": { "lane": { "index": lane } }, "kind": "hold",
                    "shape": { "sustain": { "duration": 480 } },
                    "layer": "Layer 0", "children": [{
                        "tick": tick + 240, "position": { "lane": { "index": lane } },
                        "kind": "tap", "shape": "instant" }] })
        };
        let mut song = Song::new(document(json!([hold(0, 0)])), EDITABLE);
        song.select([note_at(0, 0)], Selecting::Replace);
        song.move_selection(TickOffset::new(960), LaneOffset::new(1))
            .unwrap();
        assert_eq!(objects(&song), [hold(960, 1)]);
    }

    #[test]
    fn a_move_onto_a_note_that_stays_is_refused() {
        let mut song = four_taps();
        song.select([note_at(0, 0)], Selecting::Replace);
        let refused = song
            .move_selection(TickOffset::new(240), LaneOffset::new(1))
            .unwrap_err();
        assert!(matches!(refused, Refusal::Occupied), "{refused:?}");
        assert_eq!(objects(&song), objects(&four_taps()));
        assert_eq!(selection(&song), [note_at(0, 0)]);
    }

    #[test]
    fn a_move_off_either_side_of_the_lanes_is_refused() {
        let mut song = four_taps();
        song.select([note_at(240, 1)], Selecting::Replace);
        assert!(
            song.move_selection(TickOffset::new(0), LaneOffset::new(-2))
                .is_err()
        );
        assert!(
            song.move_selection(TickOffset::new(0), LaneOffset::new(3))
                .is_err()
        );
        assert_eq!(objects(&song), objects(&four_taps()));
        assert!(!song.can_undo());
    }

    #[test]
    fn selected_notes_can_trade_places_with_each_other() {
        let mut song = four_taps();
        song.select_all();
        song.move_selection(TickOffset::new(240), LaneOffset::new(0))
            .unwrap();
        assert_eq!(selection(&song).len(), 4);
        assert_eq!(selection(&song)[0], note_at(240, 0));
    }

    #[test]
    fn a_move_by_nothing_or_of_nothing_changes_nothing() {
        let mut song = four_taps();
        song.move_selection(TickOffset::new(240), LaneOffset::new(1))
            .unwrap();
        song.select([note_at(0, 0)], Selecting::Replace);
        song.move_selection(TickOffset::new(0), LaneOffset::new(0))
            .unwrap();
        assert!(!song.can_undo());
    }

    #[test]
    fn a_move_can_span_the_whole_tick_range() {
        let mut song = Song::new(document(json!([object(i32::MIN, 0, "tap")])), EDITABLE);
        song.select_all();
        song.move_selection(TickOffset::new(4_294_967_295), LaneOffset::new(0))
            .unwrap();
        assert_eq!(objects(&song), [object(i32::MAX, 0, "tap")]);
        song.undo().unwrap();
        assert_eq!(objects(&song), [object(i32::MIN, 0, "tap")]);
    }

    #[test]
    fn a_read_only_song_selects_but_refuses_to_delete() {
        let taps = json!([object(0, 0, "tap"), object(240, 1, "tap")]);
        let mut song = Song::new(document(taps), &[]);
        song.select_all();
        assert!(matches!(song.delete_selection(), Err(Refusal::ReadOnly)));
        assert_eq!(selection(&song).len(), 2);
    }

    #[test]
    fn a_selected_note_an_edit_removes_leaves_the_selection() {
        let mut song = four_taps();
        song.select_all();
        song.toggle(at(0), lane(0), Kind::Tap).unwrap();
        assert_eq!(selection(&song).len(), 3);
    }
}
