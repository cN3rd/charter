//! The song's audio, loaded off the window's thread and played through kira as the window asks.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use charter_audio::{Clicks, Onset, Peaks, Spectrum, Track};
use charter_ui::{Request, Window};
use kira::sound::PlaybackState;
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle, StaticSoundSettings};
use kira::sound::streaming::{StreamingSoundData, StreamingSoundHandle, StreamingSoundSettings};

use crate::stretch::{self, Pitch, Stretched};
use kira::{AudioManager, AudioManagerSettings, Decibels, DefaultBackend, Frame, Tween};

/// What the loading thread sends: kept peaks as soon as they are read, then the audio, with its
/// peaks unless they came first, then its frequencies, then its onsets, which take longest.
enum Loaded {
    /// Audio envelope available before the remaining analyses finish.
    Peaks(Peaks),
    /// Decoded audio and any envelope not already sent.
    Audio(Result<(Arc<Track>, Option<Peaks>), String>),
    /// Frequency analysis of the decoded audio.
    Spectrum(Spectrum),
    /// Detected transients in the decoded audio.
    Onsets(Arc<[Onset]>),
}

/// The song's audio playing: as it is, at a speed that moves its pitch too, or stretched to keep
/// its pitch at `rate` times its speed.
enum Playing {
    /// Playback whose pitch changes with speed.
    Plain(StaticSoundHandle),
    /// Playback stretched in time without changing pitch.
    Stretched {
        /// Kira's handle for the time-stretched stream.
        sound: StreamingSoundHandle<std::convert::Infallible>,
        /// Ratio of source time to played time.
        rate: f64,
    },
}

impl Playing {
    /// Kira's current playback state, regardless of the sound type.
    fn state(&self) -> PlaybackState {
        match self {
            Self::Plain(sound) => sound.state(),
            Self::Stretched { sound, .. } => sound.state(),
        }
    }

    /// Where playback has got to, in seconds into the song's audio.
    fn position(&self) -> f64 {
        match self {
            Self::Plain(sound) => sound.position(),
            Self::Stretched { sound, rate } => {
                source_position(StretchedSeconds(sound.position()), PlaybackRate(*rate)).get()
            }
        }
    }

    /// Sets the playing sound's volume in decibels.
    fn set_volume(&mut self, volume: Decibels) {
        match self {
            Self::Plain(sound) => sound.set_volume(volume, Tween::default()),
            Self::Stretched { sound, .. } => sound.set_volume(volume, Tween::default()),
        }
    }

    /// Carries on from `to` seconds into the song's audio.
    fn seek_to(&mut self, to: f64) {
        match self {
            Self::Plain(sound) => sound.seek_to(to),
            Self::Stretched { sound, rate } => {
                sound.seek_to(stretched_position(SourceSeconds(to), PlaybackRate(*rate)).get());
            }
        }
    }

    /// Stops either type of playing sound.
    fn stop(&mut self) {
        match self {
            Self::Plain(sound) => sound.stop(Tween::default()),
            Self::Stretched { sound, .. } => sound.stop(Tween::default()),
        }
    }
}

/// `track` at `rate` with `pitch`, `clicks` mixed over it at their volumes in `volumes`, and
/// the volume kira plays it at: the music's without clicks, else none, the clicks carrying it.
fn stretched(
    track: &Arc<Track>,
    (rate, pitch): (f64, Pitch),
    clicks: &[(f64, charter_audio::Tone)],
    volumes: charter_audio::Volumes,
) -> (Option<Stretched>, Option<Decibels>) {
    let volume = clicks.is_empty().then(|| decibels(volumes.music));
    let music = if volume.is_some() {
        charter_audio::Gain::default()
    } else {
        volumes.music
    };
    let clicks = Clicks::new(clicks, volumes, track.rate().get(), rate);
    let stretched = Stretched::new(Arc::clone(track), rate, pitch)
        .map(|stretched| stretched.with_clicks(clicks, music));
    (stretched, volume)
}

/// Seconds on the original audio clock.
#[derive(Clone, Copy, Debug, PartialEq)]
struct SourceSeconds(f64);

impl SourceSeconds {
    /// Raw seconds for audio APIs.
    const fn get(self) -> f64 {
        self.0
    }

    /// Not before the audio starts.
    const fn start(self) -> Self {
        Self(self.0.max(0.0))
    }
}

/// Seconds on the stretched playback stream's clock.
#[derive(Clone, Copy, Debug, PartialEq)]
struct StretchedSeconds(f64);

impl StretchedSeconds {
    /// Raw seconds for kira APIs.
    const fn get(self) -> f64 {
        self.0
    }
}

