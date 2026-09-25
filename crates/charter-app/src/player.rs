//! The song's audio, loaded off the window's thread and played through kira as the window asks.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use charter_audio::Track;
use charter_ui::{Request, Window};
use kira::sound::PlaybackState;
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle, StaticSoundSettings};
use kira::{AudioManager, AudioManagerSettings, DefaultBackend, Frame, Tween};

/// A file being decoded, and where its result arrives.
type Loading = (PathBuf, Receiver<Result<StaticSoundData, String>>);

/// The audio device, the song's audio and the sound playing it.
#[derive(Default)]
pub struct Player {
    /// The output, opened on first use; the error when it would not open.
    device: Option<Result<AudioManager<DefaultBackend>, String>>,
    loading: Option<Loading>,
    loaded: Option<(PathBuf, StaticSoundData)>,
    playing: Option<StaticSoundHandle>,
}

impl std::fmt::Debug for Player {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Player")
            .field("loading", &self.loading.as_ref().map(|(path, _)| path))
            .field("loaded", &self.loaded.as_ref().map(|(path, _)| path))
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
            .or(self.loaded.as_ref().map(|(path, _)| path));
        if have != wanted.as_ref() {
            self.stop();
            self.loaded = None;
            self.loading = wanted.map(|path| {
                let (send, receive) = std::sync::mpsc::channel();
                let file = path.clone();
                std::thread::spawn(move || {
                    // The window has moved on if nobody is listening; that is fine.
                    let _ = send.send(load(&file));
                });
                (path, receive)
            });
        }
        if let Some((path, receive)) = &self.loading {
            match receive.try_recv() {
                Ok(Ok(data)) => {
                    window.audio_ready(path, data.duration().as_secs_f64());
                    self.loaded = Some((path.clone(), data));
                    self.loading = None;
                }
                Ok(Err(why)) => {
                    window.audio_failed(path, &why);
                    self.loading = None;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    window.audio_failed(path, "loading stopped");
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
            Request::Play { from, rate } => {
                self.stop();
                let Some((path, data)) = &self.loaded else {
                    return;
                };
                let settings = StaticSoundSettings::new()
                    .start_position(from.max(0.0))
                    .playback_rate(rate);
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

/// Reads and decodes the file at `path` into sound kira can play.
fn load(path: &Path) -> Result<StaticSoundData, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let extension = path.extension().and_then(|extension| extension.to_str());
    let track = Track::decode(bytes, extension).map_err(|error| error.to_string())?;
    Ok(sound(&track))
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
