//! Per-frame egui integration and drawing.

use super::{
    AUDIO, Action, Direction, Discard, Finding, Leaving, Loaded, NO_AUDIO, PendingPaste, Request,
    Restoring, Song, Sounds, Ui, Window, editable_lanes, file_name, import, keys, latency, minimap,
    new_song, notefield, panels, settings, shown_grid, shown_strip, style, timing_panel,
};

impl Window {
    /// Opens dropped chart files and assigns dropped audio files to the open song.
    pub(super) fn open_dropped(&mut self, ui: &Ui) {
        // Taken out of the input first, so no song is parsed while egui's context is locked.
        let dropped = ui.input(|input| input.raw.dropped_files.clone());
        for file in dropped {
            let path = file.path();
            let extension = path.extension().and_then(|extension| extension.to_str());
            if extension.is_some_and(|extension| AUDIO.contains(&extension.to_lowercase().as_str()))
            {
                self.use_audio(path);
                continue;
            }
            match file.bytes() {
                Ok(bytes) => self.open(path, &bytes),
                Err(error) => self.refuse(path, &error),
            }
        }
    }
    /// Draws modal questions and answers their choices.
    pub(super) fn dialogs(&mut self, ui: &Ui) {
        if let Some(draft) = &mut self.dialogs.new_song {
            match new_song::show(ui, self.plugins, draft, self.no_drops) {
                Some(new_song::Answer::Create { plugin, song }) => {
                    self.create(plugin, &song);
                }
                Some(new_song::Answer::ChooseAudio) => self.requests.push(Request::ChooseAudio),
                Some(new_song::Answer::Cancel) => self.dialogs.new_song = None,
                None => {}
            }
        }
        if let Some(offered) = &self.dialogs.offer
            && let Some(take) =
                panels::offer_tags(ui, offered.title.as_deref(), offered.artist.as_deref())
        {
            let taken = self.dialogs.offer.take();
            if let (true, Some(taken)) = (take, taken) {
                self.take_tags(taken.draft, taken.title.as_deref(), taken.artist.as_deref());
            }
        }
        if let Some(Restoring { path, song, .. }) = &self.dialogs.restoring
            && let Some(restore) = panels::ask_to_restore(ui, &file_name(path.as_deref(), song))
        {
            self.answer_restore(restore);
        }
        if self.dialogs.discard.is_some() {
            let name = self.song.as_ref().map(|song| self.file_name(song));
            if let Some(answer) = panels::ask_to_save(ui, name.as_deref().unwrap_or_default()) {
                self.answer(answer);
            }
        }
    }
    /// Filters raw key actions through focused text fields, modals and the timing window.
    pub(super) fn keyboard(&mut self, ui: &Ui) -> Vec<Action> {
        let lanes = if self.calibration.is_open() {
            0
        } else {
            self.song.as_ref().map_or(0, editable_lanes)
        };
        let lane_keys = &mut self.typing.lane_keys;
        let asked = ui.input(|input| keys::key_actions(input, lanes, lane_keys));
        // A focused widget or a question takes the keyboard, but a lane key coming up still ends
        // its chord.
        let asking = self.dialogs.new_song.is_some()
            || self.dialogs.discard.is_some()
            || self.dialogs.restoring.is_some()
            || self.dialogs.offer.is_some()
            || self.calibration.is_open()
            || self.dialogs.resetting
            || self.dialogs.choosing.is_some()
            || matches!(self.dialogs.finding, Finding::Asking | Finding::NoGpu);
        // A button that Tab focused wants no letters, so only a text field counts.
        let typing = ui
            .ctx()
            .memory(egui::Memory::focused)
            .is_some_and(|id| egui::text_edit::TextEditState::load(ui.ctx(), id).is_some());
        // The timing window, once clicked, takes the keys it has a use for; undo and redo still
        // reach the song.
        let timing = self.sync.has_keyboard();
        let history = |action: &Action| matches!(action, Action::Undo | Action::Redo);
        let ends_a_chord =
            |action: &Action| matches!(action, Action::LaneUp(_) | Action::LanesReleased);
        // Commands a text field has no use for of its own still work while one has the keyboard.
        let global = |action: &Action| {
            matches!(
                action,
                Action::Save
                    | Action::SaveAs
                    | Action::Open
                    | Action::NewSong
                    | Action::Settings
                    | Action::ToggleSync
                    | Action::Help
            )
        };
        asked
            .into_iter()
            .filter(|action| {
                let open = if typing {
                    global(action)
                } else if timing {
                    global(action) || history(action)
                } else {
                    true
                };
                ends_a_chord(action) || !asking && open
            })
            .collect()
    }
    /// Draws one editor frame and applies the actions its controls produce.
    #[expect(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        reason = "one frame, panels in drawing order"
    )]
    pub fn ui(&mut self, ui: &mut Ui) {
        style::apply(ui.ctx(), &self.fallback_fonts);
        self.frame_time = ui.input(|input| input.time);
        let scale = self.settings.scale();
        if (ui.ctx().zoom_factor() - scale).abs() > f32::EPSILON {
            ui.ctx().set_zoom_factor(scale);
        }
        if self.dialogs.leaving == Leaving::Now {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            self.dialogs.leaving = Leaving::Going;
        }
        if ui.input(|input| input.viewport().close_requested())
            && self.dialogs.leaving == Leaving::Staying
        {
            if self.modified() {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.dialogs.discard = Some(Discard::Close);
            } else {
                self.dialogs.leaving = Leaving::Going;
            }
        }
        self.dialogs(ui);
        self.open_dropped(ui);
        self.forget_stale_audio();
        let mut actions = self.keyboard(ui);
        let song = self.song.as_ref();
        let audio_state = self.audio_state();
        let mut dismissed = false;
        let playing = self.playback.playing.is_some();
        egui::Panel::top("menu")
            .frame(style::chrome())
            .show(ui, |ui| {
                let importable = import::offered(self.plugins);
                let findable = self.audio_file().is_some() && self.dialogs.finding == Finding::Idle;
                let recent = &self.settings.recent;
                let formats = (self.songs_importable(), self.song_exportable());
                panels::menu_bar(
                    ui,
                    (song, recent),
                    (
                        playing,
                        importable,
                        findable,
                        self.settings.colour_notes_by_layer,
                    ),
                    formats,
                    &mut actions,
                );
            });
        let sounds = (
            self.settings.metronome,
            (self.settings.note_ticks, self.settings.tuned_lanes),
            self.settings.volumes,
        );
        let view = (
            self.zoom,
            self.playback.rate,
            self.playback.playing.is_some(),
            self.settings.song_panel,
            self.settings.grid,
        );
        egui::Panel::top("transport")
            .frame(style::toolbar())
            .show(ui, |ui| {
                panels::transport_bar(ui, self.song.as_ref(), view, sounds, &mut actions);
            });
        egui::Panel::bottom("status")
            .frame(style::chrome())
            .show(ui, |ui| {
                panels::status_bar(
                    ui,
                    self.song.as_ref(),
                    (&audio_state, self.playback.playing.is_some()),
                    &mut actions,
                );
            });
        self.follow_artwork();
        // The ticks and metronome are mixed in, and the pitch kept or not, as playback starts,
        // so a change to them plays again from where it is, once the pointer lets go.
        let changed = self.playback.played_sounds != Some(Sounds::of(&self.settings));
        if self.playback.playing.is_some() && changed && !ui.input(|input| input.pointer.any_down())
        {
            self.replay();
        }
        if let Some(text) = self.clipboard.out.take() {
            ui.ctx().copy_text(text);
        }
        if std::mem::take(&mut self.clipboard.request_paste) {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::RequestPaste);
        }
        // No text on the clipboard sends no paste, so the notes copied last go instead.
        match self.clipboard.pasting {
            Some(PendingPaste { tick, wait: 0 }) => {
                self.clipboard.pasting = None;
                self.apply(Action::Here(tick, Box::new(Action::Paste(None))));
            }
            Some(PendingPaste { tick, wait }) => {
                self.clipboard.pasting = Some(PendingPaste {
                    tick,
                    wait: wait.saturating_sub(1),
                });
                ui.ctx().request_repaint();
            }
            None => {}
        }
        let artwork = self
            .artwork
            .current
            .as_mut()
            .map(|artwork| artwork.shown(ui.ctx()));
        if let Some(panel_song) = self.song.as_ref().filter(|_| self.settings.song_panel) {
            let timing = &mut self.timing_drafts;
            let facts = &mut self.song_drafts;
            let section = &mut self.section_draft;
            let refused = egui::Panel::left("song")
                .frame(style::panel())
                .default_size(300.0)
                .show(ui, |ui| {
                    style::solid_scroll_bars(ui);
                    egui::ScrollArea::vertical()
                        .show(ui, |ui| {
                            let named =
                                panels::song_panel(ui, panel_song, (facts, artwork), &mut actions);
                            let refused =
                                timing_panel::section(ui, panel_song, timing, &mut actions);
                            let refused = named.or(refused);
                            panels::markers(ui, panel_song, section, &mut actions);
                            panels::details(ui, panel_song);
                            refused
                        })
                        .inner
                })
                .inner;
            if refused.is_some() {
                self.failure = refused;
            }
        }
        egui::CentralPanel::default()
            .frame(style::canvas())
            .show(ui, |ui| {
                if let Some(failure) = &self.failure {
                    // Over the canvas rather than in its layout, so the notefield never moves
                    // under a drag when a message comes or goes.
                    dismissed = panels::toast(ui, failure);
                }
                if let Some(why) = self.song.as_ref().and_then(Song::read_only) {
                    panels::read_only(ui, why);
                }
                if self.song.as_ref().is_some_and(|open_song| {
                    open_song.document().document().metadata.audio.as_str() == NO_AUDIO
                }) {
                    panels::no_audio(ui, self.no_drops, &mut actions);
                }
                if let Some(active_song) = &self.song {
                    let shown = notefield::Shown {
                        zoom: self.zoom,
                        direction: self.settings.direction,
                        strip: shown_strip(
                            &self.settings,
                            self.analysis.waveform.as_ref(),
                            self.analysis.spectrum.as_ref(),
                            self.audio_file().as_deref(),
                        ),
                        grid: shown_grid(self.settings.grid, active_song.snap()),
                        colour_notes_by_layer: self.settings.colour_notes_by_layer,
                    };
                    let map = self.settings.minimap && active_song.chart().is_some();
                    let length = match self.loaded() {
                        Some(Loaded::Ready(seconds)) => Some(*seconds),
                        _ => None,
                    };
                    let reversed = self.settings.direction == Direction::Down;
                    let (scrolled, gesture, object_order) = (
                        &mut self.typing.scrolled,
                        &mut self.gesture,
                        &mut self.object_order,
                    );
                    style::stage().show(ui, |ui| {
                        ui.set_min_size(ui.available_size());
                        let whole = ui.available_rect_before_wrap();
                        let (field, beside) = if map {
                            let gap = minimap::WIDTH + 8.0;
                            let (field, beside) = whole.split_left_right_at_x(whole.right() - gap);
                            (field, Some(beside.with_min_x(beside.left() + 8.0)))
                        } else {
                            (whole, None)
                        };
                        let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(field));
                        let visible = notefield::show(
                            &mut inner,
                            active_song,
                            shown,
                            scrolled,
                            gesture,
                            object_order,
                            &mut actions,
                        );
                        if let (Some(beside), Some(visible)) = (beside, visible) {
                            minimap::show(
                                ui,
                                beside,
                                active_song,
                                (visible, length, reversed),
                                &mut actions,
                            );
                        }
                        ui.allocate_rect(whole, egui::Sense::hover());
                    });
                } else {
                    let recent = &self.settings.recent;
                    panels::empty(ui, (self.no_drops, recent), &mut actions);
                }
            });
        let asked = settings::show(
            ui.ctx(),
            &mut self.settings,
            &mut self.settings_open,
            self.autosave.data_folder.as_deref(),
        );
        if asked == Some(settings::Asked::ChooseSnapshotFolder) {
            self.requests.push(Request::ChooseSnapshotFolder);
        }
        if asked == Some(settings::Asked::Calibrate) {
            self.calibration.set_open(true);
        }
        let kit = self.settings.metronome_sound;
        let time = (self.frame_time, kit, self.calibration_beat());
        if let Some(request) =
            latency::show(ui.ctx(), &mut self.calibration, time, &mut self.settings)
        {
            self.requests.push(request);
        }
        if self.dialogs.finding == Finding::Asking
            && let Some(download) = panels::ask_to_download_weights(ui, self.dialogs.tracker)
        {
            if download {
                self.requests.push(Request::DownloadWeights);
                self.dialogs.finding = Finding::Downloading(0.0);
            } else {
                self.dialogs.finding = Finding::Idle;
            }
        }
        if self.dialogs.finding == Finding::NoGpu
            && let Some(on_cpu) = panels::ask_to_run_on_cpu(ui)
        {
            if on_cpu {
                self.requests.push(Request::FindTimingOnCpu);
                self.dialogs.finding = Finding::Running(None);
            } else {
                self.dialogs.finding = Finding::Idle;
            }
        }
        self.choose_format(ui);
        if self.dialogs.resetting
            && let Some(answer) = panels::ask_to_reset_timing(ui)
        {
            self.dialogs.resetting = false;
            if answer {
                actions.push(Action::ResetTiming);
            }
        }
        if let Some(settings::Asked::Preview(preview_kit)) = asked {
            let clicks = (0..4_u8).map(|beat| {
                let tone = if beat == 0 {
                    charter_audio::Tone::Accent(preview_kit)
                } else {
                    charter_audio::Tone::Beat(preview_kit)
                };
                (f64::from(beat) * 0.5, tone)
            });
            self.requests.push(Request::Preview {
                clicks: clicks.collect(),
                volumes: self.settings.volumes,
            });
        }
        import::show(
            ui.ctx(),
            self.song.as_ref(),
            self.plugins,
            &mut self.import,
            &mut actions,
        );
        if self.sync.is_open() {
            // The timing window shows the audio whether or not the notefield's waveform is on.
            let file = self.audio_file();
            let peaks = self
                .analysis
                .waveform
                .as_ref()
                .filter(|analysis| Some(analysis.file.as_path()) == file.as_deref())
                .map(|analysis| analysis.value.as_ref());
            let onsets = self
                .analysis
                .onsets
                .as_ref()
                .filter(|analysis| Some(analysis.file.as_path()) == file.as_deref())
                .map_or(&[][..], |analysis| analysis.value.as_ref());
            let inputs = charter_timing::Inputs {
                song: self.song.as_ref(),
                peaks,
                onsets,
                playing: self.playback.playing.is_some(),
                latency_ms: self.settings.audio_latency_ms.ms(),
            };
            let mut commands = Vec::new();
            charter_timing::show(ui.ctx(), inputs, &mut self.sync, &mut commands);
            for command in commands {
                match command {
                    charter_timing::Command::Edit(edit) => actions.push(Action::Timing(edit)),
                    charter_timing::Command::PlayFrom(tick) => {
                        actions.push(Action::JumpTo(tick));
                        if self.playback.playing.is_none() {
                            actions.push(Action::TogglePlay);
                        }
                    }
                    charter_timing::Command::Audition(from) => actions.push(Action::Audition(from)),
                    charter_timing::Command::TogglePlay => actions.push(Action::TogglePlay),
                    charter_timing::Command::Loop { from, to } => {
                        actions.push(Action::Loop { from, to });
                    }
                    charter_timing::Command::ToggleRate => actions.push(Action::ToggleRate),
                    charter_timing::Command::ToggleMetronome => {
                        actions.push(Action::ToggleMetronome);
                    }
                    charter_timing::Command::MuteMetronome(muted) => {
                        actions.push(Action::MuteMetronome(muted));
                    }
                }
            }
        }
        if dismissed {
            self.failure = None;
        }
        panels::shortcuts(ui.ctx(), &mut self.help.shortcuts, &mut self.help_filter);
        panels::about(ui.ctx(), &mut self.help.about);
        let before = self.song.as_ref().map(Song::cursor);
        for action in actions {
            self.apply(action);
        }
        if self.playback.playing.is_some()
            && let Some(playing_song) = &self.song
            && Some(playing_song.cursor()) != before
        {
            let to = playing_song.timing().time_at(playing_song.cursor());
            self.requests.push(Request::Seek(to));
        }
        self.follow_volumes(ui);
        if let Some(saving) = self.saving.take() {
            self.save(saving);
        }
        self.autosave(ui.ctx());
        self.snapshot(ui.ctx());
        let title = self.title();
        if title != self.shown_title {
            self.shown_title.clone_from(&title);
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Title(title));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::window::test_support::*;

    #[test]
    fn ctrl_minus_zooms_out_where_the_layout_puts_minus_on_a_digit_key() {
        let mut harness = editable_demo();
        let command = egui::Modifiers::COMMAND;
        harness.event(egui::Event::ModifiersChanged(command));
        harness.event(keyboard(egui::Key::Minus, egui::Key::Num6, true, command));
        harness.run();
        assert!(harness.query_by_label("Zoom 80%").is_some());
    }
    #[test]
    fn the_song_panel_folds_away_and_comes_back() {
        let mut harness = editable_demo();
        assert!(harness.query_by_label("Verse").is_some());
        harness.get_by_label("Hide the song panel").click();
        harness.run();
        assert!(harness.query_by_label("Verse").is_none());
        assert!(!harness.state().settings().song_panel);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Backslash);
        harness.run();
        assert!(harness.query_by_label("Verse").is_some());
        harness.get_by_label("View").click();
        harness.run();
        harness.get_by_label_contains("Song panel").click();
        harness.run();
        assert!(harness.query_by_label("Verse").is_none());
    }
    #[test]
    fn the_settings_window_closes_from_its_own_button() {
        let mut harness = editable_demo();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
        harness.run();
        harness.get_by_label("Close settings").click();
        harness.run();
        assert!(harness.query_all_by_label("Settings").next().is_none());
    }
    #[test]
    fn the_interface_size_setting_scales_the_whole_window() {
        let settings = Settings {
            scale: 1.5,
            ..Settings::default()
        };
        let harness = window(Window::new(PLUGINS).with_settings(settings));
        assert!((harness.ctx.zoom_factor() - 1.5).abs() < f32::EPSILON);
    }
}