/// Ratio of source seconds to stretched playback seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
struct PlaybackRate(f64);

impl PlaybackRate {
    /// Raw rate for player handles that keep public floats.
    const fn get(self) -> f64 {
        self.0
    }
}

/// Source seconds corresponding to `stretched_seconds` of stretched playback.
fn source_position(stretched_seconds: StretchedSeconds, rate: PlaybackRate) -> SourceSeconds {
    SourceSeconds(stretched_seconds.0 * rate.0)
}

/// Playback seconds corresponding to `source_seconds` of the original audio.
fn stretched_position(source_seconds: SourceSeconds, rate: PlaybackRate) -> StretchedSeconds {
    StretchedSeconds(source_seconds.0 / rate.0)
}

/// A file being decoded, and where its results arrive.
type Loading = (PathBuf, Receiver<Loaded>);

/// The audio device, the song's audio and the sound playing it.
pub struct Player {
    /// The output, opened on first use and retried after a failure.
    device: Option<Result<AudioManager<DefaultBackend>, String>>,
    /// Reads and decodes an audio file.
    load: fn(&Path) -> Result<Track, String>,
    /// Whether loading happens on the caller's thread, as in tests, rather than on its own.
    inline: bool,
    /// Background decoder job and its results, if audio is loading.
    loading: Option<Loading>,
    /// The audio, decoded and as sound kira plays.
    loaded: Option<(PathBuf, Arc<Track>, StaticSoundData)>,
    /// Audio that failed to load, which is not tried again until the window wants other audio.
    failed: Option<PathBuf>,
    /// Sound currently playing the song, if any.
    playing: Option<Playing>,
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
    /// where playback has got to. Says whether playback or loading needs another frame.
    pub fn update(&mut self, window: &mut Window) -> bool {
        let wanted = window.audio_file();
        let have = self
            .loading
            .as_ref()
            .map(|(path, _)| path)
            .or_else(|| self.loaded.as_ref().map(|(path, ..)| path))
            .or(self.failed.as_ref());
        if have != wanted.as_ref() {
            self.replace_audio(wanted, window);
        }
        self.receive_loaded(window);
        // ponytail: repaint during loading; wake on channel sends if idle redraws matter.
        let loading = self.loading.is_some();
        let Some(sound) = &self.playing else {
            return loading;
        };
        if sound.state() == PlaybackState::Stopped {
            self.playing = None;
            window.playback_ended();
            return loading;
        }
        window.playhead(sound.position());
        true
    }

    /// Starts loading `wanted`, replacing any current audio and ending playback if needed.
    fn replace_audio(&mut self, wanted: Option<PathBuf>, window: &mut Window) {
        if self.playing.is_some() {
            self.stop();
            window.playback_ended();
        }
        self.loaded = None;
        self.failed = None;
        let project = window.song_folder();
        self.loading =
            wanted.map(|path| start_loading(path, project.as_deref(), self.load, self.inline));
    }

    /// Receives every result that has arrived from the current loading job.
    fn receive_loaded(&mut self, window: &mut Window) {
        if let Some((path, receive)) = &self.loading {
            // Everything that has arrived, in order: peaks, the audio, its frequencies, its onsets.
            let mut done = false;
            loop {
                match receive.try_recv() {
                    Ok(Loaded::Peaks(peaks)) => window.waveform_ready(path, Arc::new(peaks)),
                    Ok(Loaded::Onsets(onsets)) => window.onsets_ready(path, onsets),
                    Ok(Loaded::Spectrum(spectrum)) => {
                        window.spectrum_ready(path, Arc::new(spectrum));
                    }
                    Ok(Loaded::Audio(Ok((track, peaks)))) => {
                        window.audio_ready(path, track.seconds());
                        if let Some(peaks) = peaks {
                            window.waveform_ready(path, Arc::new(peaks));
                        }
                        let data = sound(&track);
                        self.loaded = Some((path.clone(), track, data));
                    }
                    Ok(Loaded::Audio(Err(why))) => {
                        window.audio_failed(path, &why);
                        self.failed = Some(path.clone());
                        done = true;
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        if self.loaded.is_none() {
                            window.audio_failed(path, "loading stopped");
                            self.failed = Some(path.clone());
                        }
                        done = true;
                        break;
                    }
                }
            }
            if done {
                self.loading = None;
            }
        }
    }

