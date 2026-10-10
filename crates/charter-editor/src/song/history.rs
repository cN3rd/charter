//! Undo, redo and edit history.

#[cfg_attr(
    not(test),
    expect(
        clippy::wildcard_imports,
        reason = "song submodules share one private implementation vocabulary"
    )
)]
use super::*;

/// A position in the undo history.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct HistoryStep(usize);

impl HistoryStep {
    /// The first history position.
    const ZERO: Self = Self(0);

    /// Wraps a raw step at public API boundaries.
    pub(super) const fn new(step: usize) -> Self {
        Self(step)
    }

    /// Raw index for collection access and public API returns.
    pub(super) const fn get(self) -> usize {
        self.0
    }

    /// Previous step, when one exists.
    fn previous(self) -> Option<Self> {
        self.0.checked_sub(1).map(Self)
    }

    /// One step later, saturating at `usize::MAX`.
    const fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

/// The charts whose layers `edit` changes.
fn layered(edit: &Edit) -> Vec<usize> {
    edit.0
        .iter()
        .filter_map(|op| {
            if let Op::Layers { chart, .. } = op {
                Some(*chart)
            } else {
                None
            }
        })
        .collect()
}

/// An edit and where it was made, which undo and redo return to.
#[derive(Clone, Debug)]
pub(super) struct Change {
    /// Chart open when the edit was made.
    chart: usize,
    /// Tick to restore with this edit during undo or redo.
    cursor: Tick,
    /// Operations recorded as one undo step.
    edit: Edit,
}

/// Undo history and save-state bookkeeping.
#[derive(Debug)]
pub(super) struct History {
    /// Every edit, oldest first. The first `done` are in effect; the rest were undone.
    changes: Vec<Change>,
    /// Number of history entries currently applied.
    done: HistoryStep,
    /// How many edits were in effect when the song was opened or last saved, unless no undo or
    /// redo can bring that state back.
    saved: Option<HistoryStep>,
    /// How many times edits, undos and redos have changed the notes view.
    revision: u64,
}

impl History {
    /// Empty history for a newly opened, unmodified song.
    pub(super) const fn new() -> Self {
        Self {
            changes: Vec::new(),
            done: HistoryStep::ZERO,
            saved: Some(HistoryStep::ZERO),
            revision: 0,
        }
    }

    /// Records `edit`, merging with the last step when the edit type allows it.
    pub(super) fn record(&mut self, chart: usize, cursor: Tick, edit: Edit) {
        self.touch_revision();
        self.changes.truncate(self.done.get());
        if self.saved.is_some_and(|saved| saved > self.done) {
            self.saved = None;
        }
        if let Some(last) = self.changes.last_mut()
            && last.chart == chart
            && last.edit.absorb(&edit)
        {
            if self.saved == Some(self.done) {
                self.saved = None;
            }
            return;
        }
        self.changes.push(Change {
            chart,
            cursor,
            edit,
        });
        self.done = HistoryStep::new(self.changes.len());
    }

    /// Newest applied edit, cloned so validation can fail without moving history.
    pub(super) fn undo_candidate(&self) -> Option<(HistoryStep, Change)> {
        let last = self.done.previous()?;
        Some((last, self.changes.get(last.get())?.clone()))
    }

    /// Marks an undo candidate as no longer applied.
    pub(super) const fn finish_undo(&mut self, done: HistoryStep) {
        self.done = done;
    }

    /// Oldest undone edit, cloned so validation can fail without moving history.
    pub(super) fn redo_candidate(&self) -> Option<Change> {
        self.changes.get(self.done.get()).cloned()
    }

    /// Marks a redo candidate as applied.
    pub(super) const fn finish_redo(&mut self) {
        self.done = self.done.next();
    }

    /// Whether the last timing edit can absorb another timing nudge.
    pub(super) fn can_merge_timing(&self, chart: usize, own: bool) -> bool {
        self.done.get() == self.changes.len()
            && self.saved != Some(self.done)
            && self.changes.last().is_some_and(|last| {
                last.chart == chart
                    && matches!(last.edit.0.as_slice(), [Op::Timing { own: was, .. }] if *was == own)
            })
    }

    /// Replaces the newest timing edit's destination with `edit`'s destination.
    pub(super) fn merge_last_timing(&mut self, edit: &mut Edit) {
        if let Some(Change {
            edit: Edit(ops), ..
        }) = self.changes.last_mut()
            && let [Op::Timing { new: kept, .. }] = ops.as_mut_slice()
            && let [Op::Timing { new: incoming, .. }] = edit.0.as_mut_slice()
        {
            std::mem::swap(kept, incoming);
        }
    }

