//! Slowed or sped-up playback: the song's audio stretched in time as kira plays it, keeping its
//! pitch by timestretch's wide-range keylock, a phase vocoder locked across the whole spectrum,
//! or letting it follow the speed like tape. The metronome and note ticks are mixed in after, at
//! their own speed and pitch. kira pulls the frames from [`Stretched`] on its decoding thread, so
//! the stretching runs ahead of playback rather than before it.

use std::convert::Infallible;
use std::sync::{Arc, Mutex};

use charter_audio::{Clicks, Gain, Track};
use kira::Frame;
use kira::sound::streaming::Decoder;
use timestretch::engine::{Engine, EngineConfig, EngineHandles, EngineProfile};

/// The slowest and fastest the stretcher keeps the pitch at; past them, pitch follows speed.
const SLOWEST: f64 = 0.1;
/// Fastest playback rate that the pitch-preserving stretcher supports.
const FASTEST: f64 = 2.0;
/// Whether stretched audio keeps its pitch or lets it follow its speed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pitch {
    /// The song sounds at its own pitch, by the wide-range keylock.
    Kept,
    /// The song's pitch rises and falls with its speed, like tape.
    FollowsSpeed,
}

impl Pitch {
    /// The engine profile playing it.
    const fn profile(self) -> EngineProfile {
        match self {
            Self::Kept => EngineProfile::WideKeylock,
            Self::FollowsSpeed => EngineProfile::Tape,
        }
    }

    /// The slowest and fastest one engine plays. timestretch's wide head clamps its ratio at 2
    /// whatever rate it is asked for, so slower rates chain engines, each playing the last one's
    /// output; tape reaches the engine's own range.
    const fn stage_range(self) -> (f64, f64) {
        match self {
            Self::Kept => (0.5, 2.0),
            Self::FollowsSpeed => (0.25, 4.0),
        }
    }
}
/// Frames each call to [`Decoder::decode`] makes, and each stage renders at a time.
const CHUNK: usize = 1024;
/// Source frames handed to the first stage at a time.
const FEED: usize = 4096;
/// Where the calibration click sits in its otherwise silent source, in frames: past the
/// stretcher's lookahead, so the click is heard whole.
const CLICK_AT: usize = 4096;
/// How long the calibration click lasts, in frames.
const CLICK_LENGTH: usize = 256;

/// Whether playing at `rate` times the audio's speed can keep its pitch: at any other speed than
/// its own, within what the stretcher reaches.
#[must_use]
pub(crate) fn keeps_pitch(rate: f64) -> bool {
    (rate - 1.0).abs() > 1e-9 && (SLOWEST..=FASTEST).contains(&rate)
}

/// The rates of the engines that play at `rate` between them, each within an engine's reach.
fn stage_rates(rate: f64, pitch: Pitch) -> Vec<f64> {
    let (slowest, fastest) = pitch.stage_range();
    let mut rates = Vec::new();
    let mut left = rate;
    while left < slowest - 1e-9 && rates.len() < 8 {
        rates.push(slowest);
        left /= slowest;
    }
    while left > fastest + 1e-9 && rates.len() < 8 {
        rates.push(fastest);
        left /= fastest;
    }
    rates.push(left);
    rates
}

/// Engines in a row, the first fed source frames and each later one fed the one before's output.
struct Chain {
    /// The engines, first to last.
    stages: Vec<EngineHandles>,
    /// The tempo rate each engine plays at.
    rates: Vec<f64>,
    /// Output of each engine but the last, rendered and not yet taken by the next.
    carried: Vec<Vec<f32>>,
}

impl Chain {
    /// Engines playing `rates` in turn at `sample_rate` with `pitch`, unless an engine cannot
    /// take it.
    fn new(rates: &[f64], sample_rate: u32, pitch: Pitch) -> Option<Self> {
        let stages = rates
            .iter()
            .map(|&rate| {
                let mut handles = Engine::build(EngineConfig {
                    sample_rate,
                    channels: 2,
                    profile: pitch.profile(),
                    initial_tempo_rate: rate,
                    ..EngineConfig::default()
                })
                .ok()?;
                handles.source.set_track_position(0);
                Some(handles)
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            carried: vec![Vec::new(); stages.len()],
            stages,
            rates: rates.to_vec(),
        })
    }