    /// Does what the window asked of playback; other requests are left to the caller.
    pub fn answer(&mut self, request: &Request, window: &mut Window) {
        match request {
            Request::Play {
                from,
                rate,
                clicks,
                volumes,
                looping,
                keep_pitch,
            } => {
                let (from, rate, volumes, looping, keep_pitch) =
                    (*from, *rate, *volumes, *looping, *keep_pitch);
                self.stop();
                let Some((path, track, data)) = &self.loaded else {
                    window.playback_ended();
                    return;
                };
                let pitch = if keep_pitch && stretch::keeps_pitch(rate) {
                    Some(Pitch::Kept)
                } else if (rate - 1.0).abs() > 1e-9 && !clicks.is_empty() {
                    // Clicks keep their own speed and pitch, so they go in after the song's.
                    Some(Pitch::FollowsSpeed)
                } else {
                    None
                };
                if let Some(pitch) = pitch {
                    let (stretched, volume) = stretched(track, (rate, pitch), clicks, volumes);
                    let path = path.clone();
                    self.play_stretched(&path, stretched, (from, rate, looping, volume), window);
                    return;
                }
                let settings = StaticSoundSettings::new()
                    .start_position(from.max(0.0))
                    .playback_rate(rate);
                let settings = match looping {
                    Some((start, end)) if end > start => settings.loop_region(start.max(0.0)..end),
                    _ => settings,
                };
                // Clicks are mixed in with the music at its volume; alone, kira sets it, so it can
                // change as it plays.
                let settings = if clicks.is_empty() {
                    settings.volume(decibels(volumes.music))
                } else {
                    settings
                };
                // ponytail: clicks mix into a copy of the whole track on each play, tens of
                // milliseconds for a long song; play them as sounds of their own if that shows.
                let data = if clicks.is_empty() {
                    data.clone()
                } else {
                    sound(&charter_audio::mix(track, clicks, volumes))
                };
                let (path, data) = (path.clone(), data.with_settings(settings));
                let played = self
                    .device()
                    .and_then(|device| device.play(data).map_err(|error| error.to_string()));
                match played {
                    Ok(sound) => self.playing = Some(Playing::Plain(sound)),
                    Err(why) => {
                        window.audio_failed(&path, &why);
                        window.playback_ended();
                    }
                }
            }
            Request::Volume(gain) => {
                if let Some(sound) = &mut self.playing {
                    sound.set_volume(decibels(*gain));
                }
            }
            Request::Preview { clicks, volumes } => {
                if let Some(mut sound) = self.previewing.take() {
                    sound.stop(Tween::default());
                }
                let data = sound(&preview(clicks, *volumes));
                // A preview that cannot play says nothing: playing the song says why.
                self.previewing = self.device().ok().and_then(|device| device.play(data).ok());
            }
            Request::Pause => self.stop(),
            Request::Seek(to) => {
                if let Some(sound) = &mut self.playing {
                    sound.seek_to(to.max(0.0));
                }
            }
            // Stretched audio changes speed by playing again, which the window asks for.
            Request::Rate(rate) => {
                if let Some(Playing::Plain(sound)) = &mut self.playing {
                    sound.set_playback_rate(*rate, Tween::default());
                }
            }
            Request::Write { .. }
            | Request::SaveAs { .. }
            | Request::Open
            | Request::OpenPath { .. }
            | Request::Snapshot { .. }
            | Request::ChooseSnapshotFolder
            | Request::Autosave { .. }
            | Request::DropAutosave
            | Request::CopyAudio { .. }
            | Request::ChooseAudio
            | Request::ChooseArtwork
            | Request::DecodeArtwork { .. }
            | Request::ReadArtwork { .. }
            | Request::ChooseTimingFile
            | Request::ReadTags { .. }
            | Request::FindTiming { .. }
            | Request::DownloadWeights => {}
        }
    }

    /// Stops playback and releases its handle.
    fn stop(&mut self) {
        if let Some(mut sound) = self.playing.take() {
            sound.stop();
        }
    }

    /// Plays `stretched`, the audio of `path` at `rate` times its speed, from `from` seconds,
    /// looping between `looping`'s seconds when given, at `volume` unless the clicks mixed in
    /// carry it.
    fn play_stretched(
        &mut self,
        path: &Path,
        stretched: Option<Stretched>,
        (from, rate, looping, volume): (f64, f64, Option<(f64, f64)>, Option<Decibels>),
        window: &mut Window,
    ) {
        let Some(stretched) = stretched else {
            window.audio_failed(path, "its sample rate cannot be played at another speed");
            window.playback_ended();
            return;
        };
        let rate = PlaybackRate(rate);
        let settings = StreamingSoundSettings::new()
            .start_position(stretched_position(SourceSeconds(from).start(), rate).get());
        let settings = match volume {
            Some(volume) => settings.volume(volume),
            None => settings,
        };
        let settings = match looping {
            Some((start, end)) if end > start => settings.loop_region(
                stretched_position(SourceSeconds(start).start(), rate).get()
                    ..stretched_position(SourceSeconds(end), rate).get(),
            ),
            _ => settings,
        };
        let data = StreamingSoundData::from_decoder(stretched).with_settings(settings);
        let played = self
            .device()
            .and_then(|device| device.play(data).map_err(|error| error.to_string()));
        match played {
            Ok(sound) => {
                self.playing = Some(Playing::Stretched {
                    sound,
                    rate: rate.get(),
                });
            }
            Err(why) => {
                window.audio_failed(path, &why);
                window.playback_ended();
            }
        }
    }

