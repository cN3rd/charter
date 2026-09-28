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
    track: Arc<Track>,
    rate: f64,
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
            let end = (self.fed + FEED).min(frames.len());
            let chunk = frames.get(self.fed..end).unwrap_or_default().as_flattened();
            let taken = source.push(chunk);
            if taken == 0 {
                break;
            }
            self.fed += taken;
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
        #[allow(clippy::cast_precision_loss)] // frames of a song, far inside f64
        let source = self.track.frames().len() as f64;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // a frame count
        let frames = (source / self.rate).ceil() as usize;
        frames
    }

    fn decode(&mut self) -> Result<Vec<Frame>, Infallible> {
        self.feed();
        let mut out = vec![0.0; CHUNK * 2];
        self.engine.processor.process(&mut out);
        let (frames, _) = out.as_chunks::<2>();
        Ok(frames
            .iter()
            .map(|&[left, right]| Frame { left, right })
            .collect())
    }

    fn seek(&mut self, index: usize) -> Result<usize, Infallible> {
        #[allow(clippy::cast_precision_loss)] // a frame index, far inside f64
        let at = index as f64 * self.rate;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // a frame index
        let at = (at.round() as usize).min(self.track.frames().len());
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

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;

    /// `seconds` of a sine at `hertz`, silent for the first `quiet` seconds.
    fn tone(hertz: f64, seconds: f64, quiet: f64) -> Arc<Track> {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let frames = (seconds * f64::from(RATE)) as u32;
        let samples = (0..frames).map(|n| {
            let t = f64::from(n) / f64::from(RATE);
            #[allow(clippy::cast_possible_truncation)]
            let sample = if t < quiet {
                0.0
            } else {
                (0.5 * (std::f64::consts::TAU * hertz * t).sin()) as f32
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
        #[allow(clippy::cast_precision_loss)]
        let hertz = rising as f64 * f64::from(RATE) / samples.len() as f64;
        hertz
    }

    #[test]
    fn pitch_is_kept_between_a_quarter_and_twice_the_speed_but_not_at_its_own() {
        assert!(keeps_pitch(0.5) && keeps_pitch(0.25) && keeps_pitch(2.0));
        assert!(!keeps_pitch(1.0) && !keeps_pitch(0.1) && !keeps_pitch(4.0));
    }

    #[test]
    fn half_speed_lasts_twice_as_long_at_the_same_pitch() {
        let mut stretched = Stretched::new(tone(440.0, 3.0, 0.0), 0.5).unwrap();
        assert_eq!(stretched.num_frames(), 6 * 44_100);
        let samples = decoded(&mut stretched, 4 * 44_100);
        let middle = samples.get(44_100..3 * 44_100).unwrap();
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
        let loud = samples.get(22_050..).unwrap();
        assert!(loud.iter().any(|sample| sample.abs() > 0.1));
        let heard = hertz(loud);
        assert!((heard - 440.0).abs() < 10.0, "{heard} Hz");
    }
}