    /// Whether the current document differs from the saved step.
    pub(super) const fn is_modified(&self) -> bool {
        !matches!(self.saved, Some(saved) if saved.get() == self.done.get())
    }

    /// Marks the current step saved.
    pub(super) const fn mark_saved(&mut self) {
        self.saved = Some(self.done);
    }

    /// Marks the song as unsaved.
    pub(super) const fn mark_unsaved(&mut self) {
        self.saved = None;
    }

    /// Current revision number.
    pub(super) const fn revision(&self) -> u64 {
        self.revision
    }

    /// Counts a document-changing operation for revision comparisons.
    pub(super) const fn touch_revision(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }

    /// Folds every done step since `step` into one step when they share a chart.
    pub(super) fn merge_since(&mut self, step: HistoryStep) {
        let step = step.get();
        let done = self.done.get();
        let Some(steps) = self.changes.get(step..done) else {
            return;
        };
        let chart = steps.first().map(|change| change.chart);
        if steps.len() < 2 || steps.iter().any(|change| Some(change.chart) != chart) {
            return;
        }
        let merged = steps.len();
        let mut drained = self.changes.drain(step..done);
        let first = drained.next();
        let Some(first) = first else {
            return;
        };
        let mut ops = first.edit.0;
        ops.extend(drained.flat_map(|change| change.edit.0));
        self.changes.insert(
            step,
            Change {
                chart: first.chart,
                cursor: first.cursor,
                edit: Edit(ops),
            },
        );
        self.done = HistoryStep::new(done.saturating_sub(merged.saturating_sub(1)));
        self.saved = match self.saved {
            Some(saved) if saved.get() <= step => Some(saved),
            Some(saved) if saved.get() >= step.saturating_add(merged) => Some(HistoryStep::new(
                saved.get().saturating_sub(merged.saturating_sub(1)),
            )),
            _ => None,
        };
    }

    /// Number of applied history steps.
    pub(super) const fn step(&self) -> HistoryStep {
        self.done
    }

    /// Whether an edit can be undone.
    pub(super) const fn can_undo(&self) -> bool {
        self.done.get() > 0
    }

    /// Whether an edit can be redone.
    pub(super) const fn can_redo(&self) -> bool {
        self.done.get() < self.changes.len()
    }
}

impl Song {
    /// Records an edit made through a `Song` method and prunes removed selections.
    pub(super) fn record(&mut self, edit: Edit) {
        self.history.record(self.chart, self.cursor, edit);
        self.prune_selection();
    }

    /// The document with `edit` made to chart `chart`, if the format and the game allow it.
    pub(crate) fn edited(&self, chart: usize, edit: &Edit) -> Result<ValidDocument, Refusal> {
        let Rules::Checked(plugin) = self.rules else {
            return Err(Refusal::ReadOnly);
        };
        // ponytail: every edit still validates and plugin-checks the whole document, about 3 ms
        // at 10,000 notes in a release build; check only the edited chart once live recording or
        // bulk edits need it faster.
        let mut document = self.document.document().clone();
        if !edit.apply(&mut document, chart) {
            return Err(Refusal::NoChart);
        }
        let edited = ValidDocument::new(document)?;
        plugin.check(&edited).map_err(Refusal::Game)?;
        Ok(edited)
    }

    /// Takes back the newest edit still in effect, returning to the chart and cursor it was made
    /// at. Does nothing when no edit is left.
    ///
    /// # Errors
    ///
    /// Refuses when the song as it was no longer passes its plugin, which cannot happen while
    /// the plugin's rules stay the same.
    pub fn undo(&mut self) -> Result<(), Refusal> {
        let Some((last, change)) = self.history.undo_candidate() else {
            return Ok(());
        };
        let layered = layered(&change.edit);
        self.document = self.edited(change.chart, &change.edit.inverse())?;
        let (chart, cursor) = (change.chart, change.cursor);
        self.history.finish_undo(last);
        self.return_to(chart, cursor);
        self.forget_solos(&layered);
        Ok(())
    }

    /// Makes the oldest undone edit again, at the chart and cursor it was made at. Does nothing
    /// when none was undone since the last edit.
    ///
    /// # Errors
    ///
    /// Refuses when the edit no longer passes the song's plugin, which cannot happen while the
    /// plugin's rules stay the same.
    pub fn redo(&mut self) -> Result<(), Refusal> {
        let Some(change) = self.history.redo_candidate() else {
            return Ok(());
        };
        let layered = layered(&change.edit);
        self.document = self.edited(change.chart, &change.edit)?;
        let (chart, cursor) = (change.chart, change.cursor);
        self.history.finish_redo();
        self.return_to(chart, cursor);
        self.forget_solos(&layered);
        Ok(())
    }

