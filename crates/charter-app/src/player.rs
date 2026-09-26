//! The song's audio, loaded off the window's thread and played through kira as the window asks.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use charter_audio::{Peaks, Track};
use charter_ui::{Request, Window};
use kira::sound::PlaybackState;
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle, StaticSoundSettings};
use kira::{AudioManager, AudioManagerSettings, DefaultBackend, Frame, Tween};

/// A file being decoded, and where its result arrives.
type Loading = (PathBuf, Receiver<Result<(Track, Peaks), String>>);

/// The audio device, the song's audio and the sound playing it.
pub struct Player {
    /// The output, opened on first use; the error when it would not open.
    device: Option<Result<AudioManager<DefaultBackend>, String>>,
    /// Reads and decodes an audio file.
    load: fn(&Path) -> Result<Track, String>,
    /// Whether loading happens on the caller's thread, as in tests, rather than on its own.
    inline: bool,
    loading: Option<Loading>,
    /// The audio, decoded and as sound kira plays.
    loaded: Option<(PathBuf, Track, StaticSoundData)>,
    /// Audio that failed to load, which is not tried again until the window wants other audio.
    failed: Option<PathBuf>,
    playing: Option<StaticSoundHandle>,
    /// A metronome sound being previewed, beside whatever else plays.
    previewing: Option<StaticSoundHandle>,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            device: None,
            load,
            inline: false,
            loading: None,
            loaded: None,
            failed: None,
            playing: None,
            previewing: None,
        }
    }
}

impl std::fmt::Debug for Player {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Player")
            .field("loading", &self.loading.as_ref().map(|(path, _)| path))
            .field("loaded", &self.loaded.as_ref().map(|(path, ..)| path))
            .field("failed", &self.failed)
            .field("playing", &self.playing.is_some())
            .finish_non_exhaustive()
    }
}

impl Player {
    /// Loads the audio the window wants when it changes, tells the window how loading went, and
    /// where playback has got to. Says whether audio is playing, so the host keeps drawing.
    pub fn update(&mut self, window: &mut Window) -> bool {
        let wanted = window.audio_file();
        let have = self
            .loading
            .as_ref()
            .map(|(path, _)| path)
            .or(self.loaded.as_ref().map(|(path, ..)| path))
            .or(self.failed.as_ref());
        if have != wanted.as_ref() {
            if self.playing.is_some() {
                self.stop();
                window.playback_ended();
            }
            self.loaded = None;
            self.failed = None;
            let (load, inline) = (self.load, self.inline);
            self.loading = wanted.map(|path| {
                let (send, receive) = std::sync::mpsc::channel();
                let file = path.clone();
                // The peaks are worked out beside the decoding, off the window's thread.
                let summarized = move |file: &Path| {
                    load(file).map(|track| {
                        let peaks = Peaks::of(&track);
                        (track, peaks)
                    })
                };
                // The window has moved on if nobody is listening; that is fine.
                if inline {
                    let _ = send.send(summarized(&file));
                } else {
                    std::thread::spawn(move || {
                        let _ = send.send(summarized(&file));
                    });
                }
                (path, receive)
            });
        }
        if let Some((path, receive)) = &self.loading {
            match receive.try_recv() {
                Ok(Ok((track, peaks))) => {
                    window.audio_ready(path, track.seconds());
                    window.waveform_ready(path, Arc::new(peaks));
                    let data = sound(&track);
                    self.loaded = Some((path.clone(), track, data));
                    self.loading = None;
                }
                Ok(Err(why)) => {
                    window.audio_failed(path, &why);
                    self.failed = Some(path.clone());
                    self.loading = None;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    window.audio_failed(path, "loading stopped");
                    self.failed = Some(path.clone());
                    self.loading = None;
                }
            }
        }
        let Some(sound) = &self.playing else {
            return false;
        };
        if sound.state() == PlaybackState::Stopped {
            self.playing = None;
            window.playback_ended();
            return false;
        }
        window.playhead(sound.position());
        true
    }

