//! Playback and audio-rate state changes.

use super::{
    AudioStatus, Finding, Loaded, LoopRegion, Mode, NO_AUDIO, Path, PlayedAudio, Rate, Request,
    ResolvedTime, Sounds, Step, Tick, TimeShift, Ui, Window, clicks, grow, latency,
};

impl Window {
    /// Marks the song's audio decoded and ready to play.
    pub fn audio_ready(&mut self, file: &Path, seconds: f64) {
        self.playback.audio = Some(AudioStatus {
            file: file.to_path_buf(),
            loaded: Loaded::Ready(seconds),
        });
    }
    /// Marks the song's audio unreadable and shows why.
    pub fn audio_failed(&mut self, file: &Path, why: &str) {
        self.playback.audio = Some(AudioStatus {
            file: file.to_path_buf(),
            loaded: Loaded::Failed,
        });
        self.failure = Some(format!("{} cannot play: {why}", file.display()));
    }
    /// Moves the cursor to the audio position the host says is playing.
    pub fn playhead(&mut self, seconds: f64) {
        let Some(seconds) = ResolvedTime::from_seconds(seconds) else {
            return;
        };
        let Some(mode) = self.playback.playing else {
            return;
        };
        if let Mode::Snippet { until, .. } = mode {
            if seconds >= until {
                self.playback.playing = None;
                self.requests.push(Request::Pause);
            }
            return;
        }
        // What plays now is heard a moment later: the cursor shows what is heard.
        let Some(latency) = TimeShift::from_seconds(
            self.settings
                .audio_latency_ms
                .seconds_at(self.playback.rate.factor()),
        ) else {
            return;
        };
        let heard = seconds - latency;
        let Some(input_latency) = TimeShift::from_seconds(self.input_latency()) else {
            return;
        };
        let tapped = heard - input_latency;
        if let Some(song) = &mut self.song {
            let tick = song.timing().latest_tick_by(heard);
            song.jump_to(tick);
            if let Some(why) = grow(song, &self.typing.holding, &self.typing.pressed, tapped) {
                self.failure = Some(why);
            }
        }
    }
    /// Click track used by the latency calibration window.
    pub(super) fn calibration_beat(&self) -> latency::Beat {
        let Some(song) = &self.song else {
            return latency::Beat::default();
        };
        let timing = song.timing();
        let first = timing
            .anchors
            .first()
            .map_or(Tick::new(0), |anchor| anchor.tick);
        let per_measure = timing
            .measures_from(first)
            .next()
            .map_or(4, |measure| measure.meter.numerator.get());
        latency::Beat::of(timing.tempo_at(first).bpm(), per_measure)
    }
    /// Input latency as playback-speed-adjusted seconds.
    pub(super) fn input_latency(&self) -> f64 {
        self.settings
            .input_latency_ms
            .seconds_at(self.playback.rate.factor())
    }
    /// User-facing audio load or timing-discovery status.
    #[expect(
        clippy::non_ascii_literal,
        reason = "the audio-analysis status shows an ellipsis while running"
    )]
    pub(super) fn audio_state(&self) -> String {
        match self.dialogs.finding {
            Finding::Downloading(share) => {
                let name = match self.dialogs.tracker {
                    super::Tracker::BeatThis => "Beat This!",
                    super::Tracker::Mapperatorinator => "Mapperatorinator",
                };
                return format!("Downloading the {name} weights, {:.0}%", share * 100.0);
            }
            Finding::Running(None) => return "Finding the timing…".to_owned(),
            Finding::Running(Some(share)) => {
                return format!("Finding the timing, {:.0}%", share * 100.0);
            }
            Finding::Idle | Finding::Asking | Finding::NoGpu => {}
        }
        match (self.audio_file(), self.loaded()) {
            (None, _) => "No audio",
            (Some(_), Some(Loaded::Ready(_))) => "Audio ready",
            (Some(_), Some(Loaded::Failed)) => "Audio failed",
            (Some(_), None) => "Audio loading",
        }
        .to_owned()
    }
    /// Loaded state for the currently selected audio file.
    pub(super) fn loaded(&self) -> Option<&Loaded> {
        let wanted = self.audio_file()?;
        self.playback
            .audio
            .as_ref()
            .filter(|status| status.file == wanted)
            .map(|status| &status.loaded)
    }
    /// Sends live volume changes to the host, or restarts mixed-click playback after dragging.
    pub(super) fn follow_volumes(&mut self, ui: &Ui) {
        let (Some(played), true) = (self.playback.played, self.playback.playing.is_some()) else {
            return;
        };
        let wanted = self.settings.volumes;
        if wanted == played.volumes {
            return;
        }
        if !played.mixed_clicks {
            if wanted.music != played.volumes.music {
                self.requests.push(Request::Volume(wanted.music));
            }
            self.playback.played = Some(PlayedAudio {
                volumes: wanted,
                mixed_clicks: false,
            });
        } else if !ui.input(|input| input.pointer.any_down()) {
            self.replay();
        }
    }
    /// Plays from `from` with the metronome on to hear a timing edit.
    pub(super) fn audition(&mut self, from: ResolvedTime) {
        self.start(from, Mode::Audition);
    }
    /// Plays the settings' snippet of audio before the cursor, or after it, then pauses, leaving the
    /// cursor where it is so the same moment can be heard again.
    pub(super) fn play_snippet(&mut self, side: Step) {
        let Some(song) = &self.song else {
            return;
        };
        if !matches!(self.loaded(), Some(Loaded::Ready(_))) {
            // Says why nothing plays.
            if self.playback.playing.is_none() {
                self.toggle_play();
            }
            return;
        }
        let at = song.timing().time_at(song.cursor());
        let Some(length) = TimeShift::from_seconds(self.settings.snippet_seconds()) else {
            return;
        };
        let (from, until) = match side {
            Step::Earlier => (at - length, at),
            Step::Later => (at, at + length),
        };
        self.start(from, Mode::Snippet { from, until });
    }
    /// Starts or stops looping the requested tick span.
    pub(super) fn toggle_loop(&mut self, from: Tick, to: Tick) {
        if self.playback.playing == Some(Mode::Loop(from, to)) {
            self.playback.playing = None;
            self.requests.push(Request::Pause);
            return;
        }
        self.play_loop(from, to);
    }
    /// Starts looping the ticks from `from` to `to`, from the first.
    pub(super) fn play_loop(&mut self, from: Tick, to: Tick) {
        let Some(song) = &self.song else {
            return;
        };
        let start = song.timing().time_at(from);
        self.start(start, Mode::Loop(from, to));
    }
    /// Plays what is playing again after a setting changes: a loop or a snippet from its start,
    /// and anything else from the cursor.
    pub(super) fn replay(&mut self) {
        let Some(song) = &self.song else {
            return;
        };
        let cursor = song.timing().time_at(song.cursor());
        match self.playback.playing {
            None => {}
            Some(Mode::Loop(from, to)) => self.play_loop(from, to),
            Some(mode @ Mode::Snippet { from, .. }) => self.start(from, mode),
            Some(mode @ (Mode::Cursor | Mode::Audition)) => self.start(cursor, mode),
        }
    }
    /// Asks the host to play the song from an audio time for `mode`.
    pub(super) fn start(&mut self, from: ResolvedTime, mode: Mode) {
        let (Some(song), Some(&Loaded::Ready(seconds))) = (&self.song, self.loaded()) else {
            return;
        };
        let Some(from) = ResolvedTime::from_seconds(from.seconds().max(0.0)) else {
            return;
        };
        if self.playback.playing.is_some() {
            self.requests.push(Request::Pause);
        }
        let looping = match mode {
            Mode::Loop(first, last) => {
                let timing = song.timing();
                LoopRegion::new(timing.time_at(first), timing.time_at(last))
            }
            Mode::Cursor | Mode::Audition | Mode::Snippet { .. } => None,
        };
        let metronome = matches!(mode, Mode::Audition | Mode::Loop(..));
        let until = looping.map_or(seconds, |region| region.end().seconds().min(seconds));
        // From the song's start, so a seek back while playing still ticks.
        let clicks = clicks(
            song,
            &self.settings,
            (self.settings.metronome || metronome) && !self.playback.muted,
            (0.0, until),
            self.sync.grid(),
        );
        let volumes = self.settings.volumes;
        self.playback.playing = Some(mode);
        self.playback.played = Some(PlayedAudio {
            volumes,
            mixed_clicks: !clicks.is_empty(),
        });
        self.requests.push(Request::Play {
            from,
            rate: self.playback.rate.factor(),
            clicks,
            volumes,
            looping,
            keep_pitch: self.settings.keep_pitch,
        });
        self.playback.played_pitch = self.settings.keep_pitch;
        self.playback.played_sounds = Some(Sounds::of(&self.settings));
    }
    /// Switches between the usual speed and the alternate playback speed.
    pub(super) fn toggle_rate(&mut self) {
        if let Some(usual) = self.playback.usual_rate.take() {
            self.set_rate(usual);
        } else {
            self.playback.usual_rate = Some(self.playback.rate);
            self.set_rate(Rate::of(self.settings.rate_b.factor()));
        }
    }
    /// Starts playback at the cursor, or pauses if already playing.
    #[expect(
        clippy::non_ascii_literal,
        reason = "the playback error names the Choose audio menu label with its ellipsis"
    )]
    pub(super) fn toggle_play(&mut self) {
        if self.playback.playing.take().is_some() {
            self.requests.push(Request::Pause);
            return;
        }
        let Some(song) = &self.song else {
            return;
        };
        let from = song.timing().time_at(song.cursor());
        match self.loaded() {
            Some(&Loaded::Ready(_)) => self.start(from, Mode::Cursor),
            Some(Loaded::Failed) => {
                self.failure = Some(
                    "The audio could not be played. Choose another audio file with File > \
                     Choose audio… and try again."
                        .to_owned(),
                );
            }
            None if self.audio_file().is_none() => {
                let name = song.document().document().metadata.audio.as_str();
                self.failure = Some(match (name == NO_AUDIO, self.no_drops) {
                    (true, false) => "This song has no audio yet: drop an audio file on the \
                        window, or choose one."
                        .to_owned(),
                    (true, true) => "This song has no audio yet: choose one.".to_owned(),
                    (false, false) => format!(
                        "Save the song beside {name}, or drop {name} on the window, to play it."
                    ),
                    (false, true) => format!(
                        "Save the song beside {name}, or choose {name} with File > Choose \
                         audio, to play it."
                    ),
                });
            }
            None => self.failure = Some("The audio is still loading.".to_owned()),
        }
    }
    /// Sets the playback rate and keeps already-playing audio in sync.
    pub(super) fn set_rate(&mut self, rate: Rate) {
        self.playback.rate = rate;
        // Audio that keeps its pitch, or played keeping it, is stretched for its speed, and so is
        // audio under clicks, which keep their own; a new speed plays it again.
        let clicked = self
            .playback
            .played
            .is_some_and(|played| played.mixed_clicks);
        let playing = self.playback.playing.is_some();
        if playing && (self.settings.keep_pitch || self.playback.played_pitch || clicked) {
            self.replay();
        } else if playing {
            self.requests.push(Request::Rate(rate.factor()));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::window::test_support::*;

    #[test]
    fn the_settings_window_shows_one_page_at_a_time_starting_with_editing() {
        let mut harness = editable_demo();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
        harness.run();
        assert!(
            harness
                .query_by_role_and_label(Role::CheckBox, "Move on a snap after placing a note")
                .is_some()
        );
        assert!(
            harness
                .query_by_role_and_label(Role::ComboBox, "Notefield")
                .is_none()
        );
        settings_page(&mut harness, "Display");
        assert!(
            harness
                .query_by_role_and_label(Role::CheckBox, "Move on a snap after placing a note")
                .is_none()
        );
        assert!(
            harness
                .query_by_role_and_label(Role::ComboBox, "Notefield")
                .is_some()
        );
    }
    #[test]
    fn restore_defaults_puts_every_setting_back_but_the_calibration_and_recent_songs() {
        let mut app = Window::new(PLUGINS).with_settings(Settings {
            metronome: true,
            tuned_lanes: true,
            zoom: 150.0.into(),
            input_latency_ms: 25.0.into(),
            ..Settings::default()
        });
        app.open(Path::new("/songs/four.json"), &four_taps());
        let mut harness = window(app);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
        harness.run();
        harness.get_by_label("Restore defaults").click();
        harness.run();
        let calibrated = Settings {
            input_latency_ms: 25.0.into(),
            recent: vec![PathBuf::from("/songs/four.json")],
            ..Settings::default()
        };
        assert_eq!(harness.state().settings(), &calibrated);
    }
    #[test]
    fn the_settings_window_turns_the_notefield_and_picks_sounds() {
        let mut harness = editable_demo();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
        harness.run();
        settings_page(&mut harness, "Display");
        harness
            .get_by_role_and_label(Role::ComboBox, "Notefield")
            .click();
        harness.run();
        harness
            .get_by_label("Horizontal, notes travel left")
            .click();
        harness.run();
        assert_eq!(harness.state().settings().direction, Direction::Left);
        settings_page(&mut harness, "Sounds");
        harness
            .get_by_role_and_label(Role::CheckBox, "Metronome")
            .click();
        harness.run();
        harness
            .get_by_role_and_label(Role::ComboBox, "Metronome sound")
            .scroll_to_me();
        harness.run();
        harness
            .get_by_role_and_label(Role::ComboBox, "Metronome sound")
            .click();
        harness.run();
        harness.get_by_label("Shaker burst").click();
        harness.run();
        assert!(harness.state().settings().metronome);
        assert_eq!(harness.state().settings().metronome_sound, Kit::ShakerBurst);
        harness.get_by_label("Preview the metronome").click();
        harness.run();
        let preview = harness.state_mut().take_requests();
        let Some(Request::Preview { clicks, .. }) = preview.last() else {
            panic!("expected a preview, got {preview:?}");
        };
        let tones: Vec<Tone> = clicks.iter().map(|(_, tone)| *tone).collect();
        let (accent, beat) = (Tone::Accent(Kit::ShakerBurst), Tone::Beat(Kit::ShakerBurst));
        assert_eq!(tones, [accent, beat, beat, beat]);
        assert!(
            harness
                .query_by_role_and_label(Role::ComboBox, "Note tick sound")
                .is_some()
        );
    }
    #[test]
    fn play_after_an_audio_load_failure_explains_how_to_try_again() {
        let mut harness = four_taps_at("/songs/four.json");
        harness
            .state_mut()
            .audio_failed(Path::new("/songs/a.ogg"), "unsupported codec");
        harness.run();
        harness.get_by_label("Dismiss").click();
        harness.run();
        space(&mut harness);
        assert!(
            harness
                .query_by_label_contains("Choose another audio file")
                .is_some()
        );
        assert_eq!(harness.state_mut().take_requests(), []);
    }
    #[test]
    fn space_plays_from_the_cursor_and_pauses() {
        let mut harness = playable();
        harness.key_press(egui::Key::ArrowDown);
        harness.run();
        space(&mut harness);
        let Request::Play { from, rate, .. } = request(&mut harness) else {
            panic!("expected play");
        };
        assert_eq!(from, ResolvedTime::from_seconds(0.125).unwrap());
        assert!((rate - 1.0).abs() < f64::EPSILON);
        space(&mut harness);
        assert_eq!(request(&mut harness), Request::Pause);
    }
    #[test]
    fn audio_that_failed_to_load_says_why_and_does_not_play() {
        let mut harness = four_taps_at("/songs/four.json");
        harness
            .state_mut()
            .audio_failed(Path::new("/songs/a.ogg"), "no such file");
        harness.run();
        assert!(harness.query_by_label_contains("no such file").is_some());
        space(&mut harness);
        assert_eq!(harness.state_mut().take_requests(), []);
    }
    #[test]
    fn the_cursor_follows_the_playhead() {
        let mut harness = playable();
        space(&mut harness);
        harness.state_mut().take_requests();
        harness.state_mut().playhead(0.5);
        harness.run();
        assert!(cursor_shows(&harness, "0:1:0"));
        assert_eq!(harness.state_mut().take_requests(), []);
    }
    #[test]
    fn a_non_finite_playhead_does_not_move_the_cursor() {
        let mut harness = playable();
        space(&mut harness);
        harness.state_mut().take_requests();
        let before = harness.state().song.as_ref().unwrap().cursor();
        for seconds in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            harness.state_mut().playhead(seconds);
            assert_eq!(harness.state().song.as_ref().unwrap().cursor(), before);
            assert_eq!(harness.state_mut().take_requests(), []);
        }
    }
    #[test]
    fn a_lane_key_while_playing_places_the_input_latency_before_the_cursor() {
        let mut harness = playable_with(Settings {
            input_latency_ms: 250.0.into(),
            ..Settings::default()
        });
        space(&mut harness);
        harness.state_mut().take_requests();
        // Heard at 0.88 s, meant at 0.63 s: tick 1209, the nearest sixteenth 1200.
        harness.state_mut().playhead(0.88);
        harness.run();
        harness.key_press(egui::Key::Num1);
        harness.run();
        assert_eq!(lanes_at(&harness, 1200), [0]);
    }
    #[test]
    fn a_lane_key_held_while_playing_grows_a_hold_until_it_comes_up() {
        let mut harness = playable();
        space(&mut harness);
        play_to(&mut harness, 0.5);
        harness.key_down(egui::Key::Num1);
        harness.run();
        play_to(&mut harness, 0.6);
        assert_eq!(note_at(&harness, 960, 0), Some(tap()));
        play_to(&mut harness, 0.9);
        assert_eq!(note_at(&harness, 960, 0), Some(hold(720)));
        play_to(&mut harness, 1.13);
        harness.key_up(egui::Key::Num1);
        harness.run();
        play_to(&mut harness, 1.6);
        assert_eq!(note_at(&harness, 960, 0), Some(hold(1200)));
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(note_at(&harness, 960, 0), None);
    }
    #[test]
    fn a_chord_held_while_playing_undoes_in_one_step() {
        let mut harness = playable();
        space(&mut harness);
        play_to(&mut harness, 0.5);
        harness.key_down(egui::Key::Num1);
        harness.key_down(egui::Key::Num2);
        harness.run();
        for seconds in [0.75, 0.9, 1.1] {
            play_to(&mut harness, seconds);
        }
        harness.key_up(egui::Key::Num1);
        harness.key_up(egui::Key::Num2);
        harness.run();
        assert!(note_at(&harness, 960, 0).is_some_and(|note| note["kind"] == "hold"));
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(note_at(&harness, 960, 0), None);
        assert_eq!(note_at(&harness, 960, 1), None);
    }
    #[test]
    fn a_lane_key_let_go_quickly_while_playing_stays_a_tap() {
        let mut harness = playable();
        space(&mut harness);
        play_to(&mut harness, 0.5);
        harness.key_down(egui::Key::Num1);
        harness.run();
        play_to(&mut harness, 0.62);
        harness.key_up(egui::Key::Num1);
        harness.run();
        play_to(&mut harness, 1.0);
        assert_eq!(note_at(&harness, 960, 0), Some(tap()));
    }
    #[test]
    fn moving_the_cursor_while_playing_seeks_there() {
        let mut harness = playable();
        space(&mut harness);
        harness.state_mut().take_requests();
        harness.key_press(egui::Key::PageDown);
        harness.run();
        let Request::Seek(to) = request(&mut harness) else {
            panic!("expected a seek");
        };
        assert_eq!(to, ResolvedTime::from_seconds(2.0).unwrap());
    }
    #[test]
    fn audio_under_clicks_plays_again_at_a_new_speed_even_letting_its_pitch_follow() {
        let mut harness = playable_with(Settings {
            keep_pitch: false,
            metronome: true,
            ..Settings::default()
        });
        space(&mut harness);
        let Request::Play { clicks, .. } = request(&mut harness) else {
            panic!("expected playback");
        };
        assert!(!clicks.is_empty());
        harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowLeft);
        harness.run();
        let requests = harness.state_mut().take_requests();
        let Some(Request::Play { rate, .. }) = requests.last() else {
            panic!("expected playback again, got {requests:?}");
        };
        assert!((rate - 0.9).abs() < 1e-9, "{rate}");
    }
    #[test]
    fn q_plays_the_snippet_before_the_cursor_and_stops_there_leaving_it_in_place() {
        let mut harness = playable();
        harness.key_press(egui::Key::PageDown);
        harness.run();
        harness.state_mut().take_requests();
        harness.key_press(egui::Key::Q);
        harness.run();
        let Request::Play { from, looping, .. } = request(&mut harness) else {
            panic!("expected play");
        };
        assert!((from.seconds() - 1.8).abs() < 1e-9, "{from:?}");
        assert_eq!(looping, None);
        harness.state_mut().playhead(1.9);
        harness.run();
        assert_eq!(harness.state_mut().take_requests(), []);
        assert!(cursor_shows(&harness, "1:0:0"));
        harness.state_mut().playhead(2.01);
        harness.run();
        assert_eq!(request(&mut harness), Request::Pause);
        assert!(cursor_shows(&harness, "1:0:0"));
        harness.key_press(egui::Key::Q);
        harness.run();
        assert!(
            matches!(request(&mut harness), Request::Play { from: again, .. } if (again.seconds() - 1.8).abs() < 1e-9)
        );
    }

    #[test]
    fn a_snippet_before_the_audio_starts_at_zero_and_keeps_its_end() {
        let mut harness = playable();
        harness.key_press(egui::Key::Q);
        harness.run();
        let Request::Play { from, .. } = request(&mut harness) else {
            panic!("expected play");
        };
        assert_eq!(from, ResolvedTime::from_seconds(0.0).unwrap());
        harness.state_mut().playhead(0.0);
        assert_eq!(request(&mut harness), Request::Pause);
    }
    #[test]
    fn w_plays_the_snippet_after_the_cursor_and_stops_at_its_end() {
        let mut harness = playable();
        harness.key_press(egui::Key::PageDown);
        harness.run();
        harness.state_mut().take_requests();
        harness.key_press(egui::Key::W);
        harness.run();
        let Request::Play { from, .. } = request(&mut harness) else {
            panic!("expected play");
        };
        assert!((from.seconds() - 2.0).abs() < 1e-9, "{from:?}");
        harness.state_mut().playhead(2.1);
        harness.run();
        assert_eq!(harness.state_mut().take_requests(), []);
        assert!(cursor_shows(&harness, "1:0:0"));
        harness.state_mut().playhead(2.2);
        harness.run();
        assert_eq!(request(&mut harness), Request::Pause);
        space(&mut harness);
        assert!(
            matches!(request(&mut harness), Request::Play { from: again, .. } if (again.seconds() - 2.0).abs() < 1e-9)
        );
    }
    #[test]
    fn a_non_finite_playhead_does_not_end_a_snippet() {
        let mut harness = playable();
        harness.key_press(egui::Key::PageDown);
        harness.run();
        harness.state_mut().take_requests();
        harness.key_press(egui::Key::W);
        harness.run();
        harness.state_mut().take_requests();
        harness.state_mut().playhead(f64::INFINITY);
        assert_eq!(harness.state_mut().take_requests(), []);
        harness.state_mut().playhead(2.2);
        assert_eq!(request(&mut harness), Request::Pause);
    }
    #[test]
    fn the_snippet_plays_as_long_as_the_settings_say() {
        let mut harness = playable_with(Settings {
            snippet_ms: 400_u32.into(),
            ..Settings::default()
        });
        harness.key_press(egui::Key::PageDown);
        harness.run();
        harness.state_mut().take_requests();
        harness.key_press(egui::Key::Q);
        harness.run();
        let Request::Play { from, .. } = request(&mut harness) else {
            panic!("expected play");
        };
        assert!((from.seconds() - 1.6).abs() < 1e-9, "{from:?}");
    }
    #[test]
    fn space_during_a_snippet_pauses_it_and_then_plays_on_normally() {
        let mut harness = playable();
        harness.key_press(egui::Key::W);
        harness.run();
        harness.state_mut().take_requests();
        space(&mut harness);
        assert_eq!(request(&mut harness), Request::Pause);
        space(&mut harness);
        harness.state_mut().take_requests();
        harness.state_mut().playhead(1.0);
        harness.run();
        assert_eq!(harness.state_mut().take_requests(), []);
        assert!(cursor_shows(&harness, "0:2:0"));
    }
    #[test]
    fn playback_that_ends_stops() {
        let mut harness = playable();
        space(&mut harness);
        harness.state_mut().take_requests();
        harness.state_mut().playback_ended();
        harness.run();
        space(&mut harness);
        assert!(matches!(request(&mut harness), Request::Play { .. }));
    }
    #[test]
    fn shift_left_and_right_change_the_playback_rate_without_touching_the_chart() {
        let mut harness = playable_with(Settings {
            keep_pitch: false,
            ..Settings::default()
        });
        let before = harness.state().song.as_ref().unwrap().document().clone();
        space(&mut harness);
        harness.state_mut().take_requests();
        harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowLeft);
        harness.run();
        let Request::Rate(rate) = request(&mut harness) else {
            panic!("expected a rate");
        };
        assert!((rate - 0.9).abs() < 1e-9, "{rate}");
        assert!(harness.query_by_label("Speed 90%").is_some());
        let command = egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT);
        harness.key_press_modifiers(command, egui::Key::ArrowRight);
        harness.run();
        let Request::Rate(requested_rate) = request(&mut harness) else {
            panic!("expected a rate");
        };
        assert!((requested_rate - 0.91).abs() < 1e-9, "{requested_rate}");
        let after = harness.state().song.as_ref().unwrap().document();
        assert_eq!(after.document(), before.document());
    }
    #[test]
    fn audio_keeping_its_pitch_plays_again_at_a_new_speed() {
        let mut harness = playable();
        space(&mut harness);
        let Request::Play { keep_pitch, .. } = request(&mut harness) else {
            panic!("expected playback");
        };
        assert!(keep_pitch);
        harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowLeft);
        harness.run();
        let requests = harness.state_mut().take_requests();
        let Some(Request::Play { rate, .. }) = requests.last() else {
            panic!("expected playback again, got {requests:?}");
        };
        assert!((rate - 0.9).abs() < 1e-9, "{rate}");
    }
    #[test]
    fn changing_the_sounds_in_the_settings_while_playing_plays_again_with_them() {
        let mut harness = playable_with(Settings {
            note_ticks: true,
            ..Settings::default()
        });
        space(&mut harness);
        harness.state_mut().take_requests();
        harness.state_mut().settings.tuned_lanes = true;
        harness.run();
        let requests = harness.state_mut().take_requests();
        let Some(Request::Play { clicks, .. }) = requests.last() else {
            panic!("expected playback again, got {requests:?}");
        };
        assert!(
            clicks
                .iter()
                .any(|(_, tone)| *tone != charter_audio::Tick::Click.tone())
        );
        harness.run();
        assert_eq!(harness.state_mut().take_requests(), [], "once");
    }
    #[test]
    fn turning_keep_pitch_off_while_playing_plays_again_and_speeds_follow() {
        let mut harness = playable();
        harness.state_mut().set_rate(Rate::of(0.6));
        space(&mut harness);
        harness.state_mut().take_requests();
        harness.state_mut().settings.keep_pitch = false;
        harness.run();
        let requests = harness.state_mut().take_requests();
        assert!(
            matches!(
                requests.last(),
                Some(Request::Play {
                    keep_pitch: false,
                    ..
                })
            ),
            "{requests:?}"
        );
        harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowRight);
        harness.run();
        let Request::Rate(rate) = request(&mut harness) else {
            panic!("expected a rate");
        };
        assert!((rate - 0.7).abs() < 1e-9, "{rate}");
    }
    #[test]
    fn the_playback_rate_stays_between_a_tenth_and_four_times() {
        let mut harness = playable();
        for _ in 0..40 {
            harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowLeft);
        }
        harness.run();
        assert!(harness.query_by_label("Speed 10%").is_some());
        space(&mut harness);
        let Request::Play { rate, .. } = request(&mut harness) else {
            panic!("expected play");
        };
        assert!((rate - 0.1).abs() < 1e-9, "{rate}");
    }
    #[test]
    fn playback_plays_at_the_volumes_the_settings_hold() {
        let mut harness = playable_with(Settings {
            volumes: quieter(),
            ..Settings::default()
        });
        space(&mut harness);
        let Request::Play { volumes, .. } = request(&mut harness) else {
            panic!("expected play");
        };
        assert_eq!(volumes, quieter());
    }
    #[test]
    fn with_no_clicks_the_music_volume_changes_live() {
        let mut harness = playable_with(Settings::default());
        space(&mut harness);
        harness.state_mut().take_requests();
        harness.state_mut().settings.volumes.music = Gain::from(0.3);
        harness.run();
        assert_eq!(request(&mut harness), Request::Volume(Gain::from(0.3)));
    }
    #[test]
    fn with_clicks_mixed_in_any_volume_change_plays_on_from_the_cursor() {
        let mut harness = playable_with(Settings {
            metronome: true,
            ..Settings::default()
        });
        space(&mut harness);
        harness.state_mut().take_requests();
        for change in [
            |volumes: &mut Volumes| volumes.music = Gain::from(0.3),
            |volumes: &mut Volumes| volumes.metronome = Gain::from(0.2),
        ] {
            change(&mut harness.state_mut().settings.volumes);
            harness.run();
            let requests = harness.state_mut().take_requests();
            let wanted = harness.state().settings.volumes;
            assert!(
                matches!(
                    requests.as_slice(),
                    [Request::Pause, Request::Play { volumes, .. }] if *volumes == wanted
                ),
                "{requests:?}"
            );
        }
    }
    #[test]
    fn the_toolbar_and_the_sounds_page_both_set_the_volumes() {
        let mut harness = playable_with(Settings::default());
        harness.get_by_label("Volume").click();
        harness.run();
        assert!(
            harness
                .query_by_role_and_label(Role::Slider, "Music volume")
                .is_some()
        );
        harness.key_press(egui::Key::Escape);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
        harness.run();
        settings_page(&mut harness, "Sounds");
        for name in ["Music volume", "Note tick volume", "Metronome volume"] {
            assert!(
                harness
                    .query_by_role_and_label(Role::Slider, name)
                    .is_some(),
                "{name}"
            );
        }
    }
    #[test]
    fn with_no_sounds_chosen_playback_clicks_nothing() {
        let mut harness = playable_with(Settings::default());
        assert_eq!(clicks_played(&mut harness), []);
    }
    #[test]
    fn tuned_lanes_step_a_whole_tone_from_the_middle_and_span_an_octave_at_most() {
        let at = |index: u16, width: u16, lanes: u16| {
            let position = charter_core::Position::Lane {
                index: LaneIndex::new(index),
                width: LaneCount::new(width).unwrap(),
            };
            lane_cents(position, lanes)
        };
        let row = |lanes: u16| {
            (0..lanes)
                .map(|lane| at(lane, 1, lanes))
                .collect::<Vec<_>>()
        };
        assert_eq!(row(1), [0]);
        assert_eq!(row(3), [-200, 0, 200]);
        assert_eq!(row(4), [-300, -100, 100, 300]);
        let wide = row(13);
        assert_eq!(
            (wide.first(), wide.get(6), wide.last()),
            (Some(&-600), Some(&0), Some(&600))
        );
        assert_eq!(at(0, 2, 4), -200);
    }
    #[test]
    fn shift_f4_tunes_the_note_ticks_by_lane() {
        let mut harness = playable_with(Settings {
            note_ticks: true,
            tick_sound: Sound::Clap,
            ..Settings::default()
        });
        harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::F4);
        harness.run();
        assert!(harness.state().settings().tuned_lanes);
        let clicks = clicks_played(&mut harness);
        let tones: Vec<_> = clicks.iter().map(|(_, tone)| *tone).collect();
        let clap = |cents| charter_audio::Tick::Clap.pitched(cents);
        assert_eq!(tones, [clap(-300), clap(-100), clap(100), clap(300)]);
        space(&mut harness);
        harness.get_by_label("Tuned lanes").click();
        harness.run();
        assert!(!harness.state().settings().tuned_lanes);
    }
    #[test]
    fn the_tick_sound_plays_on_every_note_even_before_the_cursor() {
        let mut harness = playable_with(Settings {
            note_ticks: true,
            tick_sound: Sound::Beep,
            ..Settings::default()
        });
        harness.key_press(egui::Key::ArrowDown);
        harness.run();
        let clicks = clicks_played(&mut harness);
        let times: Vec<f64> = clicks.iter().map(|(at, _)| *at).collect();
        assert_eq!(times, [0.0, 0.125, 0.25, 0.375]);
        assert!(
            clicks
                .iter()
                .all(|(_, tone)| *tone == charter_audio::Tick::Beep.tone())
        );
    }
    #[test]
    fn the_metronome_plays_on_every_beat_until_the_audio_ends_accenting_each_measure() {
        let mut harness = playable_with(Settings {
            metronome: true,
            metronome_sound: Kit::PureSine,
            ..Settings::default()
        });
        let clicks = clicks_played(&mut harness);
        let times: Vec<f64> = clicks.iter().map(|(at, _)| *at).collect();
        assert_eq!(times, [0.0, 0.5, 1.0, 1.5, 2.0]);
        let tones: Vec<Tone> = clicks.iter().map(|(_, tone)| *tone).collect();
        let (accent, beat) = (Tone::Accent(Kit::PureSine), Tone::Beat(Kit::PureSine));
        assert_eq!(tones, [accent, beat, beat, beat, accent]);
    }
    #[test]
    fn l_loops_the_bar_under_the_cursor_with_the_metronome() {
        let mut harness = listening(Settings::default());
        harness.key_press(egui::Key::L);
        harness.run();
        let (from, looping, clicks) = played(&mut harness);
        assert!(from.abs() < 1e-9);
        assert_eq!(looping, Some((0.0, 2.0)));
        assert!(
            clicks
                .iter()
                .any(|(_, tone)| matches!(tone, Tone::Accent(_)))
        );
        harness.key_press(egui::Key::L);
        harness.run();
        assert_eq!(harness.state_mut().take_requests(), [Request::Pause]);
    }
    #[test]
    fn empty_and_reversed_tick_loops_play_without_repeating() {
        for (first, last) in [
            (Tick::new(960), Tick::new(960)),
            (Tick::new(960), Tick::new(0)),
        ] {
            let mut harness = playable();
            harness.state_mut().play_loop(first, last);
            let Request::Play { looping, .. } = request(&mut harness) else {
                panic!("expected play");
            };
            assert_eq!(looping, None, "{first:?} to {last:?}");
        }
    }
    #[test]
    fn r_switches_to_the_alternate_speed_and_back() {
        let mut harness = listening(Settings::default());
        harness.key_press(egui::Key::R);
        harness.run();
        assert!((harness.state().playback.rate.factor() - 0.6).abs() < 1e-9);
        harness.key_press(egui::Key::R);
        harness.run();
        assert!((harness.state().playback.rate.factor() - 1.0).abs() < 1e-9);
    }
    #[test]
    fn a_metronome_following_the_grid_clicks_between_beats_too() {
        let mut harness = listening(Settings {
            metronome: true,
            metronome_grid: true,
            ..Settings::default()
        });
        harness.key_press(egui::Key::Num6);
        harness.run();
        space(&mut harness);
        let (_, _, clicks) = played(&mut harness);
        let subs = clicks
            .iter()
            .filter(|(_, tone)| matches!(tone, Tone::Sub(_)))
            .count();
        let beats = clicks
            .iter()
            .filter(|(_, tone)| matches!(tone, Tone::Beat(_)))
            .count();
        assert_eq!((beats, subs), (3, 4), "{clicks:?}");
    }
    #[test]
    fn c_toggles_the_metronome_and_shift_c_mutes_it_while_held() {
        let mut harness = listening(Settings {
            metronome: true,
            ..Settings::default()
        });
        space(&mut harness);
        played(&mut harness);
        harness.event(egui::Event::ModifiersChanged(egui::Modifiers::SHIFT));
        harness.key_down_modifiers(egui::Modifiers::SHIFT, egui::Key::C);
        harness.run();
        let (_, _, muted) = played(&mut harness);
        assert!(muted.is_empty(), "{muted:?}");
        harness.key_up_modifiers(egui::Modifiers::SHIFT, egui::Key::C);
        harness.run();
        let (_, _, back) = played(&mut harness);
        assert!(!back.is_empty());
        harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        harness.key_press(egui::Key::C);
        harness.run();
        assert!(!harness.state().settings().metronome);
    }
    #[test]
    fn the_cursor_shows_what_is_heard_by_the_calibrated_latency() {
        let mut harness = playable_with(Settings {
            audio_latency_ms: 250.0.into(),
            ..Settings::default()
        });
        space(&mut harness);
        harness.state_mut().take_requests();
        harness.state_mut().playhead(0.75);
        harness.run();
        assert!(cursor_shows(&harness, "0:1:0"));
    }
    #[test]
    fn the_cursor_scales_calibrated_latency_to_slowed_audio_time() {
        let mut harness = playable_with(Settings {
            audio_latency_ms: 250.0.into(),
            ..Settings::default()
        });
        harness.state_mut().set_rate(Rate::of(0.5));
        space(&mut harness);
        let Request::Play { rate, .. } = request(&mut harness) else {
            panic!("expected playback");
        };
        assert!((rate - 0.5).abs() < f64::EPSILON);

        harness.state_mut().playhead(0.75);
        harness.run();
        assert_eq!(
            harness.state().song.as_ref().unwrap().cursor(),
            Tick::new(1200)
        );
    }
    #[test]
    fn f3_and_f4_turn_the_metronome_and_the_note_ticks_on_and_off() {
        let mut harness = editable_demo();
        harness.key_press(egui::Key::F3);
        harness.key_press(egui::Key::F4);
        harness.run();
        let settings = harness.state().settings();
        assert_eq!((settings.metronome, settings.note_ticks), (true, true));
        harness.key_press(egui::Key::F3);
        harness.run();
        assert!(!harness.state().settings().metronome);
    }
    #[test]
    fn audio_that_moves_under_the_song_stops_playing_until_it_loads_again() {
        let mut harness = playable();
        space(&mut harness);
        harness.state_mut().take_requests();
        harness
            .state_mut()
            .saved(Saved::To(PathBuf::from("/elsewhere/four.json")));
        harness.run();
        harness.state_mut().take_requests();
        space(&mut harness);
        assert_eq!(harness.state_mut().take_requests(), []);
        assert!(harness.query_by_label_contains("still loading").is_some());
    }
    #[test]
    fn opening_another_song_while_playing_pauses_first() {
        let mut harness = playable();
        space(&mut harness);
        harness.state_mut().take_requests();
        harness.state_mut().open_demo();
        harness.run();
        assert_eq!(harness.state_mut().take_requests(), [Request::Pause]);
        assert!(!harness.state().is_playing());
    }
    #[test]
    fn saving_while_playing_saves_and_keeps_playing() {
        let mut harness = playable();
        space(&mut harness);
        harness.state_mut().take_requests();
        harness.key_press(egui::Key::Num1);
        harness.run();
        harness.state_mut().take_requests();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        assert!(matches!(request(&mut harness), Request::Write { .. }));
        assert!(harness.state().is_playing());
    }
    #[test]
    #[expect(
        clippy::non_ascii_literal,
        reason = "the test clicks the Open song button with its ellipsis"
    )]
    fn the_empty_window_opens_a_song_from_a_file() {
        let mut harness = window(Window::new(GAMES));
        harness.get_by_label("Open song…").click();
        harness.run();
        assert_eq!(request(&mut harness), Request::Open);
    }
    #[test]
    fn a_song_whose_audio_cannot_be_found_says_how_to_play_it() {
        let mut harness = editable_demo();
        space(&mut harness);
        assert!(
            harness
                .query_by_label_contains("drop demo.ogg on the window")
                .is_some()
        );
    }
    #[test]
    #[expect(
        clippy::non_ascii_literal,
        reason = "the test clicks the Choose audio button with its ellipsis"
    )]
    fn a_new_song_asks_the_host_to_choose_its_audio() {
        let mut harness = created();
        harness.get_by_label("Choose audio…").click();
        harness.run();
        assert_eq!(request(&mut harness), Request::ChooseAudio);
        harness.state_mut().use_audio(Path::new("/music/track.ogg"));
        harness.run();
        assert_eq!(audio(&harness), "track.ogg");
        assert!(!no_audio(&harness));
    }
    #[test]
    fn the_status_bar_says_how_the_audio_stands() {
        let harness = created();
        assert!(harness.query_by_label("No audio").is_some());
        let loading_harness = four_taps_at("/songs/four.json");
        assert!(loading_harness.query_by_label("Audio loading").is_some());
        let mut ready_harness = playable();
        assert!(ready_harness.query_by_label("Audio ready").is_some());
        space(&mut ready_harness);
        assert!(ready_harness.query_by_label("Playing").is_some());
    }
    #[test]
    #[expect(
        clippy::non_ascii_literal,
        reason = "the test clicks the New song menu label with its ellipsis"
    )]
    fn the_file_menu_and_ctrl_n_start_a_new_song_and_cancel_leaves_it() {
        let mut harness = window(Window::new(GAMES));
        harness.get_by_label("File").click();
        harness.run();
        harness.get_by_label_contains("New song…").click();
        harness.run();
        assert!(dialog_open(&harness));
        harness.get_by_label("Cancel").click();
        harness.run();
        assert!(!dialog_open(&harness));
        assert!(harness.state().song.is_none());
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::N);
        harness.run();
        assert!(dialog_open(&harness));
    }
    #[test]
    #[expect(
        clippy::non_ascii_literal,
        reason = "the test clicks the Choose audio button with its ellipsis"
    )]
    fn the_new_song_dialog_chooses_its_audio_through_the_host() {
        let mut harness = new_song_dialog();
        harness.get_by_label("Choose audio…").click();
        harness.run();
        assert_eq!(request(&mut harness), Request::ChooseAudio);
        harness
            .state_mut()
            .use_audio(Path::new("/music/picked.flac"));
        harness.run();
        assert!(harness.query_by_label("picked.flac").is_some());
        assert!(matches!(request(&mut harness), Request::ReadTags { .. }));
    }
    #[test]
    fn tags_for_audio_the_song_no_longer_plays_are_ignored() {
        let mut harness = editable_four_taps();
        drop_file(&mut harness, "/music/track.ogg");
        read_tags(&mut harness, "/music/other.ogg", "Starlight", "The Lanes");
        assert!(harness.query_by_label_contains(OFFER).is_none());
        assert_eq!(facts(&harness), ("Four".to_owned(), "a".to_owned()));
    }
    #[test]
    fn a_loop_restarts_from_its_beginning_when_its_sound_changes() {
        let mut harness = listening(Settings {
            note_ticks: true,
            ..Settings::default()
        });
        harness.key_press(egui::Key::L);
        harness.run();
        played(&mut harness);
        harness.state_mut().playhead(1.0);
        harness.state_mut().settings.tuned_lanes = true;
        harness.run();
        let requests = harness.state_mut().take_requests();
        assert!(
            requests
                .iter()
                .any(|request| matches!(request, Request::Pause))
        );
        let Some(Request::Play {
            from,
            looping,
            clicks,
            ..
        }) = requests.last()
        else {
            panic!("expected restarted loop: {requests:?}");
        };
        assert!(from.seconds().abs() < 1e-9);
        assert_eq!(
            *looping,
            LoopRegion::new(
                ResolvedTime::from_seconds(0.0).unwrap(),
                ResolvedTime::from_seconds(2.0).unwrap()
            )
        );
        assert!(
            clicks
                .iter()
                .any(|(_, tone)| *tone != charter_audio::Tick::Click.tone())
        );
        harness.run();
        assert_eq!(harness.state_mut().take_requests(), []);
    }
    #[test]
    fn a_paused_loop_stays_paused_through_a_timing_edit() {
        let mut harness = listening(Settings::default());
        harness.key_press(egui::Key::L);
        harness.run();
        harness.state_mut().apply(Action::TogglePlay);
        harness.state_mut().take_requests();
        let bpm = charter_core::Bpm::try_from(100.0).unwrap();
        let edit = charter_timing::Edit::SetBpmAt {
            tick: Tick::new(0),
            bpm,
        };
        harness.state_mut().apply(Action::Timing(edit));
        harness.run();
        assert_eq!(bpm_at_cursor(&harness), Some((0, "100".to_owned())));
        assert_eq!(harness.state_mut().take_requests(), []);
        assert!(!harness.state().is_playing());
    }
    #[test]
    fn a_volume_change_under_a_loops_clicks_keeps_it_looping() {
        let mut harness = listening(Settings::default());
        harness.key_press(egui::Key::L);
        harness.run();
        played(&mut harness);
        harness.state_mut().settings.volumes.music = Gain::from(0.3);
        harness.run();
        let (from, looping, _) = played(&mut harness);
        assert!(from.abs() < 1e-9, "{from}");
        assert_eq!(looping, Some((0.0, 2.0)));
    }
    #[test]
    fn a_sound_change_during_a_snippet_plays_the_snippet_again() {
        let mut harness = playable();
        harness.key_press(egui::Key::PageDown);
        harness.run();
        harness.key_press(egui::Key::Q);
        harness.run();
        harness.state_mut().take_requests();
        harness.state_mut().settings.note_ticks = true;
        harness.run();
        let (from, _, _) = played(&mut harness);
        assert!((from - 1.8).abs() < 1e-9, "{from}");
        harness.state_mut().playhead(2.01);
        harness.run();
        assert_eq!(request(&mut harness), Request::Pause);
    }
    #[test]
    fn a_sound_change_during_an_audition_keeps_its_metronome() {
        let mut harness = playable();
        harness
            .state_mut()
            .apply(Action::Audition(ResolvedTime::from_seconds(0.0).unwrap()));
        harness.state_mut().take_requests();
        harness.state_mut().settings.note_ticks = true;
        harness.run();
        let (_, _, clicks) = played(&mut harness);
        assert!(
            clicks
                .iter()
                .any(|(_, tone)| matches!(tone, Tone::Accent(_))),
            "{clicks:?}"
        );
    }
    #[test]
    fn playback_without_accessible_audio_explains_how_to_supply_it() {
        let mut unsaved = Window::new(PLUGINS);
        unsaved.open(Path::new("four.json"), &four_taps());
        unsaved.path = None;
        unsaved.apply(Action::TogglePlay);
        assert!(
            unsaved
                .message()
                .is_some_and(|message| message.contains("Save the song beside a.ogg"))
        );
        assert_eq!(unsaved.take_requests(), []);

        let mut harness = created();
        let new = harness.state_mut();
        new.apply(Action::TogglePlay);
        assert!(
            new.message()
                .is_some_and(|message| message.contains("drop an audio file"))
        );
        new.no_drops = true;
        new.apply(Action::TogglePlay);
        assert!(
            new.message()
                .is_some_and(|message| message.contains("choose one"))
        );
        assert!(!new.is_playing());
    }
}