    /// The output device, opened when needed and retried after a failure.
    fn device(&mut self) -> Result<&mut AudioManager<DefaultBackend>, String> {
        let device = get_or_retry(&mut self.device, || {
            AudioManager::<DefaultBackend>::new(AudioManagerSettings::default())
                .map_err(|error| error.to_string())
        });
        device.map_err(|why| why.clone())
    }
}

/// Reuses a working device or retries `init` after a previous failure.
fn get_or_retry<T, E>(
    device: &mut Option<Result<T, E>>,
    init: impl FnOnce() -> Result<T, E>,
) -> Result<&mut T, &mut E> {
    if device.as_ref().is_some_and(Result::is_err) {
        *device = None;
    }
    device.get_or_insert_with(init).as_mut()
}

/// `clicks` mixed over silence long enough to hold them.
fn preview(clicks: &[(f64, charter_audio::Tone)], volumes: charter_audio::Volumes) -> Track {
    const RATE: u32 = 44_100;
    let last = clicks.iter().map(|(at, _)| *at).fold(0.0, f64::max);
    let frames = preview_frames((last + 0.5).clamp(0.0, 10.0) * f64::from(RATE));
    charter_audio::mix(&Track::silence(RATE, frames), clicks, volumes)
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "preview duration is clamped to ten seconds"
)]
/// Truncates the clamped preview frame count to a buffer length.
const fn preview_frames(frames: f64) -> usize {
    frames as usize
}

/// `gain` as kira's volume, silent at its floor.
fn decibels(gain: charter_audio::Gain) -> Decibels {
    Decibels(gain.decibels(Decibels::SILENCE.0))
}

/// Reads and decodes the file at `path`.
fn load(path: &Path) -> Result<Track, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let extension = path.extension().and_then(|extension| extension.to_str());
    Track::decode(bytes, extension).map_err(|error| error.to_string())
}

/// Starts the audio loading job for `path`, either inline for tests or on a worker thread.
fn start_loading(
    path: PathBuf,
    project: Option<&Path>,
    load: fn(&Path) -> Result<Track, String>,
    inline: bool,
) -> Loading {
    let (send, receive) = std::sync::mpsc::channel();
    let file = PathBuf::clone(&path);
    let (cache, onset_cache) = if inline {
        (None, None)
    } else {
        let at = |kind: &str| cache_file(project, &file, kind);
        (at("peaks"), at("onsets"))
    };
    // The peaks are read or worked out beside the decoding, off the window's thread.
    let summarize = move || summarize(load, &file, (cache, onset_cache), &send);
    if inline {
        summarize();
    } else {
        std::thread::spawn(summarize);
    }
    (path, receive)
}

/// Reads `file` with `load` and sends what the window needs of it through `send`, as the loading
/// thread does: peaks kept at the first of `caches` as soon as they are read, then the audio with
/// its peaks unless they came first, then its frequencies, then its onsets, kept at the second.
fn summarize(
    load: fn(&Path) -> Result<Track, String>,
    file: &Path,
    (cache, onset_cache): (Option<PathBuf>, Option<PathBuf>),
    send: &std::sync::mpsc::Sender<Loaded>,
) {
    let disk = crate::disk::Real;
    let source = cache.as_ref().and_then(|_| std::fs::read(file).ok());
    let source = source.map(|bytes| charter_audio::fingerprint(&bytes));
    let kept = cache
        .as_deref()
        .zip(source)
        .and_then(|(cache, source)| kept_peaks(&disk, cache, source));
    let had = kept.is_some();
    if let Some(peaks) = kept {
        drop(send.send(Loaded::Peaks(peaks)));
    }
    let audio = load(file).map(|track| {
        let peaks = (!had).then(|| Peaks::of(&track));
        if let (Some(cache), Some(source), Some(peaks)) = (&cache, source, &peaks) {
            keep_peaks(&disk, cache, source, peaks);
        }
        (Arc::new(track), peaks)
    });
    let track = audio.as_ref().ok().map(|(track, _)| Arc::clone(track));
    drop(send.send(Loaded::Audio(audio)));
    let Some(track) = track else {
        return;
    };
    // ponytail: worked out on every load, a few tenths of a second a song; keep it in the cache
    // beside the peaks if that shows.
    drop(send.send(Loaded::Spectrum(Spectrum::of(&track))));
    let cached_onsets = onset_cache
        .as_deref()
        .zip(source)
        .and_then(|(onset_file, source)| kept_onsets(&disk, onset_file, source));
    let onsets = cached_onsets.unwrap_or_else(|| {
        let found: Arc<[Onset]> = charter_audio::onsets(&track).into();
        if let (Some(onset_file), Some(source)) = (&onset_cache, source) {
            keep_onsets(&disk, onset_file, source, &found);
        }
        found
    });
    drop(send.send(Loaded::Onsets(onsets)));
}