    /// Forgets the solo of each of `charts`, whose layers an undo or redo changed.
    fn forget_solos(&mut self, charts: &[usize]) {
        for chart in charts {
            if let Some(state) = self.chart_state.get_mut(*chart) {
                state.solo = None;
            }
        }
    }

    /// Whether the song differs from when it was opened or last saved.
    #[must_use]
    pub const fn is_modified(&self) -> bool {
        self.history.is_modified()
    }

    /// Takes the song as it stands to be what its file holds.
    pub const fn mark_saved(&mut self) {
        self.history.mark_saved();
    }

    /// Takes the song to differ from what its file holds, as a song restored from an autosave
    /// does.
    pub const fn mark_unsaved(&mut self) {
        self.history.mark_unsaved();
    }

    /// A count of the changes made to the song's notes, edits, undos and redos alike, so two
    /// equal revisions of one song hold the same notes.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.history.revision()
    }
}

impl Song {
    /// Opens `chart` with the cursor where an undone or redone edit was made.
    pub(super) fn return_to(&mut self, chart: usize, cursor: Tick) {
        self.history.touch_revision();
        self.open_chart(chart);
        self.cursor = cursor;
        self.prune_selection();
    }

    /// Folds every step done since `step`, a [`Song::history_step`], into one, so they undo and
    /// redo together: for a chord of lane keys and the holds it grows while playing. Does nothing
    /// unless the steps are two or more, all on one chart.
    pub fn merge_since(&mut self, step: usize) {
        self.history.merge_since(HistoryStep::new(step));
    }

    /// How many steps of the history are done: where undo and redo have left the song, so
    /// state kept beside it, such as a picture not yet saved, can follow them.
    #[must_use]
    pub const fn history_step(&self) -> usize {
        self.history.step().get()
    }

    /// Whether there is an edit left to undo.
    #[must_use]
    pub const fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    /// Whether there is an undone edit to redo.
    #[must_use]
    pub const fn can_redo(&self) -> bool {
        self.history.can_redo()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::song::tests::*;
    use proptest::prelude::*;
    use serde_json::{Value, json};

    #[derive(Clone, Debug)]
    enum Action {
        Toggle { tick: i32, lane: u16, mine: bool },
        Stretch { from: i32, lane: u16, to: i32 },
        Step(Step),
        OpenChart(usize),
        SelectAll,
        DeleteSelection,
        AddLayer,
    }

    impl Action {
        fn apply(&self, song: &mut Song) {
            match *self {
                Self::Toggle {
                    tick,
                    lane: lane_index,
                    mine,
                } => {
                    let kind = if mine { Kind::Mine } else { Kind::Tap };
                    match song.toggle(at(tick), lane(lane_index), kind) {
                        Ok(_) | Err(_) => {}
                    }
                }
                Self::Stretch {
                    from,
                    lane: lane_index,
                    to,
                } => match song.stretch(lane(lane_index), at(from), at(to)) {
                    Ok(()) | Err(_) => {}
                },
                Self::Step(step) => song.step(step),
                Self::OpenChart(index) => song.open_chart(index),
                Self::SelectAll => song.select_all(),
                Self::DeleteSelection => match song.delete_selection() {
                    Ok(()) | Err(_) => {}
                },
                Self::AddLayer => match song.add_layer() {
                    Ok(()) | Err(_) => {}
                },
            }
        }
    }

    fn action() -> impl Strategy<Value = Action> {
        prop_oneof![
            (0_i32..=4, 0_u16..4, any::<bool>()).prop_map(|(slot, lane, mine)| {
                Action::Toggle {
                    tick: slot.saturating_mul(240),
                    lane,
                    mine,
                }
            }),
            (0_i32..=4, 0_u16..4, 0_i32..=4).prop_map(|(from, lane, to)| Action::Stretch {
                from: from.saturating_mul(240),
                lane,
                to: to.saturating_mul(240),
            }),
            prop_oneof![
                Just(Action::Step(Step::Earlier)),
                Just(Action::Step(Step::Later))
            ],
            (0_usize..3).prop_map(Action::OpenChart),
            Just(Action::SelectAll),
            Just(Action::DeleteSelection),
            Just(Action::AddLayer),
        ]
    }

    #[test]
    fn a_note_placed_and_stretched_undoes_in_one_step() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        song.toggle(at(0), lane(0), Kind::Tap).unwrap();
        song.stretch(lane(0), at(0), at(240)).unwrap();
        song.stretch(lane(0), at(0), at(480)).unwrap();
        song.undo().unwrap();
        assert_eq!(objects(&song), Vec::<Value>::new());
        assert!(!song.can_undo());
        song.redo().unwrap();
        assert_eq!(objects(&song), [hold(0, 0, 480)]);
    }