    /// Does what the window asked of playback; other requests are left to the caller.
    pub fn answer(&mut self, request: &Request, window: &mut Window) {
        match *request {
            Request::Play {
                from,
                rate,
                ref clicks,
            } => {
                self.stop();
                let Some((path, track, data)) = &self.loaded else {
                    window.playback_ended();
                    return;
                };
                let settings = StaticSoundSettings::new()
                    .start_position(from.max(0.0))
                    .playback_rate(rate);
                // ponytail: clicks mix into a copy of the whole track on each play, tens of
                // milliseconds for a long song; play them as sounds of their own if that shows.
                let data = if clicks.is_empty() {
                    data.clone()
                } else {
                    sound(&charter_audio::mix(track, clicks))
                };
                let (path, data) = (path.clone(), data.with_settings(settings));
                let played = self
                    .device()
                    .and_then(|device| device.play(data).map_err(|error| error.to_string()));
                match played {
                    Ok(sound) => self.playing = Some(sound),
                    Err(why) => {
                        window.audio_failed(&path, &why);
                        window.playback_ended();
                    }
                }
            }
            Request::Preview { ref clicks } => {
                if let Some(mut sound) = self.previewing.take() {
                    sound.stop(Tween::default());
                }
                let data = sound(&preview(clicks));
                // A preview that cannot play says nothing: playing the song says why.
                self.previewing = self.device().ok().and_then(|device| device.play(data).ok());
            }
            Request::Pause => self.stop(),
            Request::Seek(to) => {
                if let Some(sound) = &mut self.playing {
                    sound.seek_to(to.max(0.0));
                }
            }
            Request::Rate(rate) => {
                if let Some(sound) = &mut self.playing {
                    sound.set_playback_rate(rate, Tween::default());
                }
            }
            _ => {}
        }
    }

    fn stop(&mut self) {
        if let Some(mut sound) = self.playing.take() {
            sound.stop(Tween::default());
        }
    }

    /// The output device, opened the first time it is needed.
    fn device(&mut self) -> Result<&mut AudioManager<DefaultBackend>, String> {
        let device = self.device.get_or_insert_with(|| {
            AudioManager::<DefaultBackend>::new(AudioManagerSettings::default())
                .map_err(|error| error.to_string())
        });
        device.as_mut().map_err(|why| why.clone())
    }
}

/// `clicks` mixed over silence long enough to hold them.
fn preview(clicks: &[(f64, charter_audio::Tone)]) -> Track {
    const RATE: u32 = 44_100;
    let last = clicks.iter().map(|(at, _)| *at).fold(0.0, f64::max);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // a few seconds
    let frames = ((last + 0.5).clamp(0.0, 10.0) * f64::from(RATE)) as usize;
    charter_audio::mix(&Track::silence(RATE, frames), clicks)
}

/// Reads and decodes the file at `path`.
fn load(path: &Path) -> Result<Track, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let extension = path.extension().and_then(|extension| extension.to_str());
    Track::decode(bytes, extension).map_err(|error| error.to_string())
}