/// Where what is worked out from `audio`, of the `kind` such as `peaks`, is kept: in the `cache`
/// folder of the song's project folder `project`, when the audio is in it, so nothing is written
/// among the user's own music.
fn cache_file(project: Option<&Path>, audio: &Path, kind: &str) -> Option<PathBuf> {
    let project = project?;
    let relative = audio.strip_prefix(project).ok()?;
    if relative.file_name().is_none()
        || !relative
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return None;
    }
    let mut name = relative.as_os_str().to_owned();
    name.push(".");
    name.push(kind);
    Some(project.join("cache").join(name))
}

/// The peaks kept at `cache` for the audio whose fingerprint is `source`, if they are there,
/// whole and of it.
fn kept_peaks(disk: &dyn crate::disk::Disk, cache: &Path, source: u64) -> Option<Peaks> {
    let bytes = disk.read(cache).ok()?;
    Peaks::from_bytes(&bytes, source)
}

/// Keeps `peaks` at `cache` for next time. A cache that cannot be written is not worth saying so:
/// the peaks are only worked out again.
fn keep_peaks(disk: &dyn crate::disk::Disk, cache: &Path, source: u64, peaks: &Peaks) {
    if let Some(folder) = cache.parent() {
        drop(disk.make_folder(folder));
    }
    drop(crate::disk::write_whole(
        disk,
        cache,
        &peaks.to_bytes(source),
    ));
}

/// The onsets kept at `cache` for the audio whose fingerprint is `source`, if they are there,
/// whole and of it.
fn kept_onsets(disk: &dyn crate::disk::Disk, cache: &Path, source: u64) -> Option<Arc<[Onset]>> {
    let bytes = disk.read(cache).ok()?;
    charter_audio::onsets_from_bytes(&bytes, source)
}