    #[test]
    fn steps_merged_since_a_point_undo_and_redo_as_one() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        let start = song.history_step();
        song.toggle(at(0), lane(0), Kind::Tap).unwrap();
        song.toggle(at(0), lane(1), Kind::Tap).unwrap();
        song.stretch(lane(0), at(0), at(480)).unwrap();
        song.merge_since(start);
        assert_eq!(song.history_step(), start + 1);
        song.undo().unwrap();
        assert_eq!(objects(&song), Vec::<Value>::new());
        assert!(!song.can_undo());
        song.redo().unwrap();
        assert_eq!(objects(&song), [hold(0, 0, 480), object(0, 1, "tap")]);
    }

    #[test]
    fn a_saved_state_inside_merged_steps_cannot_be_restored_by_undo_or_redo() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        let start = song.history_step();
        song.toggle(at(0), lane(0), Kind::Tap).unwrap();
        song.mark_saved();
        song.toggle(at(240), lane(1), Kind::Tap).unwrap();
        song.merge_since(start);
        assert!(song.is_modified());
        song.undo().unwrap();
        assert_eq!(objects(&song), Vec::<Value>::new());
        assert!(song.is_modified());
        song.redo().unwrap();
        assert_eq!(objects(&song), [object(0, 0, "tap"), object(240, 1, "tap")]);
        assert!(song.is_modified());
    }

    #[test]
    fn merging_edits_after_the_saved_step_keeps_the_saved_state_reachable() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        song.toggle(at(0), lane(0), Kind::Tap).unwrap();
        song.mark_saved();
        let start = song.history_step();
        song.toggle(at(240), lane(1), Kind::Tap).unwrap();
        song.toggle(at(480), lane(2), Kind::Tap).unwrap();
        song.merge_since(start);
        song.undo().unwrap();
        assert_eq!(objects(&song), [object(0, 0, "tap")]);
        assert!(!song.is_modified());
        song.redo().unwrap();
        assert!(song.is_modified());
    }

    /// A song with a tap placed on lane 1 at tick 0 and on lane 2 at tick 240, and the cursor
    /// left three sixteenths on.
    fn two_taps_placed() -> Song {
        let mut song = Song::new(document(json!([])), EDITABLE);
        song.toggle(song.cursor(), lane(0), Kind::Tap).unwrap();
        song.step(Step::Later);
        song.toggle(song.cursor(), lane(1), Kind::Tap).unwrap();
        song.step(Step::Later);
        song.step(Step::Later);
        song
    }

    #[test]
    fn undo_takes_edits_back_newest_first_and_returns_the_cursor_to_each() {
        let mut song = two_taps_placed();
        song.undo().unwrap();
        assert_eq!(objects(&song), [object(0, 0, "tap")]);
        assert_eq!(song.cursor(), at(240));
        song.undo().unwrap();
        assert_eq!(objects(&song), Vec::<Value>::new());
        assert_eq!(song.cursor(), at(0));
        assert!(!song.can_undo());
    }

    #[test]
    fn redo_makes_undone_edits_again_oldest_first() {
        let mut song = two_taps_placed();
        song.undo().unwrap();
        song.undo().unwrap();
        song.redo().unwrap();
        assert_eq!(objects(&song), [object(0, 0, "tap")]);
        assert_eq!(song.cursor(), at(0));
        song.redo().unwrap();
        assert_eq!(objects(&song), [object(0, 0, "tap"), object(240, 1, "tap")]);
        assert_eq!(song.cursor(), at(240));
        assert!(!song.can_redo());
    }

    #[test]
    fn a_new_edit_drops_what_could_have_been_redone() {
        let mut song = two_taps_placed();
        song.undo().unwrap();
        song.toggle(song.cursor(), lane(3), Kind::Tap).unwrap();
        assert!(!song.can_redo());
        song.redo().unwrap();
        assert_eq!(objects(&song), [object(0, 0, "tap"), object(240, 3, "tap")]);
    }

    #[test]
    fn undoing_a_removal_puts_the_note_back_where_it_was() {
        let three = json!([
            object(0, 0, "tap"),
            object(0, 1, "mine"),
            object(480, 2, "tap")
        ]);
        let mut song = Song::new(document(three.clone()), EDITABLE);
        song.toggle(song.cursor(), lane(1), Kind::Tap).unwrap();
        song.undo().unwrap();
        assert_eq!(Value::Array(objects(&song)), three);
    }

    #[test]
    fn undo_opens_the_chart_the_edit_was_made_in() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        song.toggle(song.cursor(), lane(0), Kind::Tap).unwrap();
        song.open_chart(1);
        song.undo().unwrap();
        assert_eq!(song.chart_index(), 0);
        assert_eq!(objects(&song), Vec::<Value>::new());
    }

    #[test]
    fn with_nothing_to_undo_or_redo_both_leave_the_song_alone() {
        let mut song = Song::new(document(json!([object(0, 0, "tap")])), EDITABLE);
        assert!(!song.can_undo() && !song.can_redo());
        song.undo().unwrap();
        song.redo().unwrap();
        assert_eq!(objects(&song), [object(0, 0, "tap")]);
    }

    #[test]
    fn a_song_is_unmodified_until_edited_and_again_once_undone() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        assert!(!song.is_modified());
        song.toggle(at(0), lane(0), Kind::Tap).unwrap();
        assert!(song.is_modified());
        song.undo().unwrap();
        assert!(!song.is_modified());
        song.redo().unwrap();
        assert!(song.is_modified());
    }

    #[test]
    fn saving_makes_the_song_as_it_stands_unmodified() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        song.toggle(at(0), lane(0), Kind::Tap).unwrap();
        song.mark_saved();
        assert!(!song.is_modified());
        song.undo().unwrap();
        assert!(song.is_modified());
    }

    #[test]
    fn a_saved_state_undone_and_edited_over_is_gone_for_good() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        song.toggle(at(0), lane(0), Kind::Tap).unwrap();
        song.mark_saved();
        song.undo().unwrap();
        song.toggle(at(0), lane(1), Kind::Tap).unwrap();
        song.undo().unwrap();
        assert!(song.is_modified());
    }

    #[test]
    fn stretching_a_note_placed_before_saving_modifies_the_song() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        song.toggle(at(0), lane(0), Kind::Tap).unwrap();
        song.mark_saved();
        song.stretch(lane(0), at(0), at(480)).unwrap();
        assert!(song.is_modified());
        song.undo().unwrap();
        assert!(song.is_modified());
    }

    #[test]
    fn the_revision_moves_with_every_change_to_the_notes_and_nothing_else() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        let mut seen = vec![song.revision()];
        let mut changed = |current_song: &Song| {
            let new = !seen.contains(&current_song.revision());
            seen.push(current_song.revision());
            new
        };
        song.toggle(at(0), lane(0), Kind::Tap).unwrap();
        assert!(changed(&song));
        song.stretch(lane(0), at(0), at(480)).unwrap();
        assert!(changed(&song));
        song.undo().unwrap();
        assert!(changed(&song));
        song.redo().unwrap();
        assert!(changed(&song));
        let before = song.revision();
        song.step(Step::Later);
        song.select_all();
        song.open_chart(1);
        assert_eq!(song.revision(), before);
    }

    #[test]
    fn a_song_marked_unsaved_is_modified_until_saved() {
        let mut song = Song::new(document(json!([])), EDITABLE);
        song.mark_unsaved();
        assert!(song.is_modified());
        song.mark_saved();
        assert!(!song.is_modified());
    }

    proptest! {
        #![proptest_config(crate::proptest_config())]

        #[test]
        fn random_edit_sequences_undo_to_the_original_and_redo_to_the_final(
            actions in prop::collection::vec(action(), 0..40)
        ) {
            let mut song = Song::new(document(json!([])), EDITABLE);
            let original = song.document().to_json().unwrap();
            for action in actions {
                action.apply(&mut song);
            }
            let final_document = song.document().to_json().unwrap();
            let final_step = song.history_step();
            let final_modified = song.is_modified();
            prop_assert_eq!(final_modified, final_step != 0);

            while song.can_undo() {
                song.undo().unwrap();
            }
            prop_assert_eq!(song.document().to_json().unwrap(), original);
            prop_assert!(!song.is_modified());
            prop_assert_eq!(song.history_step(), 0);

            while song.can_redo() {
                song.redo().unwrap();
            }
            prop_assert_eq!(song.document().to_json().unwrap(), final_document);
            prop_assert_eq!(song.history_step(), final_step);
            prop_assert_eq!(song.is_modified(), final_modified);
        }
    }
}
