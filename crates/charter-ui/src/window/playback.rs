//! Playback and audio-rate state changes.

use super::{
    Finding, Loaded, NO_AUDIO, Path, Rate, Request, Sounds, Tick, Ui, Window, clicks, grow, latency,
};

impl Window {
    /// Handles this window operation.
    pub fn audio_ready(&mut self, file: &Path, seconds: f64) {
        self.playback.audio = Some((file.to_path_buf(), Loaded::Ready(seconds)));
    }
    /// Handles this window operation.
    pub fn audio_failed(&mut self, file: &Path, why: &str) {
        self.playback.audio = Some((file.to_path_buf(), Loaded::Failed));
        self.failure = Some(format!("{} cannot play: {why}", file.display()));
    }
    /// Handles this window operation.
    pub fn playhead(&mut self, seconds: f64) {
        if !self.playback.playing {
            return;
        }
        // What plays now is heard a moment later: the cursor shows what is heard.
        let seconds =
            seconds - self.settings.audio_latency_ms * self.playback.rate.factor() / 1000.0;
        let tapped = seconds - self.input_latency();
        if let Some(song) = &mut self.song {
            let tick = song.timing().latest_tick_by(seconds);
            song.jump_to(tick);
            if let Some(why) = grow(song, &self.typing.holding, &self.typing.pressed, tapped) {
                self.failure = Some(why);
            }
        }
    }
    /// Handles this private window operation.
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
        latency::Beat::of(timing.tempo_at(first), per_measure)
    }
    /// Handles this private window operation.
    pub(super) fn input_latency(&self) -> f64 {
        self.settings.input_latency_ms * self.playback.rate.factor() / 1000.0
    }
    /// Handles this private window operation.
    #[expect(
        clippy::non_ascii_literal,
        reason = "the audio-analysis status shows an ellipsis while running"
    )]
    pub(super) fn audio_state(&self) -> String {
        match self.dialogs.finding {
            Finding::Downloading(share) => {
                return format!("Downloading the Beat This! weights, {:.0}%", share * 100.0);
            }
            Finding::Running => return "Finding the timing…".to_owned(),
            Finding::Idle | Finding::Asking => {}
        }
        match (self.audio_file(), self.loaded()) {
            (None, _) => "No audio",
            (Some(_), Some(Loaded::Ready(_))) => "Audio ready",
            (Some(_), Some(Loaded::Failed)) => "Audio failed",
            (Some(_), None) => "Audio loading",
        }
        .to_owned()
    }
    /// Handles this private window operation.
    pub(super) fn loaded(&self) -> Option<&Loaded> {
        let wanted = self.audio_file()?;
        self.playback
            .audio
            .as_ref()
            .filter(|(file, _)| *file == wanted)
            .map(|(_, loaded)| loaded)
    }
    /// Handles this private window operation.
    pub(super) fn follow_volumes(&mut self, ui: &Ui) {
        let (Some((played, mixed)), true) = (self.playback.played, self.playback.playing) else {
            return;
        };
        let wanted = self.settings.volumes;
        if wanted == played {
            return;
        }
        if !mixed {
            if wanted.music != played.music {
                self.requests.push(Request::Volume(wanted.music));
            }
            self.playback.played = Some((wanted, false));
        } else if !ui.input(|input| input.pointer.any_down()) {
            self.toggle_play();
            self.toggle_play();
        }
    }
    /// Handles this private window operation.
    pub(super) fn audition(&mut self, from: f64) {
        self.playback.looping = None;
        self.start(from, None, true);
    }
    /// Handles this private window operation.
    pub(super) fn toggle_loop(&mut self, from: Tick, to: Tick) {
        if self.playback.looping == Some((from, to)) && self.playback.playing {
            self.playback.looping = None;
            self.playback.playing = false;
            self.requests.push(Request::Pause);
            return;
        }
        self.playback.looping = Some((from, to));
        self.play_loop();
    }
    /// Handles this private window operation.
    pub(super) fn play_loop(&mut self) {
        let (Some(song), Some((from, to))) = (&self.song, self.playback.looping) else {
            return;
        };
        let timing = song.timing();
        let span = (timing.time_at(from).seconds(), timing.time_at(to).seconds());
        self.start(span.0, Some(span), true);
    }
    /// Handles this private window operation.
    pub(super) fn replay(&mut self) {
        if !self.playback.playing {
            return;
        }
        if self.playback.looping.is_some() {
            self.play_loop();
        } else if let Some(song) = &self.song {
            let from = song.timing().time_at(song.cursor()).seconds();
            self.start(from, None, false);
        }
    }
    /// Handles this private window operation.
    pub(super) fn start(&mut self, from: f64, looping: Option<(f64, f64)>, metronome: bool) {
        let (Some(song), Some(&Loaded::Ready(seconds))) = (&self.song, self.loaded()) else {
            return;
        };
        if self.playback.playing {
            self.requests.push(Request::Pause);
        }
        let from = from.max(0.0);
        let until = looping.map_or(seconds, |(_, end)| end.min(seconds));
        // From the song's start, so a seek back while playing still ticks.
        let clicks = clicks(
            song,
            &self.settings,
            (self.settings.metronome || metronome) && !self.playback.muted,
            (0.0, until),
            self.sync.grid(),
        );
        let volumes = self.settings.volumes;
        self.playback.playing = true;
        self.playback.played = Some((volumes, !clicks.is_empty()));
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
    /// Handles this private window operation.
    pub(super) fn toggle_rate(&mut self) {
        if let Some(usual) = self.playback.usual_rate.take() {
            self.set_rate(usual);
        } else {
            self.playback.usual_rate = Some(self.playback.rate);
            self.set_rate(Rate::of(self.settings.rate_b));
        }
    }
    /// Handles this private window operation.
    #[expect(
        clippy::non_ascii_literal,
        reason = "the playback error names the Choose audio menu label with its ellipsis"
    )]
    pub(super) fn toggle_play(&mut self) {
        if std::mem::take(&mut self.playback.playing) {
            self.requests.push(Request::Pause);
            return;
        }
        let Some(song) = &self.song else {
            return;
        };
        let from = song.timing().time_at(song.cursor()).seconds();
        match self.loaded() {
            Some(&Loaded::Ready(_)) => {
                self.playback.looping = None;
                self.start(from, None, false);
            }
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
    /// Handles this private window operation.
    pub(super) fn set_rate(&mut self, rate: Rate) {
        self.playback.rate = rate;
        // Audio that keeps its pitch, or played keeping it, is stretched for its speed, and so is
        // audio under clicks, which keep their own; a new speed plays it again.
        let clicked = self.playback.played.is_some_and(|(_, mixed)| mixed);
        if self.playback.playing
            && (self.settings.keep_pitch || self.playback.played_pitch || clicked)
        {
            self.replay();
        } else if self.playback.playing {
            self.requests.push(Request::Rate(rate.factor()));
        }
    }
}
