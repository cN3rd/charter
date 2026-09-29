//! Slowed or sped-up playback that keeps the song's pitch: its audio stretched in time as kira
//! plays it, by timestretch's wide-range keylock, a phase vocoder locked across the whole
//! spectrum. kira pulls the stretched frames from [`Stretched`] on its decoding thread, so the
//! stretching runs ahead of playback rather than before it.

use std::convert::Infallible;
use std::sync::Arc;

use charter_audio::Track;
use kira::Frame;
use kira::sound::streaming::Decoder;
use timestretch::engine::{Engine, EngineConfig, EngineHandles, EngineProfile};

/// The slowest and fastest the stretcher keeps the pitch at; past them, pitch follows speed.
const SLOWEST: f64 = 0.25;
/// Fastest playback rate that the pitch-preserving stretcher supports.
const FASTEST: f64 = 2.0;
/// Frames each call to [`Decoder::decode`] makes.
const CHUNK: usize = 1024;
/// Source frames handed to the stretcher at a time.
const FEED: usize = 4096;

/// Whether playing at `rate` times the audio's speed can keep its pitch: at any other speed than
/// its own, within what the stretcher reaches.
#[must_use]
pub fn keeps_pitch(rate: f64) -> bool {
    (rate - 1.0).abs() > 1e-9 && (SLOWEST..=FASTEST).contains(&rate)
}

// ponytail: stretched audio plays 0.6 to 2.3 dB quieter than the source; make it up with gain if
// switching speeds is heard as a drop.
/// A track played at `rate` times its speed with its pitch kept, as frames of the stretched
/// audio: frame `n` sounds source frame `n * rate`.
pub struct Stretched {
    /// Source audio kept alive for the streaming decoder.
    track: Arc<Track>,
    /// Source frames played per output frame.
    rate: f64,
    /// Timestretch source and processor state.
    engine: EngineHandles,
    /// Source frames handed to the stretcher so far.
    fed: usize,
    /// Whether the stretcher has been told the source has ended.
    finished: bool,
}

impl Stretched {
    /// `track` at `rate`, from its start, unless the stretcher cannot take its sample rate.
    #[must_use]
    pub fn new(track: Arc<Track>, rate: f64) -> Option<Self> {
        let engine = engine(&track, rate)?;
        Some(Self {
            track,
            rate,
            engine,
            fed: 0,
            finished: false,
        })
    }

    /// Hands the stretcher source frames until it holds what the next chunk needs, and tells it
    /// the source has ended once it has all of them.
    fn feed(&mut self) {
        let frames = self.track.frames();
        let source = &mut self.engine.source;
        while self.fed < frames.len()
            && source.occupied_frames() < source.demand_hint(CHUNK, self.rate)
        {
            let end = self.fed.saturating_add(FEED).min(frames.len());
            let chunk = frames.get(self.fed..end).unwrap_or_default().as_flattened();
            let taken = source.push(chunk);
            if taken == 0 {
                break;
            }
            self.fed = self.fed.saturating_add(taken);
        }
        if self.fed >= frames.len() && !self.finished {
            self.finished = source.finish();
        }
    }
}

/// A stretcher for `track`'s stereo audio at `rate`, unless it cannot take the sample rate.
fn engine(track: &Track, rate: f64) -> Option<EngineHandles> {
    let mut handles = Engine::build(EngineConfig {
        sample_rate: track.rate().get(),
        channels: 2,
        profile: EngineProfile::WideKeylock,
        initial_tempo_rate: rate,
        ..EngineConfig::default()
    })
    .ok()?;
    handles.source.set_track_position(0);
    Some(handles)
}

impl Decoder for Stretched {
    type Error = Infallible;

    fn sample_rate(&self) -> u32 {
        self.track.rate().get()
    }

    fn num_frames(&self) -> usize {
        let source = frame_index_as_f64(self.track.frames().len());
        let frames = frame_index_from_f64((source / self.rate).ceil());
        frames
    }

    fn decode(&mut self) -> Result<Vec<Frame>, Infallible> {
        self.feed();
        let mut out = [0.0; CHUNK * 2];
        self.engine.processor.process(&mut out);
        let (frames, _) = out.as_chunks::<2>();
        Ok(frames
            .iter()
            .map(|&[left, right]| Frame { left, right })
            .collect())
    }