    /// Renders the next [`CHUNK`] frames, interleaved, into `out`, taking source frames from
    /// `frames` from `fed` on and moving `fed` past those taken. Tells the first engine the
    /// source has ended once `finished` is not set and it has all of them.
    fn render(
        &mut self,
        frames: &[[f32; 2]],
        fed: &mut usize,
        finished: &mut bool,
        out: &mut [f32],
    ) {
        let last = self.stages.len().saturating_sub(1);
        self.fill(last, frames, fed, finished);
        if let Some(stage) = self.stages.get_mut(last) {
            stage.processor.process(out);
        }
    }

    /// Gives engine `index` what its next chunk needs: source frames for the first, the output
    /// of the one before for the rest.
    fn fill(&mut self, index: usize, frames: &[[f32; 2]], fed: &mut usize, finished: &mut bool) {
        let Some(&rate) = self.rates.get(index) else {
            return;
        };
        if index == 0 {
            let Some(stage) = self.stages.first_mut() else {
                return;
            };
            let source = &mut stage.source;
            while *fed < frames.len() && source.occupied_frames() < source.demand_hint(CHUNK, rate)
            {
                let end = fed.saturating_add(FEED).min(frames.len());
                let taken = source.push(frames.get(*fed..end).unwrap_or_default().as_flattened());
                if taken == 0 {
                    break;
                }
                *fed = fed.saturating_add(taken);
            }
            if *fed >= frames.len() && !*finished {
                *finished = source.finish();
            }
            return;
        }
        let before = index.saturating_sub(1);
        loop {
            let (Some(downstream), Some(waiting)) =
                (self.stages.get(index), self.carried.get(index))
            else {
                return;
            };
            if downstream.source.occupied_frames() >= downstream.source.demand_hint(CHUNK, rate) {
                return;
            }
            if waiting.is_empty() {
                self.fill(before, frames, fed, finished);
                let mut chunk = [0.0; CHUNK * 2];
                if let Some(upstream) = self.stages.get_mut(before) {
                    upstream.processor.process(&mut chunk);
                }
                if let Some(rendered) = self.carried.get_mut(index) {
                    rendered.extend_from_slice(&chunk);
                }
            }
            let (Some(stage), Some(carried)) =
                (self.stages.get_mut(index), self.carried.get_mut(index))
            else {
                return;
            };
            let taken = stage.source.push(carried);
            if taken == 0 {
                return;
            }
            carried.drain(..taken.saturating_mul(2).min(carried.len()));
        }
    }
}

// ponytail: one entry per speed and sample rate played, a handful a session; bound it if speeds
// ever become continuous.
/// A measured lead and the speed, sample rate, and pitch used to measure it.
type CachedLead = ((u64, u32, Pitch), f64);
/// Leads measured so far, by rate, sample rate and pitch, so a speed played again starts at once.
static LEADS: Mutex<Vec<CachedLead>> = Mutex::new(Vec::new());

/// The lead of the stretcher at `rate` with `pitch` for audio at `sample_rate`, measured once.
fn known_lead(rate: f64, sample_rate: u32, pitch: Pitch) -> Option<f64> {
    let key = (rate.to_bits(), sample_rate, pitch);
    let known = LEADS.lock().ok().and_then(|leads| {
        leads
            .iter()
            .find(|(found, _)| *found == key)
            .map(|&(_, lead)| lead)
    });
    if known.is_some() {
        return known;
    }
    let measured = lead(&stage_rates(rate, pitch), sample_rate, pitch)?;
    if let Ok(mut leads) = LEADS.lock() {
        leads.push((key, measured));
    }
    Some(measured)
}

/// How far ahead of where its output says the stretcher at `rates` plays, in source frames:
/// found by playing a click through a fresh chain like it and hearing where it lands, since the
/// phase vocoder's window and the chaining move the audio by an amount timestretch does not
/// report.
fn lead(rates: &[f64], sample_rate: u32, pitch: Pitch) -> Option<f64> {
    let mut chain = Chain::new(rates, sample_rate, pitch)?;
    let rate: f64 = rates.iter().product();
    let mut click = vec![[0.0; 2]; CLICK_AT.saturating_mul(2)];
    // A burst of 2 kHz, where transients are heard: the vocoder moves the lowest band apart.
    let hertz = 2000.0 / f64::from(sample_rate);
    for (offset, frame) in click
        .iter_mut()
        .skip(CLICK_AT)
        .take(CLICK_LENGTH)
        .enumerate()
    {
        let at = frame_index_as_f64(offset);
        let envelope =
            0.5 - 0.5 * (at / frame_index_as_f64(CLICK_LENGTH) * std::f64::consts::TAU).cos();
        *frame = [narrow(envelope * (at * hertz * std::f64::consts::TAU).sin()); 2];
    }
    let frames = frame_index_from_f64((frame_index_as_f64(CLICK_AT) * 1.5 / rate).ceil());
    let (mut fed, mut finished) = (0, false);
    let (mut energy, mut weighted) = (0.0, 0.0);
    let mut out = [0.0; CHUNK * 2];
    let mut at = 0;
    while at < frames {
        chain.render(&click, &mut fed, &mut finished, &mut out);
        for [left, _] in out.as_chunks::<2>().0 {
            let power = f64::from(*left).powi(2);
            energy += power;
            weighted += power * frame_index_as_f64(at);
            at = at.saturating_add(1);
        }
    }
    let heard = weighted / energy;
    heard.is_finite().then(|| {
        frame_index_as_f64(CLICK_AT) + frame_index_as_f64(CLICK_LENGTH) / 2.0 - heard * rate
    })
}