/// `track` as sound data kira plays.
fn sound(track: &Track) -> StaticSoundData {
    let frames: Arc<[Frame]> = track
        .frames()
        .iter()
        .map(|&[left, right]| Frame { left, right })
        .collect();
    StaticSoundData {
        sample_rate: track.rate().get(),
        frames,
        settings: StaticSoundSettings::default(),
        slice: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A song in `sm`, which opens read-only without its plugin, naming `a.wav`.
    const SONG: &[u8] = br#"{ "format_version": 0, "ruleset": { "id": "sm", "version": "0" },
        "metadata": { "title": "t", "artist": "a", "audio": "a.wav" },
        "timing": { "offset_seconds": 0.0, "events": [ { "tick": 0, "kind": { "bpm": 120.0 } } ] },
        "charts": [ { "name": "Easy", "lanes": 4 } ] }"#;

    fn song_window() -> Window {
        let mut window = Window::new(&[]);
        window.open(Path::new("/s/song.json"), SONG);
        window
    }

    #[test]
    fn a_preview_holds_every_click_and_a_moment_after() {
        let kit = charter_audio::Kit::StickClick;
        let clicks = [
            (0.0, charter_audio::Tone::Accent(kit)),
            (1.5, charter_audio::Tone::Beat(kit)),
        ];
        let track = preview(&clicks);
        assert!((track.seconds() - 2.0).abs() < 1e-3);
        let heard = |from: usize, to: usize| {
            track.frames()[from..to]
                .iter()
                .any(|frame| frame[0].abs() > 0.01)
        };
        assert!(heard(0, 400));
        assert!(heard(66_150, 66_600));
    }

    static FAILED_LOADS: AtomicUsize = AtomicUsize::new(0);

    fn failing(_: &Path) -> Result<Track, String> {
        FAILED_LOADS.fetch_add(1, Ordering::SeqCst);
        Err("no such file".to_owned())
    }

    #[allow(clippy::unnecessary_wraps)] // a loader's signature
    fn silent(_: &Path) -> Result<Track, String> {
        Ok(Track::silence(1000, 2000))
    }

    fn player(load: fn(&Path) -> Result<Track, String>) -> Player {
        Player {
            load,
            inline: true,
            ..Player::default()
        }
    }

    #[test]
    fn audio_that_fails_to_load_is_reported_once_and_not_loaded_again() {
        let mut player = player(failing);
        let mut window = song_window();
        let before = FAILED_LOADS.load(Ordering::SeqCst);
        for _ in 0..5 {
            player.update(&mut window);
        }
        assert_eq!(FAILED_LOADS.load(Ordering::SeqCst) - before, 1);
        assert!(window.message().unwrap().contains("no such file"));
    }

    #[test]
    fn audio_that_loads_is_kept_as_sound_ready_to_play() {
        let mut player = player(silent);
        let mut window = song_window();
        player.update(&mut window);
        player.update(&mut window);
        let (path, _, data) = player.loaded.as_ref().unwrap();
        assert_eq!(path, Path::new("/s/a.wav"));
        assert_eq!(data.frames.len(), 2000);
        assert_eq!(window.message(), None);
    }

    #[test]
    fn audio_that_loads_gives_the_window_its_waveform() {
        let mut player = player(silent);
        let mut window = song_window();
        assert!(!window.shows_waveform());
        player.update(&mut window);
        player.update(&mut window);
        assert!(window.shows_waveform());
    }

    #[test]
    fn play_with_nothing_loaded_ends_playback_at_once() {
        let mut player = player(silent);
        let mut window = song_window();
        let play = Request::Play {
            from: 0.0,
            rate: 1.0,
            clicks: Vec::new(),
        };
        player.answer(&play, &mut window);
        assert!(player.playing.is_none());
        assert!(!window.is_playing());
    }

    #[test]
    fn a_track_becomes_sound_of_the_same_frames_and_rate() {
        // A one-frame 16-bit stereo WAV at 8000 Hz.
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&40u32.to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        for field in [1u16, 2] {
            wav.extend_from_slice(&field.to_le_bytes());
        }
        wav.extend_from_slice(&8000u32.to_le_bytes());
        wav.extend_from_slice(&32000u32.to_le_bytes());
        for field in [4u16, 16] {
            wav.extend_from_slice(&field.to_le_bytes());
        }
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&4u32.to_le_bytes());
        for sample in [16384i16, -16384] {
            wav.extend_from_slice(&sample.to_le_bytes());
        }
        let track = Track::decode(wav, Some("wav")).unwrap();
        let data = sound(&track);
        assert_eq!(data.sample_rate, 8000);
        assert_eq!(data.frames.len(), 1);
        assert!((data.frames[0].left - 0.5).abs() < 1e-3);
        assert!((data.frames[0].right + 0.5).abs() < 1e-3);
    }
}
