//! Action dispatch from panels, keys and gestures.

use super::{
    Action, KeepNotes, Mode, Note, PASTE_WAIT, PendingPaste, Rate, Request, Saving, Selecting,
    Song, Step, Tick, Toggled, Window, grow, nudge_bpm, pattern, placing_at, refused, retime,
    stretch,
};

impl Window {
    /// Applies actions that affect the window state rather than the open song.
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "this dispatcher handles only window-global actions; song actions are ignored here"
    )]
    pub(super) fn apply_to_window(&mut self, action: &Action) {
        match *action {
            Action::ToggleLayerColours => {
                self.settings.colour_notes_by_layer = !self.settings.colour_notes_by_layer;
            }
            Action::OpenDemo => self.open_demo(),
            Action::Open => self.requests.push(Request::Open),
            #[expect(
                clippy::ref_patterns,
                reason = "every other arm copies its fields out of the dereferenced action"
            )]
            Action::OpenRecent(ref path) => {
                self.requests.push(Request::OpenPath { path: path.clone() });
            }
            Action::ClearRecent => self.settings.recent.clear(),
            Action::Save => self.saving = Some(Saving::InPlace),
            Action::SaveAs => self.saving = Some(Saving::Elsewhere),
            Action::Settings => self.settings_open = !self.settings_open,
            Action::Calibrate => self.calibration.set_open(true),
            Action::ToggleSync => self.sync.set_open(!self.sync.is_open()),
            Action::ShowSync => self.sync.set_open(true),
            Action::ShowImport => self.import.set_open(true),
            Action::ChooseTimingFile => self.requests.push(Request::ChooseTimingFile),
            Action::Help => self.help.shortcuts = !self.help.shortcuts,
            Action::About => self.help.about = !self.help.about,
            Action::ToggleSongPanel => self.settings.song_panel = !self.settings.song_panel,
            Action::ToggleMetronome => self.settings.metronome = !self.settings.metronome,
            Action::ToggleTicks => self.settings.note_ticks = !self.settings.note_ticks,
            Action::ToggleTunedLanes => self.settings.tuned_lanes = !self.settings.tuned_lanes,
            Action::SetGrid(grid) => self.settings.grid = grid,
            Action::TogglePlay => self.toggle_play(),
            Action::Audition(from) => self.audition(from),
            Action::PlaySnippet(side) => self.play_snippet(side),
            Action::AskResetTiming => self.dialogs.resetting = true,
            Action::FindTiming(tracker) => self.find_timing(tracker),
            Action::ImportSong => self.ask_import(),
            Action::ExportSong => self.ask_export(),
            Action::Loop { from, to } => self.toggle_loop(from, to),
            Action::ToggleRate => self.toggle_rate(),
            Action::MuteMetronome(muted) => {
                self.playback.muted = muted;
                self.replay();
            }
            Action::ChooseAudio => self.requests.push(Request::ChooseAudio),
            Action::ChooseArtwork => self.requests.push(Request::ChooseArtwork),
            Action::NudgeRate(step) => self.set_rate(self.playback.rate.by(step)),
            Action::SetRate(factor) => self.set_rate(Rate::of(factor)),
            Action::SetVolumes(volumes) => self.settings.volumes = volumes,
            Action::NewSong => {
                self.dialogs.new_song.get_or_insert_default();
            }
            _ => {}
        }
    }
    /// Applies one action, closing any open lane-key chord first when needed.
    pub(super) fn apply(&mut self, action: Action) {
        // A chord's notes undo as one, but anything else done while it is held is its own step.
        let chord = matches!(
            action,
            Action::Toggle(..) | Action::LaneUp(_) | Action::LanesReleased
        );
        if !chord
            && let Some(step) = self.typing.chord_since.take()
            && let Some(song) = &mut self.song
        {
            song.merge_since(step);
        }
        if let Action::Here(tick, action) = action {
            // Done on the line the menu was opened on, and the cursor left where it was.
            let was = self.song.as_ref().map(Song::cursor);
            self.apply(Action::JumpTo(tick));
            self.apply(*action);
            if let Some(was) = was {
                self.apply(Action::JumpTo(was));
            }
            return;
        }
        if let Action::PasteHere(tick) = action {
            // The host answers with the clipboard's text as a paste, or nothing without any.
            self.clipboard.pasting = Some(PendingPaste {
                tick,
                wait: PASTE_WAIT,
            });
            self.clipboard.request_paste = true;
            return;
        }
        if let Action::Paste(Some(_)) = &action
            && let Some(PendingPaste { tick, .. }) = self.clipboard.pasting.take()
        {
            self.apply(Action::Here(tick, Box::new(action)));
            return;
        }
        self.apply_to_window(&action);
        let input_latency = self.input_latency();
        let Some(song) = &mut self.song else {
            return;
        };
        song.keep_notes(if self.settings.notes_keep_time {
            KeepNotes::InTime
        } else {
            KeepNotes::OnBeats
        });
        let _: bool = self.apply_layer_action(&action)
            || self.apply_cursor_action(&action, input_latency)
            || self.apply_timing_action(&action)
            || self.apply_marker_action(&action)
            || self.apply_selection_action(&action);
    }

    /// Applies a layer-stack action, returning whether it matched.
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "this helper handles only layer actions and lets the next helper try the rest"
    )]
    fn apply_layer_action(&mut self, action: &Action) -> bool {
        let Some(song) = &mut self.song else {
            return false;
        };
        match action {
            Action::AddLayer => self.failure = song.add_layer().err().as_ref().map(refused),
            Action::SetActiveLayer(index) => song.set_active_layer(*index),
            Action::StepLayer(step) => {
                let active = song.active_layer();
                song.set_active_layer(match step {
                    Step::Earlier => active.saturating_sub(1),
                    Step::Later => active.saturating_add(1),
                });
            }
            Action::MoveSelectionToLayer(index) => {
                self.failure = song
                    .move_selection_to_layer(*index)
                    .err()
                    .as_ref()
                    .map(refused);
            }
            Action::SelectLayer(index) => song.select_layer(*index),
            Action::SetLayerView(index, view) => {
                self.failure = song
                    .set_layer_view(*index, *view)
                    .err()
                    .as_ref()
                    .map(refused);
            }
            Action::SoloLayer(index) => {
                self.failure = song.solo_layer(*index).err().as_ref().map(refused);
            }
            Action::SetLayerLocked(index, locked) => {
                self.failure = song
                    .set_layer_locked(*index, *locked)
                    .err()
                    .as_ref()
                    .map(refused);
            }
            Action::RecolourLayer(index, colour) => {
                self.failure = song
                    .recolour_layer(*index, *colour)
                    .err()
                    .as_ref()
                    .map(refused);
            }
            Action::RenameLayer(index, name) => {
                self.failure = song.rename_layer(*index, name).err().as_ref().map(refused);
            }
            Action::MoveLayer { from, to } => {
                self.failure = song.move_layer(*from, *to).err().as_ref().map(refused);
            }
            Action::MergeActiveLayer => {
                self.failure = song
                    .merge_layer_down(song.active_layer())
                    .err()
                    .as_ref()
                    .map(refused);
            }
            Action::MergeLayer(index) => {
                self.failure = song.merge_layer_down(*index).err().as_ref().map(refused);
            }
            Action::DeleteLayer(index, notes) => {
                self.failure = song
                    .delete_layer(*index, *notes)
                    .err()
                    .as_ref()
                    .map(refused);
            }
            Action::CopyLayerToChart { layer, chart } => {
                self.failure = match song.copy_layer_to_chart(*layer, *chart) {
                    Ok(0) => None,
                    Ok(skipped) => Some(format!("{skipped} overlapping notes skipped")),
                    Err(why) => Some(refused(&why)),
                };
            }
            Action::ToggleActiveLayerView => {
                let index = song.active_layer();
                if let Some(row) = song.layers().get(index) {
                    let view = if row.view == charter_core::LayerView::Hidden {
                        charter_core::LayerView::Shown
                    } else {
                        charter_core::LayerView::Hidden
                    };
                    self.failure = song.set_layer_view(index, view).err().as_ref().map(refused);
                }
            }
            Action::OpenChart(index) => {
                song.open_chart(*index);
                self.typing.holding.clear();
                self.typing.pressed.clear();
            }
            _ => return false,
        }
        true
    }

    /// Applies cursor, snap, lane-key and direct-placement actions.
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "this helper handles only cursor and lane-key actions"
    )]
    fn apply_cursor_action(&mut self, action: &Action, input_latency: f64) -> bool {
        let Some(song) = &mut self.song else {
            return false;
        };
        match action {
            Action::Move(step) => {
                song.step(*step);
                self.failure = stretch(song, &self.typing.holding).or_else(|| self.failure.take());
            }
            Action::MoveMeasure(step) => {
                song.step_measure(*step);
                self.failure = stretch(song, &self.typing.holding).or_else(|| self.failure.take());
            }
            Action::FinerSnap => song.finer_snap(),
            Action::CoarserSnap => song.coarser_snap(),
            Action::ZoomBy(factor) => self.zoom = self.zoom.by(*factor),
            Action::Toggle(lane, note) => {
                // Mines and removals are not held, so a chord starts once, with its first key.
                if self.typing.chord_since.is_none() {
                    self.typing.chord_since = Some(song.history_step());
                }
                let at = placing_at(
                    song,
                    self.playback.playing.is_some().then_some(input_latency),
                );
                let toggled = song.toggle(at, *lane, note.kind());
                if matches!(toggled, Ok(Toggled::Placed)) {
                    self.typing.placed = true;
                    if *note == Note::Tap {
                        self.typing.holding.push((*lane, at));
                        if self.playback.playing.is_some() {
                            let now = song.timing().time_at(song.cursor()).seconds();
                            self.typing.pressed.push((*lane, now - input_latency));
                        }
                    }
                }
                self.failure = toggled.err().as_ref().map(refused);
            }
            Action::LaneUp(lane) => {
                if self.playback.playing.is_some() {
                    // Where the key came up is where the hold ends.
                    let now = song.timing().time_at(song.cursor()).seconds() - input_latency;
                    let held: Vec<_> = self
                        .typing
                        .holding
                        .iter()
                        .copied()
                        .filter(|(held, _)| held == lane)
                        .collect();
                    self.failure = grow(song, &held, &self.typing.pressed, now)
                        .or_else(|| self.failure.take());
                }
                self.typing.holding.retain(|(held, _)| held != lane);
                self.typing.pressed.retain(|(held, _)| held != lane);
            }
            // Playback moves the cursor on already; a jump would seek the audio ahead.
            Action::LanesReleased => {
                // A chord, and the holds it grew while playing, undo as one.
                if let Some(step) = self.typing.chord_since.take() {
                    song.merge_since(step);
                }
                if std::mem::take(&mut self.typing.placed)
                    && self.settings.auto_jump
                    && self.playback.playing.is_none()
                {
                    song.step(Step::Later);
                }
            }
            Action::Undo => {
                self.typing.placed = false;
                self.typing.holding.clear();
                self.typing.pressed.clear();
                self.failure = song.undo().err().as_ref().map(refused);
            }
            Action::Redo => {
                self.typing.placed = false;
                self.typing.holding.clear();
                self.typing.pressed.clear();
                self.failure = song.redo().err().as_ref().map(refused);
            }
            Action::Place { tick, lane, note } => {
                self.failure = song
                    .toggle(*tick, *lane, note.kind())
                    .err()
                    .as_ref()
                    .map(refused);
            }
            Action::Stretch { lane, from, to } => {
                self.failure = song.stretch(*lane, *from, *to).err().as_ref().map(refused);
            }
            Action::Select(Some(note), how) => song.select([*note], *how),
            Action::Select(None, Selecting::Replace) | Action::ClearSelection => {
                song.clear_selection();
            }
            Action::Select(None, Selecting::Add | Selecting::Subtract) => {}
            _ => return false,
        }
        true
    }

    /// Applies timing-window and cursor-jump actions.
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "this helper handles only timing and jump actions"
    )]
    fn apply_timing_action(&mut self, action: &Action) -> bool {
        let Some(song) = &mut self.song else {
            return false;
        };
        match action {
            Action::SelectBetween {
                from_tick,
                to_tick,
                from_lane,
                to_lane,
                how,
            } => {
                song.select_between(*from_tick..=*to_tick, *from_lane..=*to_lane, *how);
            }
            Action::SetBpmAt(..)
            | Action::MovePointTo(..)
            | Action::SetMetadata(_)
            | Action::SetSetting { .. }
            | Action::SetMeterAt(..)
            | Action::RemoveTimingAt(_)
            | Action::SetOffset(_)
            | Action::AddBpm
            | Action::NudgeOffset(_)
            | Action::ImportTiming(_)
            | Action::ResetTiming
            | Action::MoveTiming { .. } => self.failure = retime(song, action),
            // The timing window says itself why its edits were refused.
            Action::Timing(edit) => match edit.apply(song) {
                Err(why) => {
                    let now = self.frame_time;
                    self.sync.refused(edit, &why, song.timing(), now);
                }
                // A loop plays again from its start, so the edit is heard on this pass.
                Ok(()) => {
                    if let Some(Mode::Loop(from, to)) = self.playback.playing {
                        self.play_loop(from, to);
                    }
                }
            },
            Action::JumpTo(tick) => song.jump_to(*tick),
            Action::JumpEnd => song.jump_to(song.end()),
            _ => return false,
        }
        true
    }

    /// Applies marker and section-list actions.
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "this helper handles only marker actions"
    )]
    fn apply_marker_action(&mut self, action: &Action) -> bool {
        let Some(song) = &mut self.song else {
            return false;
        };
        match action {
            Action::MoveMarker { index, to } => {
                self.failure = song.move_marker(*index, *to).err().as_ref().map(refused);
            }
            Action::JumpSection(step) => {
                if let Some(tick) = song.section_from(*step) {
                    song.jump_to(tick);
                }
            }
            Action::ToggleBookmark => {
                self.failure = song.toggle_bookmark().err().as_ref().map(refused);
            }
            Action::NudgeBpm(by) => self.failure = nudge_bpm(song, *by),
            Action::SetSection(text) => {
                self.failure = song.set_section(text).err().as_ref().map(refused);
            }
            Action::RemoveMarker(index) => {
                self.failure = song.remove_marker(*index).err().as_ref().map(refused);
            }
            _ => return false,
        }
        true
    }

    /// Applies selection, clipboard and note-transform actions.
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "this helper handles only selection actions"
    )]
    fn apply_selection_action(&mut self, action: &Action) -> bool {
        let Some(song) = &mut self.song else {
            return false;
        };
        match action {
            Action::SelectAll => song.select_all(),
            Action::DeleteSelection => {
                self.failure = song.delete_selection().err().as_ref().map(refused);
            }
            Action::Copy => self.copy(),
            Action::Cut => {
                self.copy();
                if let Some(open_song) = &mut self.song {
                    self.failure = open_song.delete_selection().err().as_ref().map(refused);
                }
            }
            Action::Paste(text) => self.paste(text.as_deref()),
            Action::Duplicate => self.duplicate(),
            Action::ShiftSelection { snaps, lanes } => {
                let step = song
                    .document()
                    .document()
                    .ticks_per_beat
                    .note_length(song.snap())
                    .map_or(0, |step| i64::from(u32::from(step)));
                let ticks =
                    i32::try_from(i64::from(*snaps).saturating_mul(step)).unwrap_or(i32::MAX);
                self.failure = song
                    .move_selection(ticks, *lanes)
                    .err()
                    .as_ref()
                    .map(refused);
            }
            Action::Mirror => self.failure = song.mirror_selection().err().as_ref().map(refused),
            Action::Quantize => {
                self.failure = song.quantize_selection().err().as_ref().map(refused);
            }
            Action::ChangeNotes(change) => {
                self.failure = song.change_selection(*change).err().as_ref().map(refused);
            }
            Action::Pattern {
                steps,
                per_beat,
                lane,
            } => self.failure = pattern(song, steps, *per_beat, *lane),
            Action::SelectLane(lane) => {
                song.select_between(
                    Tick::new(i32::MIN)..=Tick::new(i32::MAX),
                    *lane..=*lane,
                    Selecting::Replace,
                );
            }
            Action::ResizeNotes(step) => {
                self.failure = song.resize_notes(*step).err().as_ref().map(refused);
            }
            Action::MoveSelection { ticks, lanes } => {
                self.failure = song
                    .move_selection(*ticks, *lanes)
                    .err()
                    .as_ref()
                    .map(refused);
            }
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::window::test_support::*;

    #[test]
    fn alt_digits_switch_layers_and_move_notes_without_placing_lane_taps() {
        let mut harness = editable_four_taps();
        harness.state_mut().apply(Action::AddLayer);
        let before = note_count(&harness);
        harness.key_press_modifiers(egui::Modifiers::ALT, egui::Key::Num2);
        harness.run();
        assert_eq!(harness.state().song.as_ref().unwrap().active_layer(), 1);
        assert!(harness.query_by_label("Layer 0").is_some());
        select_note(&mut harness, 3, 720);
        harness.key_press_modifiers(
            egui::Modifiers::ALT.plus(egui::Modifiers::SHIFT),
            egui::Key::Num1,
        );
        harness.run();
        let song = harness.state().song.as_ref().unwrap();
        assert_eq!(song.layers()[0].notes, 1);
        assert_eq!(note_count(&harness), before);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(harness.state().song.as_ref().unwrap().layers()[0].notes, 0);
    }
    #[test]
    fn view_menu_toggles_the_persisted_layer_tint_setting() {
        let mut harness = editable_demo();
        assert!(!harness.state().settings.colour_notes_by_layer);
        harness.get_by_label("View").click();
        harness.run();
        harness.get_by_label("Colour notes by layer").click();
        harness.run();
        let settings = &harness.state().settings;
        assert!(settings.colour_notes_by_layer);
        assert!(Settings::from_kept(&settings.to_kept()).colour_notes_by_layer);
    }
    #[test]
    fn hidden_or_locked_layers_contribute_no_notes_to_select_all() {
        for action in [
            Action::SetLayerView(1, charter_core::LayerView::Hidden),
            Action::SetLayerLocked(1, true),
        ] {
            let mut harness = editable_four_taps();
            harness.state_mut().apply(Action::AddLayer);
            harness.state_mut().apply(action);
            harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
            harness.run();
            assert_eq!(
                harness.state().song.as_ref().unwrap().selection().count(),
                0
            );
        }
    }
    #[test]
    fn a_lane_key_places_a_tap_at_the_cursor_and_moves_on_a_snap() {
        let mut harness = editable_demo();
        harness.key_press(egui::Key::Num1);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [0, 3]);
        assert!(cursor_shows(&harness, "0:0:240"));
    }
    #[test]
    fn lane_keys_held_together_place_a_chord_and_move_on_once_all_are_up() {
        let mut harness = editable_demo();
        harness.key_down(egui::Key::Num1);
        harness.key_down(egui::Key::Num2);
        harness.run();
        harness.key_up(egui::Key::Num1);
        harness.run();
        assert!(cursor_shows(&harness, "0:0:0"));
        harness.key_up(egui::Key::Num2);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [0, 1, 3]);
        assert!(cursor_shows(&harness, "0:0:240"));
    }
    #[test]
    fn a_lane_key_tapped_twice_in_one_frame_places_two_notes_down_the_lane() {
        let mut harness = editable_demo();
        tap_in_one_frame(&mut harness, &[egui::Key::Num1, egui::Key::Num1]);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [0, 3]);
        assert_eq!(lanes_at(&harness, 240), [0]);
        assert!(cursor_shows(&harness, "0:0:480"));
    }
    #[test]
    fn two_lane_keys_tapped_in_one_frame_make_a_stream_not_a_chord() {
        let mut harness = editable_demo();
        tap_in_one_frame(&mut harness, &[egui::Key::Num1, egui::Key::Num2]);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [0, 3]);
        assert_eq!(lanes_at(&harness, 240), [1]);
    }
    #[test]
    fn keys_in_one_frame_act_in_the_order_they_were_pressed() {
        let mut harness = editable_demo();
        tap_in_one_frame(&mut harness, &[egui::Key::Num1, egui::Key::ArrowDown]);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [0, 3]);
        assert!(cursor_shows(&harness, "0:0:480"));
    }
    #[test]
    fn a_lane_key_released_under_ctrl_still_moves_the_cursor_on() {
        let mut harness = editable_demo();
        harness.key_down(egui::Key::Num1);
        harness.run();
        harness.event(egui::Event::ModifiersChanged(egui::Modifiers::COMMAND));
        harness.event(egui::Event::Key {
            key: egui::Key::Num1,
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
            physical_key: None,
        });
        harness.run();
        assert!(cursor_shows(&harness, "0:0:240"));
    }
    #[test]
    fn a_lane_key_released_under_shift_still_moves_the_cursor_on() {
        let mut harness = editable_demo();
        let shift = egui::Modifiers::SHIFT;
        harness.event(keyboard(
            egui::Key::Num1,
            egui::Key::Num1,
            true,
            egui::Modifiers::NONE,
        ));
        harness.run();
        harness.event(egui::Event::ModifiersChanged(shift));
        harness.event(keyboard(
            egui::Key::Exclamationmark,
            egui::Key::Num1,
            false,
            shift,
        ));
        harness.run();
        assert!(cursor_shows(&harness, "0:0:240"));
    }
    #[test]
    fn a_lane_key_held_through_shift_coming_up_repeats_without_toggling() {
        let mut harness = editable_demo();
        shift_down(&harness, egui::Key::Exclamationmark, egui::Key::Num1);
        harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        let none = egui::Modifiers::NONE;
        harness.event(keyboard(egui::Key::Num1, egui::Key::Num1, true, none));
        harness.run();
        assert_eq!(note_at(&harness, 0, 0), Some(mine()));
    }
    #[test]
    fn a_numpad_arrow_moves_the_cursor_rather_than_placing_a_note() {
        let mut harness = editable_demo();
        let none = egui::Modifiers::NONE;
        harness.event(keyboard(egui::Key::ArrowDown, egui::Key::Num2, true, none));
        harness.event(keyboard(egui::Key::ArrowDown, egui::Key::Num2, false, none));
        harness.run();
        assert!(cursor_shows(&harness, "0:0:240"));
        assert_eq!(lanes_at(&harness, 0), [3]);
    }
    #[test]
    fn a_lane_key_released_while_a_widget_has_focus_still_moves_the_cursor_on() {
        let mut harness = editable_demo();
        harness.key_down(egui::Key::Num1);
        harness.run();
        harness.key_press(egui::Key::Tab);
        harness.run();
        assert!(harness.ctx.egui_wants_keyboard_input());
        harness.key_up(egui::Key::Num1);
        harness.run();
        assert!(cursor_shows(&harness, "0:0:240"));
    }
    #[test]
    fn losing_focus_with_a_lane_key_down_moves_on_as_if_it_came_up() {
        let mut harness = editable_demo();
        harness.key_down(egui::Key::Num1);
        harness.run();
        harness.event(egui::Event::WindowFocused(false));
        harness.run();
        assert!(cursor_shows(&harness, "0:0:240"));
        harness.event(egui::Event::WindowFocused(true));
        harness.key_press(egui::Key::Num1);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [0, 3]);
        assert_eq!(lanes_at(&harness, 240), [0]);
    }
    #[test]
    fn undoing_a_note_whose_key_is_still_down_keeps_the_cursor_when_it_comes_up() {
        let mut harness = editable_demo();
        harness.key_down(egui::Key::Num1);
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        harness.key_up(egui::Key::Num1);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [3]);
        assert!(cursor_shows(&harness, "0:0:0"));
    }
    #[test]
    fn shift_and_a_lane_key_place_a_mine_and_move_on() {
        let mut harness = editable_demo();
        shift_down(&harness, egui::Key::Exclamationmark, egui::Key::Num1);
        harness.event(keyboard(
            egui::Key::Exclamationmark,
            egui::Key::Num1,
            false,
            egui::Modifiers::SHIFT,
        ));
        harness.run();
        assert_eq!(note_at(&harness, 0, 0), Some(mine()));
        assert!(cursor_shows(&harness, "0:0:240"));
    }
    #[test]
    fn holding_a_lane_key_while_the_cursor_moves_draws_a_hold() {
        let mut harness = editable_demo();
        harness.key_down(egui::Key::Num1);
        harness.key_press(egui::Key::ArrowDown);
        harness.key_press(egui::Key::ArrowDown);
        harness.run();
        assert_eq!(note_at(&harness, 0, 0), Some(hold(480)));
        harness.key_up(egui::Key::Num1);
        harness.run();
        assert!(cursor_shows(&harness, "0:0:720"));
    }
    #[test]
    fn a_hold_drawn_with_a_lane_key_undoes_in_one_step() {
        let mut harness = editable_demo();
        harness.key_down(egui::Key::Num1);
        harness.key_press(egui::Key::ArrowDown);
        harness.key_up(egui::Key::Num1);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(note_at(&harness, 0, 0), None);
        assert!(cursor_shows(&harness, "0:0:0"));
    }
    #[test]
    fn moving_back_to_where_the_key_went_down_makes_the_hold_a_tap_again() {
        let mut harness = editable_demo();
        harness.key_down(egui::Key::Num1);
        harness.key_press(egui::Key::ArrowDown);
        harness.key_press(egui::Key::ArrowUp);
        harness.run();
        assert_eq!(note_at(&harness, 0, 0), Some(tap()));
    }
    #[test]
    fn a_mine_does_not_stretch() {
        let mut harness = editable_demo();
        shift_down(&harness, egui::Key::Exclamationmark, egui::Key::Num1);
        harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        harness.key_press(egui::Key::ArrowDown);
        harness.run();
        assert_eq!(note_at(&harness, 0, 0), Some(mine()));
    }
    #[test]
    fn ctrl_a_selects_the_chart_and_delete_removes_it_in_one_step() {
        let mut harness = editable_four_taps();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.run();
        assert!(harness.query_by_label("4 notes selected").is_some());
        harness.key_press(egui::Key::Delete);
        harness.run();
        assert_eq!(note_count(&harness), 0);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(note_count(&harness), 4);
    }
    #[test]
    fn the_status_bar_counts_the_selection() {
        let mut harness = editable_four_taps();
        assert!(nothing_selected(&harness));
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.run();
        assert!(harness.query_by_label("4 notes selected").is_some());
    }
    #[test]
    fn escape_clears_the_selection() {
        let mut harness = editable_four_taps();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.key_press(egui::Key::Escape);
        harness.run();
        assert!(nothing_selected(&harness));
        harness.key_press(egui::Key::Backspace);
        harness.run();
        assert_eq!(note_count(&harness), 4);
    }
    #[test]
    fn ctrl_d_keeps_the_source_layer_when_another_is_active() {
        let mut harness = editable_four_taps();
        harness.state_mut().apply(Action::AddLayer);
        select_note(&mut harness, 3, 720);
        let source = harness
            .state()
            .song
            .as_ref()
            .unwrap()
            .chart()
            .unwrap()
            .objects
            .iter()
            .find(|o| o.tick == Tick::new(720))
            .unwrap()
            .layer
            .clone();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::D);
        harness.run();
        let song = harness.state().song.as_ref().unwrap();
        assert!(
            song.chart()
                .unwrap()
                .objects
                .iter()
                .any(|o| o.tick == Tick::new(960) && o.layer == source)
        );
    }
    #[test]
    fn dragging_a_note_with_shift_changes_its_lane_and_keeps_its_time() {
        let mut harness = editable_four_taps();
        let from = on_line(&harness, lane_x(&harness, 0), 0);
        let to = on_line(&harness, lane_x(&harness, 2), 480);
        drag(&mut harness, from, to, egui::Modifiers::SHIFT);
        assert_eq!(lanes_at(&harness, 0), [2]);
        assert_eq!(lanes_at(&harness, 480), [2]);
    }
    #[test]
    fn a_middle_drag_pans_the_chart_and_shift_wheel_moves_by_measure() {
        let mut harness = editable_four_taps();
        let from = on_line(&harness, lane_x(&harness, 1), 960);
        let to = on_line(&harness, lane_x(&harness, 1), 0);
        let middle = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Middle,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        harness.hover_at(from);
        harness.event(middle(from, true));
        for step in 1..=4 {
            harness
                .hover_at(from.lerp(to, f32::from(u16::try_from(step).unwrap_or(u16::MAX)) / 4.0));
        }
        harness.event(middle(to, false));
        harness.run();
        let cursor = harness.state().song.as_ref().unwrap().cursor();
        assert_eq!(cursor, Tick::new(960), "dragging the chart up goes on");
        assert_eq!(note_count(&harness), 4);
        harness.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, -1.0),
            modifiers: egui::Modifiers::SHIFT,
            phase: egui::TouchPhase::Move,
        });
        harness.run();
        let moved_cursor = harness.state().song.as_ref().unwrap().cursor();
        assert_eq!(moved_cursor, Tick::new(3840));
    }
    #[test]
    fn right_clicking_an_already_selected_note_keeps_that_selection() {
        let mut harness = editable_four_taps();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.run();
        assert_eq!(
            harness.state().song.as_ref().unwrap().selection().count(),
            4
        );
        let pos = on_line(&harness, lane_x(&harness, 2), 480);
        harness.hover_at(pos);
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
        harness.run();
        assert!(harness.query_by_label("4 notes selected").is_some());
        let selected = harness.state().song.as_ref().unwrap().selection().count();
        assert_eq!(selected, 4);
    }
    #[test]
    fn ctrl_clicking_a_lane_places_a_tap_on_the_nearest_snap_line() {
        let mut harness = editable_four_taps();
        let pos = spot(&harness, 3, 240, 4.0);
        click(&mut harness, pos, egui::Modifiers::COMMAND);
        assert_eq!(note_at(&harness, 240, 3), Some(tap()));
    }
    #[test]
    fn ctrl_clicking_a_lane_number_does_not_place_a_hidden_note() {
        let mut harness = editable_four_taps();
        let x = spot(&harness, 0, 0, 0.0).x;
        let top = harness.get_by_label("Notefield").rect().top();
        click(
            &mut harness,
            egui::pos2(x, top + 13.0),
            egui::Modifiers::COMMAND,
        );

        assert_eq!(note_count(&harness), 4);
        assert!(!harness.state().song.as_ref().unwrap().is_modified());
    }
    #[test]
    fn ctrl_shift_clicking_places_a_mine() {
        let mut harness = editable_four_taps();
        let pos = spot(&harness, 1, 480, 0.0);
        click(
            &mut harness,
            pos,
            egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT),
        );
        assert_eq!(note_at(&harness, 480, 1), Some(mine()));
    }
    #[test]
    fn ctrl_dragging_down_a_lane_draws_a_hold_that_undoes_in_one_step() {
        let mut harness = editable_four_taps();
        let (from, to) = (spot(&harness, 3, 240, 0.0), spot(&harness, 3, 720, -3.0));
        drag(&mut harness, from, to, egui::Modifiers::COMMAND);
        assert_eq!(note_at(&harness, 240, 3), Some(hold(480)));
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(note_at(&harness, 240, 3), None);
    }
    #[test]
    fn clicking_notes_selects_them_as_shift_adds_and_alt_takes_away() {
        let mut harness = editable_four_taps();
        let (first, second) = (spot(&harness, 0, 0, 0.0), spot(&harness, 1, 240, 2.0));
        click(&mut harness, first, egui::Modifiers::NONE);
        assert!(harness.query_by_label("1 note selected").is_some());
        click(&mut harness, second, egui::Modifiers::SHIFT);
        assert!(harness.query_by_label("2 notes selected").is_some());
        click(&mut harness, first, egui::Modifiers::ALT);
        assert!(harness.query_by_label("1 note selected").is_some());
        let empty = spot(&harness, 3, 0, 0.0);
        click(&mut harness, empty, egui::Modifiers::NONE);
        assert!(nothing_selected(&harness));
    }
    #[test]
    fn dragging_a_note_moves_it_along_the_snap_and_across_lanes_in_one_step() {
        let mut harness = editable_four_taps();
        let (from, to) = (spot(&harness, 0, 0, 0.0), spot(&harness, 1, 480, 3.0));
        drag(&mut harness, from, to, egui::Modifiers::NONE);
        assert_eq!(note_at(&harness, 0, 0), None);
        assert_eq!(note_at(&harness, 480, 1), Some(tap()));
        assert_eq!(note_count(&harness), 4);
        assert!(harness.query_by_label("1 note selected").is_some());
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(note_at(&harness, 0, 0), Some(tap()));
        assert_eq!(note_at(&harness, 480, 1), None);
    }
    #[test]
    fn dragging_a_selected_note_moves_the_selection_and_keeps_it_on_the_lanes() {
        let mut harness = editable_four_taps();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.run();
        let (from, to) = (spot(&harness, 1, 240, 0.0), spot(&harness, 2, 480, 0.0));
        drag(&mut harness, from, to, egui::Modifiers::NONE);
        for (tick, lane) in [(240, 0), (480, 1), (720, 2), (960, 3)] {
            assert_eq!(note_at(&harness, tick, lane), Some(tap()), "{tick} {lane}");
        }
        assert!(harness.query_by_label("4 notes selected").is_some());
    }
    #[test]
    fn a_button_tab_focused_leaves_the_keys_to_the_editor() {
        let mut harness = editable_demo();
        harness.key_press(egui::Key::Tab);
        harness.run();
        assert!(harness.ctx.memory(egui::Memory::focused).is_some());
        let before = harness.state().song.as_ref().unwrap().cursor();
        harness.key_press(egui::Key::ArrowDown);
        harness.run();
        assert!(harness.state().song.as_ref().unwrap().cursor() > before);
    }
    #[test]
    fn ctrl_clicking_places_a_tap_however_the_notefield_runs() {
        for direction in Direction::ALL {
            let mut harness = turned_demo(direction);
            let at = turned_spot(&harness, 1, 240);
            click(&mut harness, at, egui::Modifiers::COMMAND);
            assert_eq!(lanes_at(&harness, 240), [1], "{direction:?}");
        }
    }
    #[test]
    fn clicking_a_note_selects_it_however_the_notefield_runs() {
        for direction in [Direction::Down, Direction::Right] {
            let mut harness = turned_demo(direction);
            let at = turned_spot(&harness, 3, 0);
            click(&mut harness, at, egui::Modifiers::NONE);
            let song = harness.state().song.as_ref().unwrap();
            assert_eq!(song.selection().count(), 1, "{direction:?}");
        }
    }
    #[test]
    fn a_marker_can_be_removed_from_the_list() {
        let mut harness = editable_demo();
        let remove = "Remove marker Check the stream";
        harness.get_by_label(remove).scroll_to_me();
        harness.run();
        harness.get_by_label(remove).click();
        harness.run();
        assert!(!marker_texts(&harness).contains(&"Check the stream".to_owned()));
        assert!(harness.query_by_label("Check the stream").is_none());
    }
    #[test]
    fn home_and_end_jump_to_the_start_and_to_where_the_last_note_ends() {
        let mut harness = editable_demo();
        harness.key_press(egui::Key::End);
        harness.run();
        let song = harness.state().song.as_ref().unwrap();
        assert_eq!(song.cursor(), song.end());
        assert_ne!(song.cursor(), Tick::new(0));
        harness.key_press(egui::Key::Home);
        harness.run();
        assert!(cursor_shows(&harness, "0:0:0"));
    }
    #[test]
    fn ctrl_b_toggles_a_bookmark_at_the_cursor() {
        let mut harness = editable_demo();
        let before = marker_texts(&harness).len();
        harness.key_press(egui::Key::ArrowDown);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::B);
        harness.run();
        assert_eq!(marker_texts(&harness).len(), before + 1);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::B);
        harness.run();
        assert_eq!(marker_texts(&harness).len(), before);
    }
    #[test]
    fn a_chord_of_mines_undoes_in_one_step() {
        let mut harness = editable_four_taps();
        harness.key_press(egui::Key::PageDown);
        harness.run();
        harness.event(egui::Event::ModifiersChanged(egui::Modifiers::SHIFT));
        for key in [egui::Key::Num1, egui::Key::Num2] {
            harness.event(egui::Event::Key {
                key,
                physical_key: Some(key),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::SHIFT,
            });
        }
        harness.run();
        for key in [egui::Key::Num1, egui::Key::Num2] {
            harness.event(egui::Event::Key {
                key,
                physical_key: Some(key),
                pressed: false,
                repeat: false,
                modifiers: egui::Modifiers::SHIFT,
            });
        }
        harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        harness.run();
        assert_eq!(note_count(&harness), 6);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(note_count(&harness), 4);
    }
    #[test]
    fn the_toolbar_says_when_the_grid_draws_the_snap_s_lines_too() {
        let mut harness = editable_four_taps();
        assert!(harness.query_by_label("+ snap").is_some());
        for _ in 0..8 {
            harness.key_press(egui::Key::ArrowLeft);
        }
        harness.run();
        assert!(harness.query_by_label("+ snap").is_none());
    }
    #[test]
    fn the_toolbar_draws_grid_lines_between_the_beats() {
        let mut harness = editable_demo();
        harness
            .get_by_role_and_label(Role::ComboBox, "Grid lines")
            .click();
        harness.run();
        harness.get_by_label("16th notes").click();
        harness.run();
        assert_eq!(harness.state().settings().grid.get(), 16);
        let grid = harness.get_by_role_and_label(Role::ComboBox, "Grid lines");
        assert_eq!(grid.value().as_deref(), Some("16th notes"));
    }
    #[test]
    fn ctrl_dragging_from_a_note_stretches_it_rather_than_removing_it() {
        let mut harness = editable_four_taps();
        let (from, to) = (spot(&harness, 1, 240, 0.0), spot(&harness, 1, 720, -3.0));
        drag(&mut harness, from, to, egui::Modifiers::COMMAND);
        assert_eq!(note_at(&harness, 240, 1), Some(hold(480)));
    }
    #[test]
    fn a_box_and_a_dragged_hold_work_however_the_notefield_runs() {
        for direction in Direction::ALL {
            let mut harness = turned_demo(direction);
            let (from, to) = (turned_spot(&harness, 0, 480), turned_spot(&harness, 0, 960));
            drag(&mut harness, from, to, egui::Modifiers::COMMAND);
            assert_eq!(note_at(&harness, 480, 0), Some(hold(480)), "{direction:?}");
            let corner = |lane, tick| turned_spot(&harness, lane, tick);
            let (a, b) = (corner(0, 470), corner(3, 490));
            let (a, b) = (
                a + (a - b).normalized() * 30.0,
                b + (b - a).normalized() * 30.0,
            );
            drag(&mut harness, a, b, egui::Modifiers::NONE);
            let song = harness.state().song.as_ref().unwrap();
            assert!(song.selection().count() >= 1, "{direction:?}");
        }
    }
    #[test]
    fn the_edit_menu_selects_and_deletes() {
        let mut harness = editable_four_taps();
        harness.get_by_label("Edit").click();
        harness.run();
        harness.get_by_label_contains("Select all").click();
        harness.run();
        harness.get_by_label("Edit").click();
        harness.run();
        harness
            .get_by_label_contains("Delete the selection")
            .click();
        harness.run();
        assert_eq!(note_count(&harness), 0);
    }
    #[test]
    fn dragging_a_marker_in_the_ruler_moves_it() {
        let mut harness = timed_song();
        let x = field(&harness).columns.ruler_x();
        let (from, to) = (on_line(&harness, x, 480), on_line(&harness, x, 1440));
        drag(&mut harness, from, to, egui::Modifiers::NONE);
        let song = harness.state().song.as_ref().unwrap();
        assert_eq!(song.document().document().markers[0].tick, Tick::new(1440));
    }
    #[test]
    fn dragging_the_cursor_handle_scrubs_through_the_song() {
        let mut harness = timed_song();
        let field = field(&harness);
        let from = egui::pos2(field.columns.handle_x(), field.view.now_y);
        let beat = field.view.y_of(0.5) - field.view.y_of(0.0);
        drag(
            &mut harness,
            from,
            from + egui::vec2(0.0, beat),
            egui::Modifiers::NONE,
        );
        assert!(cursor_shows(&harness, "0:1:0"));
    }
    #[test]
    fn turning_auto_jump_off_keeps_the_cursor_where_notes_go() {
        let mut harness = editable_demo();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
        harness.run();
        harness
            .get_by_role_and_label(Role::CheckBox, "Move on a snap after placing a note")
            .click();
        harness.run();
        assert!(!harness.state().settings().auto_jump);
        harness.get_by_label("Close settings").click();
        harness.run();
        harness.key_press(egui::Key::Num1);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [0, 3]);
        assert!(cursor_shows(&harness, "0:0:0"));
    }
    #[test]
    fn a_ctrl_drag_draws_its_hold_while_a_message_comes_and_goes() {
        let mut harness = editable_four_taps();
        harness
            .state_mut()
            .refuse(Path::new("other.json"), "it is not a song");
        harness.run();
        let (from, to) = (spot(&harness, 3, 240, 0.0), spot(&harness, 3, 720, -3.0));
        drag(&mut harness, from, to, egui::Modifiers::COMMAND);
        assert_eq!(note_at(&harness, 240, 3), Some(hold(480)));
    }
    #[test]
    fn a_hold_reaches_where_the_drag_lets_go() {
        let mut harness = editable_four_taps();
        let (from, near, far) = (
            spot(&harness, 3, 240, 0.0),
            spot(&harness, 3, 480, 0.0),
            spot(&harness, 3, 960, 0.0),
        );
        harness.hover_at(from);
        harness.event(egui::Event::ModifiersChanged(egui::Modifiers::COMMAND));
        press(&harness, from, true, egui::Modifiers::COMMAND);
        harness.hover_at(from.lerp(near, 0.5));
        harness.hover_at(near);
        press(&harness, far, false, egui::Modifiers::COMMAND);
        harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        harness.run();
        assert_eq!(note_at(&harness, 240, 3), Some(hold(720)));
    }
    #[test]
    fn escape_during_a_box_selection_cancels_it() {
        let mut harness = editable_four_taps();
        let (from, to) = (spot(&harness, 1, 180, 0.0), spot(&harness, 2, 540, 0.0));
        harness.hover_at(from);
        press(&harness, from, true, egui::Modifiers::NONE);
        harness.hover_at(from.lerp(to, 0.5));
        harness.hover_at(to);
        harness.key_press(egui::Key::Escape);
        press(&harness, to, false, egui::Modifiers::NONE);
        harness.run();
        assert!(nothing_selected(&harness));
    }
    #[test]
    fn a_lane_key_held_across_a_chart_switch_leaves_the_other_chart_alone() {
        let mut harness = editable_demo();
        harness.key_down(egui::Key::Num1);
        harness.run();
        harness.get_by_label("Hard").click();
        harness.run();
        harness.key_press(egui::Key::ArrowDown);
        harness.run();
        assert_eq!(note_at(&harness, 0, 0), Some(tap()));
    }
    #[test]
    fn a_lane_key_on_a_note_removes_it_and_leaves_the_cursor_there() {
        let mut harness = editable_demo();
        harness.key_press(egui::Key::Num4);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), Vec::<u16>::new());
        assert!(cursor_shows(&harness, "0:0:0"));
    }
    #[test]
    fn a_key_past_the_last_lane_does_nothing() {
        let mut harness = editable_demo();
        harness.key_press(egui::Key::Num5);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [3]);
        assert!(cursor_shows(&harness, "0:0:0"));
        assert!(harness.query_by_label_contains("Refused").is_none());
    }
    #[test]
    fn ctrl_z_takes_an_edit_back_and_ctrl_shift_z_or_ctrl_y_makes_it_again() {
        let mut harness = editable_demo();
        let command = egui::Modifiers::COMMAND;
        harness.key_press(egui::Key::Num1);
        harness.run();
        harness.key_press_modifiers(command, egui::Key::Z);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [3]);
        assert!(cursor_shows(&harness, "0:0:0"));
        harness.key_press_modifiers(command.plus(egui::Modifiers::SHIFT), egui::Key::Z);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [0, 3]);
        harness.key_press_modifiers(command, egui::Key::Z);
        harness.run();
        harness.key_press_modifiers(command, egui::Key::Y);
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [0, 3]);
    }
    #[test]
    fn the_edit_menu_undoes_and_redoes_what_it_can() {
        let mut harness = editable_demo();
        harness.get_by_label("Edit").click();
        harness.run();
        let disabled = |view: &Harness<'_, Window>, item: &str| {
            view.get_by_label_contains(item)
                .accesskit_node()
                .is_disabled()
        };
        assert!(disabled(&harness, "Undo") && disabled(&harness, "Redo"));
        harness.key_press(egui::Key::Escape);
        harness.key_press(egui::Key::Num1);
        harness.run();
        harness.get_by_label("Edit").click();
        harness.run();
        assert!(!disabled(&harness, "Undo"));
        harness.get_by_label_contains("Undo").click();
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [3]);
        harness.get_by_label("Edit").click();
        harness.run();
        harness.get_by_label_contains("Redo").click();
        harness.run();
        assert_eq!(lanes_at(&harness, 0), [0, 3]);
    }
    #[test]
    fn the_keyboard_moves_the_cursor_and_changes_the_snap() {
        let mut harness = with_demo();
        for _ in 0..4 {
            harness.key_press(egui::Key::ArrowDown);
        }
        harness.run();
        assert!(cursor_shows(&harness, "0:1:0"));
        harness.key_press(egui::Key::ArrowRight);
        harness.run();
        assert!(harness.query_by_label("Snap 24th").is_some());
        harness.key_press(egui::Key::PageDown);
        harness.run();
        assert!(cursor_shows(&harness, "1:0:0"));
    }
    #[test]
    fn ctrl_plus_zooms_the_notefield_and_leaves_the_window_its_size() {
        let mut harness = with_demo();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Plus);
        harness.run();
        assert!(harness.query_by_label("Zoom 125%").is_some());
        assert!((harness.ctx.zoom_factor() - 1.0).abs() < f32::EPSILON);
    }
    #[test]
    fn fallback_fonts_are_installed_in_the_windows_text_families() {
        let bytes = include_bytes!("../../../charter-style/assets/fonts/Inter-Regular.ttf");
        let app =
            Window::default().with_fallback_fonts(vec![("fallback".to_owned(), bytes.to_vec())]);
        let harness = window(app);
        harness.ctx.fonts(|fonts| {
            let definitions = fonts.definitions();
            assert!(definitions.font_data.contains_key("fallback"));
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                assert!(
                    definitions.families[&family]
                        .iter()
                        .any(|name| name == "fallback")
                );
            }
        });
    }
}