// ponytail: stretched audio plays 0.6 to 2.3 dB quieter than the source; make it up with gain if
// switching speeds is heard as a drop.
/// A track played at `rate` times its speed, as frames of the stretched audio: frame `n` sounds
/// source frame `n * rate`, with any clicks mixed in at their own speed.
pub(crate) struct Stretched {
    /// Source audio kept alive for the streaming decoder.
    track: Arc<Track>,
    /// Source frames played per output frame.
    rate: f64,
    /// Whether the song keeps its pitch.
    pitch: Pitch,
    /// The metronome and note ticks, placed in output frames.
    clicks: Clicks,
    /// The song's gain under the clicks.
    music: f32,
    /// The output frame the next decoded frame is.
    heard: usize,
    /// The engines stretching it.
    chain: Chain,
    /// How far ahead of the source frame its output frame says the chain plays, in source frames.
    lead: f64,
    /// Source frames handed to the chain so far.
    fed: usize,
    /// Whether the chain has been told the source has ended.
    finished: bool,
    /// Silent frames to play before the chain's, where the song starts too soon to feed early.
    silence: usize,
}

impl Stretched {
    /// `track` at `rate` with `pitch`, from its start, unless the stretcher cannot take its
    /// sample rate.
    #[must_use]
    pub(crate) fn new(track: Arc<Track>, rate: f64, pitch: Pitch) -> Option<Self> {
        let lead = known_lead(rate, track.rate().get(), pitch)?;
        let chain = Chain::new(&stage_rates(rate, pitch), track.rate().get(), pitch)?;
        let mut stretched = Self {
            track,
            rate,
            pitch,
            clicks: Clicks::default(),
            music: 1.0,
            heard: 0,
            chain,
            lead,
            fed: 0,
            finished: false,
            silence: 0,
        };
        stretched.start(0.0);
        Some(stretched)
    }

    /// The same, with `clicks` placed for its speed mixed over the song at `music`.
    #[must_use]
    pub(crate) fn with_clicks(self, clicks: Clicks, music: Gain) -> Self {
        Self {
            clicks,
            music: music.amplitude(),
            ..self
        }
    }

    /// Feeds from where output frame 0 must sound source frame `from`: `lead` frames sooner,
    /// or from the start after silence as long as the song does not reach.
    fn start(&mut self, from: f64) {
        let feed = from - self.lead;
        if feed < 0.0 {
            self.fed = 0;
            self.silence = frame_index_from_f64((-feed / self.rate).round());
        } else {
            self.fed = frame_index_from_f64(feed.round()).min(self.track.frames().len());
            self.silence = 0;
        }
        if let Some(first) = self.chain.stages.first_mut() {
            first
                .source
                .set_track_position(u64::try_from(self.fed).unwrap_or(u64::MAX));
        }
        self.finished = false;
    }
}

impl Decoder for Stretched {
    type Error = Infallible;

    fn sample_rate(&self) -> u32 {
        self.track.rate().get()
    }

    fn num_frames(&self) -> usize {
        let source = frame_index_as_f64(self.track.frames().len());

        frame_index_from_f64((source / self.rate).ceil())
    }

    fn decode(&mut self) -> Result<Vec<Frame>, Infallible> {
        let mut frames = if self.silence > 0 {
            let quiet = self.silence.min(CHUNK);
            self.silence = self.silence.saturating_sub(quiet);
            vec![[0.0; 2]; quiet]
        } else {
            let mut out = [0.0; CHUNK * 2];
            self.chain.render(
                self.track.frames(),
                &mut self.fed,
                &mut self.finished,
                &mut out,
            );
            let music = self.music;
            out.as_chunks::<2>()
                .0
                .iter()
                .map(|&[left, right]| [left * music, right * music])
                .collect()
        };
        self.clicks.mix_into(&mut frames, self.heard);
        self.heard = self.heard.saturating_add(frames.len());
        Ok(frames
            .into_iter()
            .map(|[left, right]| Frame { left, right })
            .collect())
    }

