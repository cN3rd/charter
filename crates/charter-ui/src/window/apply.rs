//! Action dispatch from panels, keys and gestures.

use super::{
    Action, KeepNotes, Note, PASTE_WAIT, Rate, Request, Saving, Selecting, Song, Step, Tick,
    Toggled, Window, grow, nudge_bpm, pattern, placing_at, refused, retime, stretch,
};

impl Window {
    /// Handles this private window operation.
    #[expect(clippy::too_many_lines, reason = "one match arm per window command")]
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
            Action::Help => self.help_open = !self.help_open,
            Action::ToggleSongPanel => self.settings.song_panel = !self.settings.song_panel,
            Action::ToggleMetronome => self.settings.metronome = !self.settings.metronome,
            Action::ToggleTicks => self.settings.note_ticks = !self.settings.note_ticks,
            Action::ToggleTunedLanes => self.settings.tuned_lanes = !self.settings.tuned_lanes,
            Action::SetGrid(grid) => self.settings.grid = grid,
            Action::TogglePlay => self.toggle_play(),
            Action::Audition(from) => self.audition(from),
            Action::AskResetTiming => self.dialogs.resetting = true,
            Action::FindTiming => self.find_timing(),
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
            Action::OpenChart(_)
            | Action::AddLayer
            | Action::SetActiveLayer(_)
            | Action::StepLayer(_)
            | Action::MoveSelectionToLayer(_)
            | Action::SelectLayer(_)
            | Action::SetLayerView(..)
            | Action::SoloLayer(_)
            | Action::SetLayerLocked(..)
            | Action::RecolourLayer(..)
            | Action::RenameLayer(..)
            | Action::MoveLayer(..)
            | Action::MergeActiveLayer
            | Action::MergeLayer(_)
            | Action::DeleteLayer(..)
            | Action::CopyLayerToChart(..)
            | Action::ToggleActiveLayerView
            | Action::Move(_)
            | Action::MoveMeasure(_)
            | Action::FinerSnap
            | Action::CoarserSnap
            | Action::ZoomBy(_)
            | Action::Toggle(..)
            | Action::LaneUp(_)
            | Action::LanesReleased
            | Action::Place { .. }
            | Action::Stretch { .. }
            | Action::Select(..)
            | Action::MoveSelection { .. }
            | Action::SelectBetween { .. }
            | Action::Undo
            | Action::Redo
            | Action::SelectAll
            | Action::ClearSelection
            | Action::DeleteSelection
            | Action::ResizeNotes(_)
            | Action::Copy
            | Action::Cut
            | Action::Paste(_)
            | Action::Duplicate
            | Action::ShiftSelection { .. }
            | Action::Mirror
            | Action::Quantize
            | Action::ChangeNotes(_)
            | Action::Pattern { .. }
            | Action::SelectLane(_)
            | Action::Here(..)
            | Action::PasteHere(_)
            | Action::SetBpmAt(..)
            | Action::SetMetadata(_)
            | Action::SetSetting { .. }
            | Action::MovePointTo(..)
            | Action::SetMeterAt(..)
            | Action::RemoveTimingAt(_)
            | Action::SetOffset(_)
            | Action::AddBpm
            | Action::NudgeOffset(_)
            | Action::ImportTiming(_)
            | Action::JumpTo(_)
            | Action::SetSection(_)
            | Action::RemoveMarker(_)
            | Action::JumpEnd
            | Action::Timing(_)
            | Action::ResetTiming
            | Action::MoveTiming { .. }
            | Action::MoveMarker { .. }
            | Action::JumpSection(_)
            | Action::ToggleBookmark
            | Action::NudgeBpm(_) => {}
        }
    }
    /// Handles this private window operation.
    #[expect(clippy::too_many_lines, reason = "one arm per action")]
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
            self.clipboard.pasting = Some((tick, PASTE_WAIT));
            self.clipboard.request_paste = true;
            return;
        }
        if let Action::Paste(Some(_)) = &action
            && let Some((tick, _)) = self.clipboard.pasting.take()
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
        match action {
            Action::AddLayer => self.failure = song.add_layer().err().as_ref().map(refused),
            Action::SetActiveLayer(index) => song.set_active_layer(index),
            Action::StepLayer(step) => {
                let active = song.active_layer();
                song.set_active_layer(match step {
                    Step::Earlier => active.saturating_sub(1),
                    Step::Later => active.saturating_add(1),
                });
            }
            Action::MoveSelectionToLayer(index) => {
                self.failure = song
                    .move_selection_to_layer(index)
                    .err()
                    .as_ref()
                    .map(refused);
            }
            Action::SelectLayer(index) => song.select_layer(index),
            Action::SetLayerView(index, view) => {
                self.failure = song.set_layer_view(index, view).err().as_ref().map(refused);
            }
            Action::SoloLayer(index) => {
                self.failure = song.solo_layer(index).err().as_ref().map(refused);
            }
            Action::SetLayerLocked(index, locked) => {
                self.failure = song
                    .set_layer_locked(index, locked)
                    .err()
                    .as_ref()
                    .map(refused);
            }
            Action::RecolourLayer(index, colour) => {
                self.failure = song
                    .recolour_layer(index, colour)
                    .err()
                    .as_ref()
                    .map(refused);
            }
            Action::RenameLayer(index, name) => {
                self.failure = song.rename_layer(index, &name).err().as_ref().map(refused);
            }
            Action::MoveLayer(from, to) => {
                self.failure = song.move_layer(from, to).err().as_ref().map(refused);
            }
            Action::MergeActiveLayer => {
                self.failure = song
                    .merge_layer_down(song.active_layer())
                    .err()
                    .as_ref()
                    .map(refused);
            }
            Action::MergeLayer(index) => {
                self.failure = song.merge_layer_down(index).err().as_ref().map(refused);
            }
            Action::DeleteLayer(index, notes) => {
                self.failure = song.delete_layer(index, notes).err().as_ref().map(refused);
            }
            Action::CopyLayerToChart(index, chart) => {
                self.failure = match song.copy_layer_to_chart(index, chart) {
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
                song.open_chart(index);
                self.typing.holding.clear();
                self.typing.pressed.clear();
            }
            Action::Move(step) => {
                song.step(step);
                self.failure = stretch(song, &self.typing.holding).or_else(|| self.failure.take());
            }
            Action::MoveMeasure(step) => {
                song.step_measure(step);
                self.failure = stretch(song, &self.typing.holding).or_else(|| self.failure.take());
            }
            Action::FinerSnap => song.finer_snap(),
            Action::CoarserSnap => song.coarser_snap(),
            Action::ZoomBy(factor) => self.zoom = self.zoom.by(factor),
            Action::Toggle(lane, note) => {
                // Mines and removals are not held, so a chord starts once, with its first key.
                if self.typing.chord_since.is_none() {
                    self.typing.chord_since = Some(song.history_step());
                }
                let at = placing_at(song, self.playback.playing.then_some(input_latency));
                let toggled = song.toggle(at, lane, note.kind());
                if matches!(toggled, Ok(Toggled::Placed)) {
                    self.typing.placed = true;
                    if note == Note::Tap {
                        self.typing.holding.push((lane, at));
                        if self.playback.playing {
                            let now = song.timing().time_at(song.cursor()).seconds();
                            self.typing.pressed.push((lane, now - input_latency));
                        }
                    }
                }
                self.failure = toggled.err().as_ref().map(refused);
            }
            Action::LaneUp(lane) => {
                if self.playback.playing {
                    // Where the key came up is where the hold ends.
                    let now = song.timing().time_at(song.cursor()).seconds() - input_latency;
                    let held: Vec<_> = self
                        .typing
                        .holding
                        .iter()
                        .copied()
                        .filter(|(held, _)| *held == lane)
                        .collect();
                    self.failure = grow(song, &held, &self.typing.pressed, now)
                        .or_else(|| self.failure.take());
                }
                self.typing.holding.retain(|(held, _)| *held != lane);
                self.typing.pressed.retain(|(held, _)| *held != lane);
            }
            // Playback moves the cursor on already; a jump would seek the audio ahead.
            Action::LanesReleased => {
                // A chord, and the holds it grew while playing, undo as one.
                if let Some(step) = self.typing.chord_since.take() {
                    song.merge_since(step);
                }
                if std::mem::take(&mut self.typing.placed)
                    && self.settings.auto_jump
                    && !self.playback.playing
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
                    .toggle(tick, lane, note.kind())
                    .err()
                    .as_ref()
                    .map(refused);
            }
            Action::Stretch { lane, from, to } => {
                self.failure = song.stretch(lane, from, to).err().as_ref().map(refused);
            }
            Action::Select(Some(note), how) => song.select([note], how),
            Action::Select(None, Selecting::Replace) | Action::ClearSelection => {
                song.clear_selection();
            }
            Action::OpenDemo
            | Action::ToggleLayerColours
            | Action::Open
            | Action::Save
            | Action::SaveAs
            | Action::Settings
            | Action::Calibrate
            | Action::ToggleSync
            | Action::ShowSync
            | Action::ShowImport
            | Action::ChooseTimingFile
            | Action::Help
            | Action::ToggleSongPanel
            | Action::ToggleMetronome
            | Action::ToggleTicks
            | Action::ToggleTunedLanes
            | Action::FindTiming
            | Action::SetGrid(_)
            | Action::TogglePlay
            | Action::Audition(_)
            | Action::AskResetTiming
            | Action::Loop { .. }
            | Action::ToggleRate
            | Action::MuteMetronome(_)
            | Action::ChooseAudio
            | Action::ChooseArtwork
            | Action::Here(..)
            | Action::PasteHere(_)
            | Action::OpenRecent(_)
            | Action::ClearRecent
            | Action::NudgeRate(_)
            | Action::SetRate(_)
            | Action::SetVolumes(_)
            | Action::NewSong
            | Action::Select(None, Selecting::Add | Selecting::Subtract) => {}
            Action::SelectBetween { ticks, lanes, how } => {
                song.select_between(ticks.0..=ticks.1, lanes.0..=lanes.1, how);
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
            | Action::MoveTiming { .. } => self.failure = retime(song, &action),
            // The timing window says itself why its edits were refused.
            Action::Timing(edit) => match edit.apply(song) {
                Err(why) => {
                    let now = self.frame_time;
                    self.sync.refused(&edit, &why, song.timing(), now);
                }
                // A loop plays again from its start, so the edit is heard on this pass.
                Ok(()) if self.playback.looping.is_some() => self.play_loop(),
                Ok(()) => {}
            },
            Action::JumpTo(tick) => song.jump_to(tick),
            Action::JumpEnd => song.jump_to(song.end()),
            Action::MoveMarker { index, to } => {
                self.failure = song.move_marker(index, to).err().as_ref().map(refused);
            }
            Action::JumpSection(step) => {
                if let Some(tick) = song.section_from(step) {
                    song.jump_to(tick);
                }
            }
            Action::ToggleBookmark => {
                self.failure = song.toggle_bookmark().err().as_ref().map(refused);
            }
            Action::NudgeBpm(by) => self.failure = nudge_bpm(song, by),
            Action::SetSection(text) => {
                self.failure = song.set_section(&text).err().as_ref().map(refused);
            }
            Action::RemoveMarker(index) => {
                self.failure = song.remove_marker(index).err().as_ref().map(refused);
            }
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
                    i32::try_from(i64::from(snaps).saturating_mul(step)).unwrap_or(i32::MAX);
                self.failure = song
                    .move_selection(ticks, lanes)
                    .err()
                    .as_ref()
                    .map(refused);
            }
            Action::Mirror => self.failure = song.mirror_selection().err().as_ref().map(refused),
            Action::Quantize => {
                self.failure = song.quantize_selection().err().as_ref().map(refused);
            }
            Action::ChangeNotes(change) => {
                self.failure = song.change_selection(&change).err().as_ref().map(refused);
            }
            Action::Pattern {
                steps,
                per_beat,
                lane,
            } => self.failure = pattern(song, steps, per_beat, lane),
            Action::SelectLane(lane) => {
                song.select_between(
                    Tick::new(i32::MIN)..=Tick::new(i32::MAX),
                    lane..=lane,
                    Selecting::Replace,
                );
            }
            Action::ResizeNotes(step) => {
                self.failure = song.resize_notes(step).err().as_ref().map(refused);
            }
            Action::MoveSelection { ticks, lanes } => {
                self.failure = song
                    .move_selection(ticks, lanes)
                    .err()
                    .as_ref()
                    .map(refused);
            }
        }
    }
}