    fn seek(&mut self, index: usize) -> Result<usize, Infallible> {
        let at = frame_index_as_f64(index) * self.rate;
        let at = frame_index_from_f64(at.round()).min(self.track.frames().len());
        // A fresh stretcher from the source frame played there: timestretch seeks by rebuilding.
        if let Some(mut engine) = engine(&self.track, self.rate) {
            engine
                .source
                .set_track_position(u64::try_from(at).unwrap_or(u64::MAX));
            self.engine = engine;
        }
        self.fed = at;
        self.finished = false;
        Ok(index)
    }
}

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "the audio frame index is approximate when converted to seconds"
)]
/// Converts a source frame index to floating point for rate scaling.
fn frame_index_as_f64(index: usize) -> f64 {
    index as f64
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a calculated frame index is clamped to the source length"
)]
/// Truncates a nonnegative calculated frame index to an array offset.
fn frame_index_from_f64(index: f64) -> usize {
    index as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;

    /// `seconds` of a sine at `hertz`, silent for the first `quiet` seconds.
    fn tone(hertz: f64, seconds: f64, quiet: f64) -> Arc<Track> {
        let frames = u32::try_from(frame_index_from_f64(seconds * f64::from(RATE))).unwrap();
        let samples = (0..frames).map(|n| {
            let t = f64::from(n) / f64::from(RATE);
            let sample = if t < quiet {
                0.0
            } else {
                sample_as_f32(0.5 * (std::f64::consts::TAU * hertz * t).sin())
            };
            [sample; 2]
        });
        Arc::new(Track::from_frames(RATE, samples.collect()))
    }

    fn decoded(stretched: &mut Stretched, frames: usize) -> Vec<f32> {
        let mut left = Vec::new();
        while left.len() < frames {
            left.extend(stretched.decode().unwrap().iter().map(|frame| frame.left));
        }
        left.truncate(frames);
        left
    }

    /// How often a second `samples` cross zero going up.
    fn hertz(samples: &[f32]) -> f64 {
        let rising = samples
            .windows(2)
            .filter(|pair| matches!(pair, [a, b] if *a < 0.0 && *b >= 0.0))
            .count();
        let hertz =
            frame_index_as_f64(rising) * f64::from(RATE) / frame_index_as_f64(samples.len());
        hertz
    }

    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        reason = "test waveform fits f32"
    )]
    fn sample_as_f32(sample: f64) -> f32 {
        sample as f32
    }

    #[test]
    fn pitch_is_kept_between_a_quarter_and_twice_the_speed_but_not_at_its_own() {
        assert!(keeps_pitch(0.5) && keeps_pitch(0.25) && keeps_pitch(2.0));
        assert!(!keeps_pitch(1.0) && !keeps_pitch(0.1) && !keeps_pitch(4.0));
    }

    #[test]
    fn stretched_audio_keeps_the_source_sample_rate() {
        let stretched = Stretched::new(tone(440.0, 0.1, 0.0), 0.5).unwrap();
        assert_eq!(stretched.sample_rate(), RATE);
    }

    #[test]
    fn half_speed_lasts_twice_as_long_at_the_same_pitch() {
        let mut stretched = Stretched::new(tone(440.0, 3.0, 0.0), 0.5).unwrap();
        assert_eq!(stretched.num_frames(), 6 * 44_100);
        let samples = decoded(&mut stretched, 4 * 44_100);
        let middle = &samples[44_100..3 * 44_100];
        let heard = hertz(middle);
        assert!((heard - 440.0).abs() < 5.0, "{heard} Hz");
    }

    #[test]
    fn a_seek_plays_on_from_the_source_frame_its_frame_sounds() {
        // Silent for 2 s, then a tone: at half speed, frame 5 s sounds the tone at 2.5 s.
        let mut stretched = Stretched::new(tone(440.0, 4.0, 2.0), 0.5).unwrap();
        let quiet = decoded(&mut stretched, 44_100);
        assert!(quiet.iter().all(|sample| sample.abs() < 1e-3));
        assert_eq!(stretched.seek(5 * 44_100).unwrap(), 5 * 44_100);
        let samples = decoded(&mut stretched, 44_100);
        let loud = &samples[22_050..];
        assert!(loud.iter().any(|sample| sample.abs() > 0.1));
        let heard = hertz(loud);
        assert!((heard - 440.0).abs() < 10.0, "{heard} Hz");
    }

    #[test]
    fn seeking_to_or_past_the_end_does_not_replay_the_source() {
        let mut stretched = Stretched::new(tone(440.0, 1.0, 0.0), 0.5).unwrap();
        assert_eq!(stretched.seek(stretched.num_frames()).unwrap(), 88_200);
        assert!(
            decoded(&mut stretched, CHUNK)
                .iter()
                .all(|sample| sample.abs() < 1e-3)
        );
        assert_eq!(stretched.seek(usize::MAX).unwrap(), usize::MAX);
        assert!(
            decoded(&mut stretched, CHUNK)
                .iter()
                .all(|sample| sample.abs() < 1e-3)
        );
    }

    #[test]
    fn the_extreme_pitch_preserving_rates_keep_the_tone_and_expected_length() {
        for (rate, length) in [(SLOWEST, 529_200), (FASTEST, 66_150)] {
            let mut stretched = Stretched::new(tone(440.0, 3.0, 0.0), rate).unwrap();
            assert_eq!(stretched.num_frames(), length);
            let samples = decoded(&mut stretched, 44_100);
            let middle = &samples[22_050..];
            let heard = hertz(middle);
            assert!((heard - 440.0).abs() < 15.0, "{rate}: {heard} Hz");
        }
    }
}