    fn seek(&mut self, index: usize) -> Result<usize, Infallible> {
        let at = (frame_index_as_f64(index) * self.rate)
            .min(frame_index_as_f64(self.track.frames().len()));
        // A fresh chain from the source frame played there: timestretch seeks by rebuilding.
        if let Some(chain) = Chain::new(&self.chain.rates, self.track.rate().get(), self.pitch) {
            self.chain = chain;
        }
        self.start(at);
        self.heard = index;
        Ok(index)
    }
}

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "the audio frame index is approximate when converted to seconds"
)]
/// Converts a source frame index to floating point for rate scaling.
const fn frame_index_as_f64(index: usize) -> f64 {
    index as f64
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a calculated frame index is clamped to the source length"
)]
/// Truncates a nonnegative calculated frame index to an array offset.
const fn frame_index_from_f64(index: f64) -> usize {
    index as usize
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "a click's samples are between -1 and 1"
)]
/// A sample of the calibration click as the engines take it.
const fn narrow(sample: f64) -> f32 {
    sample as f32
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

        frame_index_as_f64(rising) * f64::from(RATE) / frame_index_as_f64(samples.len())
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
    fn pitch_is_kept_between_a_tenth_and_twice_the_speed_but_not_at_its_own() {
        assert!(keeps_pitch(0.5) && keeps_pitch(0.25) && keeps_pitch(0.1) && keeps_pitch(2.0));
        assert!(!keeps_pitch(1.0) && !keeps_pitch(0.05) && !keeps_pitch(4.0));
    }

    #[test]
    fn stretched_audio_keeps_the_source_sample_rate() {
        let stretched = Stretched::new(tone(440.0, 0.1, 0.0), 0.5, Pitch::Kept).unwrap();
        assert_eq!(stretched.sample_rate(), RATE);
    }

    #[test]
    fn half_speed_lasts_twice_as_long_at_the_same_pitch() {
        let mut stretched = Stretched::new(tone(440.0, 3.0, 0.0), 0.5, Pitch::Kept).unwrap();
        assert_eq!(stretched.num_frames(), 6 * 44_100);
        let samples = decoded(&mut stretched, 4 * 44_100);
        let middle = &samples[44_100..3 * 44_100];
        let heard = hertz(middle);
        assert!((heard - 440.0).abs() < 5.0, "{heard} Hz");
    }

    #[test]
    fn a_seek_plays_on_from_the_source_frame_its_frame_sounds() {
        // Silent for 2 s, then a tone: at half speed, frame 5 s sounds the tone at 2.5 s.
        let mut stretched = Stretched::new(tone(440.0, 4.0, 2.0), 0.5, Pitch::Kept).unwrap();
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
        let mut stretched = Stretched::new(tone(440.0, 1.0, 0.0), 0.5, Pitch::Kept).unwrap();
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
        for (rate, length) in [(SLOWEST, 1_323_000), (0.25, 529_200), (FASTEST, 66_150)] {
            let mut stretched = Stretched::new(tone(440.0, 3.0, 0.0), rate, Pitch::Kept).unwrap();
            assert_eq!(stretched.num_frames(), length);
            let samples = decoded(&mut stretched, 44_100);
            let middle = &samples[22_050..];
            let heard = hertz(middle);
            assert!((heard - 440.0).abs() < 15.0, "{rate}: {heard} Hz");
        }
    }

    /// Four seconds of short 2 kHz clicks every half second, and where each is centred.
    fn click_track() -> (Arc<Track>, Vec<f64>) {
        let seconds = 4.0;
        let frames = frame_index_from_f64(seconds * f64::from(RATE));
        let mut samples = vec![[0.0_f32; 2]; frames];
        let mut clicks = Vec::new();
        let mut t = 0.25;
        while t < seconds - 0.5 {
            let start = frame_index_from_f64(t * f64::from(RATE));
            for (k, frame) in samples.iter_mut().skip(start).take(220).enumerate() {
                let v = sample_as_f32(
                    0.8 * (std::f64::consts::TAU * 2000.0 * frame_index_as_f64(k)
                        / f64::from(RATE))
                    .sin(),
                );
                *frame = [v; 2];
            }
            clicks.push(t + 110.0 / f64::from(RATE));
            t += 0.5;
        }
        (Arc::new(Track::from_frames(RATE, samples)), clicks)
    }

    /// How far from where it is in the source each of `clicks` sounds in `out`, played at `rate`
    /// from source second `from`, in milliseconds of the source: the centre of its energy.
    fn centroids(out: &[f32], rate: f64, clicks: &[f64], from: f64) -> Vec<f64> {
        clicks
            .iter()
            .filter(|c| **c >= from + 0.05)
            .map(|c| {
                let centre = (c - from) / rate * f64::from(RATE);
                let half = 0.2 / rate * f64::from(RATE);
                let lo = frame_index_from_f64((centre - half).max(0.0));
                let hi = frame_index_from_f64(centre + half).min(out.len());
                let (mut e, mut te) = (0.0, 0.0);
                for (n, sample) in out.iter().enumerate().take(hi).skip(lo) {
                    let v = f64::from(*sample).powi(2);
                    e += v;
                    te += v * frame_index_as_f64(n);
                }
                ((te / e) / f64::from(RATE) * rate + from - c) * 1000.0
            })
            .collect()
    }

    #[test]
    fn song_clicks_sound_where_the_playhead_says_at_every_stretched_speed() {
        let (track, clicks) = click_track();
        let speeds = [0.1, 0.25, 1.0 / 3.0, 0.4, 0.5, 0.75, 1.5, 2.0]
            .map(|rate| (rate, Pitch::Kept))
            .into_iter()
            .chain([0.1, 0.3, 0.8, 3.0].map(|rate| (rate, Pitch::FollowsSpeed)));
        for (rate, pitch) in speeds {
            for from in [0.0, 1.3] {
                let mut stretched = Stretched::new(Arc::clone(&track), rate, pitch).unwrap();
                let start = frame_index_from_f64((from / rate * f64::from(RATE)).round());
                assert_eq!(stretched.seek(start).unwrap(), start);
                let until = 3.4;
                let frames = frame_index_from_f64((until - from) / rate * f64::from(RATE));
                let out = decoded(&mut stretched, frames);
                let heard: Vec<f64> = clicks
                    .iter()
                    .copied()
                    .filter(|c| *c < until - 0.3)
                    .collect();
                let played_from = frame_index_as_f64(start) * rate / f64::from(RATE);
                let off = centroids(&out, rate, &heard, played_from);
                assert!(off.len() >= 3, "{rate} from {from}: {off:?}");
                assert!(
                    off.iter().all(|ms| ms.abs() < 2.0),
                    "{rate} from {from}: {off:?}"
                );
            }
        }
    }

    #[test]
    fn clicks_play_at_their_own_speed_over_stretched_audio() {
        use charter_audio::{Kit, Tone, Volumes};
        let silent = Arc::new(Track::from_frames(RATE, vec![[0.0; 2]; 3 * 44_100]));
        let tone = Tone::Beat(Kit::ToneGate);
        let sound = charter_audio::render(tone, RATE);
        for (rate, pitch) in [
            (0.25, Pitch::Kept),
            (0.3, Pitch::FollowsSpeed),
            (1.5, Pitch::Kept),
        ] {
            let clicks = Clicks::new(&[(1.0, tone)], Volumes::default(), RATE, rate);
            let mut stretched = Stretched::new(Arc::clone(&silent), rate, pitch)
                .unwrap()
                .with_clicks(clicks, Gain::default());
            let at = frame_index_from_f64((1.0 / rate * f64::from(RATE)).round());
            // From partway in, so the click is placed by where the seek lands.
            let from = at.div_euclid(2);
            assert_eq!(stretched.seek(from).unwrap(), from);
            let mut heard: Vec<[f32; 2]> = Vec::new();
            while heard.len() < at - from + sound.len() {
                heard.extend(
                    stretched
                        .decode()
                        .unwrap()
                        .iter()
                        .map(|f| [f.left, f.right]),
                );
            }
            let click = &heard[at - from..at - from + sound.len()];
            assert!(
                click
                    .iter()
                    .zip(&sound)
                    .all(|(got, want)| (got[0] - want[0]).abs() < 1e-4),
                "{rate} {pitch:?}"
            );
            assert!(
                heard[..at - from].iter().all(|f| f[0].abs() < 1e-4),
                "{rate} {pitch:?}"
            );
        }
    }
}