/// Keeps `onsets` at `cache` for next time; a cache that cannot be written only costs finding
/// them again.
fn keep_onsets(disk: &dyn crate::disk::Disk, cache: &Path, source: u64, onsets: &[Onset]) {
    if let Some(folder) = cache.parent() {
        drop(disk.make_folder(folder));
    }
    drop(crate::disk::write_whole(
        disk,
        cache,
        &charter_audio::onsets_to_bytes(onsets, source),
    ));
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

    #[expect(
        clippy::as_conversions,
        clippy::cast_precision_loss,
        reason = "test-only sample phase approximation"
    )]
    fn offset_as_f32(offset: usize) -> f32 {
        offset as f32
    }

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
        let track = preview(&clicks, charter_audio::Volumes::default());
        assert!((track.seconds() - 2.0).abs() < 1e-3);
        let heard = |from: usize, to: usize| {
            track.frames()[from..to]
                .iter()
                .any(|frame| frame[0].abs() > 0.01)
        };
        assert!(heard(0, 400));
        assert!(heard(66_150, 66_600));
    }

    #[test]
    fn preview_clicks_respect_the_metronome_volume_without_a_device() {
        let clicks = [(
            0.0,
            charter_audio::Tone::Beat(charter_audio::Kit::StickClick),
        )];
        let full = preview(&clicks, charter_audio::Volumes::default());
        let muted = preview(
            &clicks,
            charter_audio::Volumes {
                metronome: charter_audio::Gain::from(0.0),
                ..charter_audio::Volumes::default()
            },
        );
        assert!(full.frames().iter().any(|frame| frame[0].abs() > 0.01));
        assert!(
            muted
                .frames()
                .iter()
                .all(|frame| frame.iter().all(|sample| sample.abs() < f32::EPSILON))
        );
        assert!((decibels(charter_audio::Gain::from(0.5)).0 + 6.0206).abs() < 0.001);
        assert!((decibels(charter_audio::Gain::from(0.0)).0 - Decibels::SILENCE.0).abs() < 0.001);
    }

    #[test]
    fn peaks_are_kept_in_the_project_folder_only_for_audio_beside_the_song() {
        let project = Some(Path::new("/s/Song"));
        assert_eq!(
            cache_file(project, Path::new("/s/Song/a.ogg"), "peaks"),
            Some(PathBuf::from("/s/Song/cache/a.ogg.peaks"))
        );
        assert_eq!(
            cache_file(project, Path::new("/music/a.ogg"), "peaks"),
            None
        );
        assert_eq!(cache_file(None, Path::new("/s/Song/a.ogg"), "peaks"), None);
    }

    #[test]
    fn nested_project_audio_has_its_own_cache_without_writing_outside_the_project() {
        let project = Some(Path::new("/s/Song"));
        assert_eq!(
            cache_file(project, Path::new("/s/Song/assets/a.ogg"), "peaks"),
            Some(PathBuf::from("/s/Song/cache/assets/a.ogg.peaks"))
        );
        assert_eq!(
            cache_file(project, Path::new("/s/Song/alternate/a.ogg"), "peaks"),
            Some(PathBuf::from("/s/Song/cache/alternate/a.ogg.peaks"))
        );
        assert_eq!(
            cache_file(project, Path::new("/s/Song/../other/a.ogg"), "peaks"),
            None
        );
    }

    #[test]
    fn peaks_worked_out_once_are_read_back_for_the_same_audio_and_not_for_other() {
        use crate::disk::memory::Memory;
        let disk = Memory::with(&[("/s/a.ogg", b"audio")]);
        let cache = Path::new("/s/cache/a.ogg.peaks");
        let source = charter_audio::fingerprint(b"audio");
        assert_eq!(kept_peaks(&disk, cache, source), None);
        let peaks = Peaks::of(&Track::silence(1000, 2000));
        keep_peaks(&disk, cache, source, &peaks);
        assert_eq!(kept_peaks(&disk, cache, source), Some(peaks));
        assert_eq!(kept_peaks(&disk, cache, source + 1), None);
    }

    #[test]
    fn audio_that_loads_gives_the_window_its_onsets_after_it() {
        let mut player = player(clicking);
        let mut window = song_window();
        player.update(&mut window);
        let onsets = window.onsets().expect("onsets");
        let strong = onsets.iter().filter(|onset| onset.strength > 0.2).count();
        assert_eq!(strong, 2, "{onsets:?}");
        assert!((onsets[0].seconds - 0.5).abs() < 0.002);
    }

    #[test]
    fn onsets_kept_are_read_back_for_the_same_audio() {
        use crate::disk::memory::Memory;
        let disk = Memory::default();
        let cache = Path::new("/s/cache/a.ogg.onsets");
        assert_eq!(kept_onsets(&disk, cache, 3), None);
        let found = [charter_audio::Onset {
            seconds: 0.5,
            strength: 1.0,
        }];
        keep_onsets(&disk, cache, 3, &found);
        assert_eq!(kept_onsets(&disk, cache, 3).as_deref(), Some(&found[..]));
        assert_eq!(kept_onsets(&disk, cache, 4), None);
    }

    #[test]
    fn damaged_or_unreadable_summaries_are_recomputed_instead_of_reused() {
        use crate::disk::memory::Memory;
        let cache = Path::new("/s/cache/a.wav.peaks");
        let onset_cache = Path::new("/s/cache/a.wav.onsets");
        let peaks = Peaks::of(&Track::silence(1000, 2000));
        let onsets = [Onset {
            seconds: 0.25,
            strength: 0.8,
        }];
        let disk = Memory::with(&[
            ("/s/cache/a.wav.peaks", b"truncated"),
            ("/s/cache/a.wav.onsets", b"truncated"),
        ]);
        assert_eq!(kept_peaks(&disk, cache, 7), None);
        assert_eq!(kept_onsets(&disk, onset_cache, 7), None);
        keep_peaks(&disk, cache, 7, &peaks);
        keep_onsets(&disk, onset_cache, 7, &onsets);
        assert_eq!(kept_peaks(&disk, cache, 7), Some(peaks));
        assert_eq!(
            kept_onsets(&disk, onset_cache, 7).as_deref(),
            Some(&onsets[..])
        );
        assert_eq!(kept_peaks(&disk, cache, 8), None);
        assert_eq!(kept_onsets(&disk, onset_cache, 8), None);

        let disk = Memory {
            unreadable: vec![cache.to_path_buf(), onset_cache.to_path_buf()],
            ..disk
        };
        assert_eq!(kept_peaks(&disk, cache, 7), None);
        assert_eq!(kept_onsets(&disk, onset_cache, 7), None);
    }

    #[test]
    fn a_cache_write_failure_leaves_previous_summaries_intact() {
        use crate::disk::memory::Memory;
        let peak_path = Path::new("/s/cache/a.wav.peaks");
        let onset_path = Path::new("/s/cache/a.wav.onsets");
        let peaks = Peaks::of(&Track::silence(1000, 2000));
        let onsets = [Onset {
            seconds: 0.5,
            strength: 1.0,
        }];
        let disk = Memory::default();
        keep_peaks(&disk, peak_path, 4, &peaks);
        keep_onsets(&disk, onset_path, 4, &onsets);
        let disk = Memory {
            stuck: true,
            ..disk
        };
        keep_peaks(&disk, peak_path, 5, &peaks);
        keep_onsets(&disk, onset_path, 5, &onsets);
        assert_eq!(kept_peaks(&disk, peak_path, 4), Some(peaks));
        assert_eq!(
            kept_onsets(&disk, onset_path, 4).as_deref(),
            Some(&onsets[..])
        );
        assert_eq!(disk.get("/s/cache/a.wav.peaks.partial"), None);
        assert_eq!(disk.get("/s/cache/a.wav.onsets.partial"), None);
    }

    #[test]
    fn stretched_positions_seeks_and_loop_bounds_use_the_audio_clock() {
        // At 1.6x, the stretched sound's clock runs for 2.5s over 4s of source.
        let fast = PlaybackRate(1.6);
        assert!((source_position(StretchedSeconds(2.5), fast).get() - 4.0).abs() < 1e-12);
        assert!((stretched_position(SourceSeconds(4.0), fast).get() - 2.5).abs() < 1e-12);
        assert!((stretched_position(SourceSeconds(8.0), fast).get() - 5.0).abs() < 1e-12);
        // The seek and loop start are clamped in source seconds before conversion.
        assert!(stretched_position(SourceSeconds(-2.0).start(), fast).get() < 1e-12);
        let slow = PlaybackRate(0.75);
        assert!((source_position(StretchedSeconds(8.0), slow).get() - 6.0).abs() < 1e-12);
        assert!((stretched_position(SourceSeconds(6.0), slow).get() - 8.0).abs() < 1e-12);
        assert!(
            (stretched_position(SourceSeconds(13.7), PlaybackRate(1.37)).get() - 10.0).abs()
                < 1e-12
        );
    }

    #[expect(clippy::unnecessary_wraps, reason = "matches the loader signature")]
    fn clicking(_: &Path) -> Result<Track, String> {
        let mut frames = vec![[0.0_f32; 2]; 44_100 * 2];
        for start in [22_050, 44_100] {
            for (offset, frame) in frames.iter_mut().skip(start).take(1500).enumerate() {
                let t = offset_as_f32(offset) / 44_100.0;
                let sample = (std::f32::consts::TAU * 2000.0 * t).sin() * (-t / 0.008).exp();
                *frame = [sample * 0.8; 2];
            }
        }
        Ok(Track::from_frames(44_100, frames))
    }

    static FAILED_LOADS: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn an_audio_device_failure_is_retried_on_the_next_request() {
        let mut device: Option<Result<u8, &str>> = None;
        let mut attempts = 0;
        get_or_retry(&mut device, || {
            attempts += 1;
            Err("device not found")
        })
        .unwrap_err();
        let retried = get_or_retry(&mut device, || {
            attempts += 1;
            Ok(7)
        })
        .map(|device| *device);
        assert_eq!(retried.ok(), Some(7));
        assert_eq!(attempts, 2);
    }

    fn failing(_: &Path) -> Result<Track, String> {
        FAILED_LOADS.fetch_add(1, Ordering::SeqCst);
        Err("no such file".to_owned())
    }

    #[expect(clippy::unnecessary_wraps, reason = "matches the loader signature")]
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
        assert!(window.strip().is_none());
        player.update(&mut window);
        player.update(&mut window);
        assert!(window.strip().is_some());
    }

    #[test]
    fn loading_audio_requests_another_frame_without_user_input() {
        let mut player = player(silent);
        let mut window = song_window();
        let (send, receive) = std::sync::mpsc::channel();
        player.loading = Some((PathBuf::from("/s/a.wav"), receive));
        assert!(player.update(&mut window));
        drop(send);
    }

    #[test]
    fn a_worker_that_stops_before_audio_reports_failure_and_can_load_another_song() {
        let mut player = player(silent);
        let mut window = song_window();
        let (send, receive) = std::sync::mpsc::channel();
        player.loading = Some((PathBuf::from("/s/a.wav"), receive));
        drop(send);
        assert!(!player.update(&mut window));
        assert!(window.message().unwrap().contains("loading stopped"));
        assert_eq!(player.failed.as_deref(), Some(Path::new("/s/a.wav")));
        assert!(player.loaded.is_none());

        window.open(
            Path::new("/s/other.json"),
            std::str::from_utf8(SONG)
                .unwrap()
                .replace("a.wav", "b.wav")
                .as_bytes(),
        );
        player.update(&mut window);
        assert_eq!(
            player.loaded.as_ref().map(|(path, ..)| path.as_path()),
            Some(Path::new("/s/b.wav"))
        );
        assert_eq!(player.failed, None);
    }

    #[test]
    fn a_worker_that_stops_after_audio_keeps_the_usable_track() {
        let mut player = player(silent);
        let mut window = song_window();
        let (send, receive) = std::sync::mpsc::channel();
        send.send(Loaded::Audio(Ok((
            Arc::new(Track::silence(1000, 2000)),
            None,
        ))))
        .unwrap();
        drop(send);
        player.loading = Some((PathBuf::from("/s/a.wav"), receive));
        assert!(!player.update(&mut window));
        assert_eq!(
            player.loaded.as_ref().map(|(path, ..)| path.as_path()),
            Some(Path::new("/s/a.wav"))
        );
        assert!(player.failed.is_none());
        assert_eq!(window.message(), None);
    }

    #[test]
    fn cached_peaks_arrive_before_a_failed_decode_and_failure_stops_loading() {
        let mut player = player(silent);
        let mut window = song_window();
        let (send, receive) = std::sync::mpsc::channel();
        send.send(Loaded::Peaks(Peaks::of(&Track::silence(1000, 2000))))
            .unwrap();
        send.send(Loaded::Audio(Err("invalid audio".to_owned())))
            .unwrap();
        drop(send);
        player.loading = Some((PathBuf::from("/s/a.wav"), receive));
        assert!(!player.update(&mut window));
        assert!(window.strip().is_some());
        assert!(window.message().unwrap().contains("invalid audio"));
        assert_eq!(player.failed.as_deref(), Some(Path::new("/s/a.wav")));
        assert!(player.loaded.is_none());
        assert!(player.loading.is_none());
    }

    #[test]
    fn messages_for_audio_no_longer_requested_do_not_replace_the_current_song() {
        let mut player = player(silent);
        let mut window = song_window();
        let (send, receive) = std::sync::mpsc::channel();
        send.send(Loaded::Audio(Ok((
            Arc::new(Track::silence(1000, 9000)),
            None,
        ))))
        .unwrap();
        player.loading = Some((PathBuf::from("/s/a.wav"), receive));
        window.open(
            Path::new("/s/other.json"),
            std::str::from_utf8(SONG)
                .unwrap()
                .replace("a.wav", "b.wav")
                .as_bytes(),
        );
        player.update(&mut window);
        assert_eq!(
            player.loaded.as_ref().map(|(path, ..)| path.as_path()),
            Some(Path::new("/s/b.wav"))
        );
        assert_eq!(
            player.loaded.as_ref().map(|(_, _, data)| data.frames.len()),
            Some(2000)
        );
        drop(send);
    }

    #[test]
    fn play_with_nothing_loaded_ends_playback_at_once() {
        let mut player = player(silent);
        let mut window = song_window();
        let play = Request::Play {
            from: 0.0,
            rate: 1.0,
            clicks: Vec::new(),
            volumes: charter_audio::Volumes::default(),
            looping: None,
            keep_pitch: true,
        };
        player.answer(&play, &mut window);
        assert!(player.playing.is_none());
        assert!(!window.is_playing());
        assert!(player.device.is_none());
    }

    #[test]
    fn idle_transport_requests_never_open_the_audio_device() {
        let mut player = player(silent);
        let mut window = song_window();
        for request in [
            Request::Seek(-4.0),
            Request::Volume(charter_audio::Gain::default()),
            Request::Rate(1.5),
            Request::Pause,
        ] {
            player.answer(&request, &mut window);
        }
        assert!(player.device.is_none());
        assert!(player.playing.is_none());
    }

    #[test]
    #[expect(
        clippy::little_endian_bytes,
        reason = "the WAV and RIFF test fixture uses little-endian fields"
    )]
    fn a_track_becomes_sound_of_the_same_frames_and_rate() {
        // A one-frame 16-bit stereo WAV at 8000 Hz.
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&40_u32.to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        for field in [1_u16, 2] {
            wav.extend_from_slice(&field.to_le_bytes());
        }
        wav.extend_from_slice(&8000_u32.to_le_bytes());
        wav.extend_from_slice(&32000_u32.to_le_bytes());
        for field in [4_u16, 16] {
            wav.extend_from_slice(&field.to_le_bytes());
        }
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&4_u32.to_le_bytes());
        for sample in [16384_i16, -16384] {
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
