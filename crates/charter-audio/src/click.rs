//! Short sounds mixed over a song as it plays: a tick on each note, a metronome on each beat.
//! The metronome's sounds are the CC0 pairs under `assets/clicks`, and the hit sounds a note can
//! tick with the CC0 ones under `assets/hitsounds`; each folder's `PROVENANCE.md` says how they
//! were made.

use serde::{Deserialize, Serialize};

use crate::Track;

/// A short sound the editor mixes over the song.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// A note's tick: `sound`, pitched `cents` hundredths of a semitone up, or down when
    /// negative, by playing it faster or slower.
    Tick { sound: Tick, cents: i16 },
    /// A metronome's sound for a beat.
    Beat(Kit),
    /// A metronome's sound for the first beat of a measure.
    Accent(Kit),
    /// A metronome's sound for a grid line between beats: the beat's, at the subdivision
    /// volume.
    Sub(Kit),
}

/// What a note ticks with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tick {
    /// A bright tick, over almost as soon as it starts.
    Click,
    /// A pitched blip.
    Beep,
    /// The synthesised hit sounds.
    Hit,
    Clap,
    SoftClap,
    Finish,
}

impl Tick {
    /// The tick at its own pitch.
    #[must_use]
    pub fn tone(self) -> Tone {
        self.pitched(0)
    }

    /// The tick `cents` hundredths of a semitone up, or down when negative.
    #[must_use]
    pub fn pitched(self, cents: i16) -> Tone {
        Tone::Tick { sound: self, cents }
    }

    /// The hit sound's WAV file, for the ticks that are one.
    fn wav(self) -> Option<&'static [u8]> {
        macro_rules! hit {
            ($name:literal) => {
                include_bytes!(concat!("../assets/hitsounds/", $name, ".wav"))
            };
        }
        match self {
            Self::Click | Self::Beep => None,
            Self::Hit => Some(hit!("normal")),
            Self::Clap => Some(hit!("clap")),
            Self::SoftClap => Some(hit!("soft_clap")),
            Self::Finish => Some(hit!("finish")),
        }
    }
}

/// A metronome's pair of sounds, one for the first beat of a measure and one for the rest.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kit {
    #[default]
    ToneGate,
    SoftBuzz,
    StickClick,
    FilterKnock,
    PureSine,
    RampTick,
    SquareDecay,
    ShakerBurst,
    BrightTick,
}

impl Kit {
    pub const ALL: [Self; 9] = [
        Self::ToneGate,
        Self::SoftBuzz,
        Self::StickClick,
        Self::FilterKnock,
        Self::PureSine,
        Self::RampTick,
        Self::SquareDecay,
        Self::ShakerBurst,
        Self::BrightTick,
    ];

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::ToneGate => "Tone gate",
            Self::SoftBuzz => "Soft buzz",
            Self::StickClick => "Stick click",
            Self::FilterKnock => "Filter knock",
            Self::PureSine => "Pure sine",
            Self::RampTick => "Ramp tick",
            Self::SquareDecay => "Square decay",
            Self::ShakerBurst => "Shaker burst",
            Self::BrightTick => "Bright tick",
        }
    }

    /// The kit's accent and beat, as WAV files.
    fn wavs(self) -> (&'static [u8], &'static [u8]) {
        macro_rules! pair {
            ($name:literal) => {
                (
                    include_bytes!(concat!("../assets/clicks/", $name, "/accent.wav")),
                    include_bytes!(concat!("../assets/clicks/", $name, "/beat.wav")),
                )
            };
        }
        match self {
            Self::ToneGate => pair!("tone_gate"),
            Self::SoftBuzz => pair!("soft_buzz"),
            Self::StickClick => pair!("stick_click"),
            Self::FilterKnock => pair!("filter_knock"),
            Self::PureSine => pair!("pure_sine"),
            Self::RampTick => pair!("ramp_tick"),
            Self::SquareDecay => pair!("square_decay"),
            Self::ShakerBurst => pair!("shaker_burst"),
            Self::BrightTick => pair!("bright_tick"),
        }
    }
}

/// How loud something plays, from silent at 0 to as recorded at 1: a factor on its samples.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(from = "f32", into = "f32")]
pub struct Gain(f32);

impl Gain {
    pub const FULL: Self = Self(1.0);

    /// The factor on the samples, from 0 to 1.
    #[must_use]
    pub fn amplitude(self) -> f32 {
        self.0
    }

    /// The gain in decibels, as audio libraries take it, with silence as `floor`.
    #[must_use]
    pub fn decibels(self, floor: f32) -> f32 {
        if self.0 <= 0.0 {
            floor
        } else {
            (20.0 * self.0.log10()).max(floor)
        }
    }
}

impl Default for Gain {
    fn default() -> Self {
        Self::FULL
    }
}

/// Any number as a gain: kept from 0 to 1, and full for one that is not a number.
impl From<f32> for Gain {
    fn from(factor: f32) -> Self {
        if factor.is_nan() {
            Self::FULL
        } else {
            Self(factor.clamp(0.0, 1.0))
        }
    }
}

impl From<Gain> for f32 {
    fn from(gain: Gain) -> Self {
        gain.0
    }
}

/// How loud the song's audio plays, and the sounds mixed over it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Volumes {
    pub music: Gain,
    /// The tick on each note.
    pub ticks: Gain,
    pub metronome: Gain,
    /// The metronome's clicks between beats, as a share of its volume.
    pub subdivision: Gain,
}

impl Default for Volumes {
    fn default() -> Self {
        Self {
            music: Gain::FULL,
            ticks: Gain::FULL,
            metronome: Gain::FULL,
            subdivision: Gain::from(0.5),
        }
    }
}

impl Volumes {
    /// How loud `tone` mixes in.
    #[must_use]
    pub fn of(self, tone: Tone) -> Gain {
        match tone {
            Tone::Tick { .. } => self.ticks,
            Tone::Beat(_) | Tone::Accent(_) => self.metronome,
            Tone::Sub(_) => Gain::from(self.metronome.amplitude() * self.subdivision.amplitude()),
        }
    }
}

/// `tone` as frames at `rate` frames per second.
#[must_use]
pub fn render(tone: Tone, rate: u32) -> Vec<[f32; 2]> {
    match tone {
        Tone::Tick { sound, cents } => pitched(&tick(sound, rate), cents),
        Tone::Beat(kit) | Tone::Sub(kit) => sample(kit.wavs().1, rate),
        Tone::Accent(kit) => sample(kit.wavs().0, rate),
    }
}

/// `frames` played `cents` hundredths of a semitone higher, or lower when negative, and so
/// shorter or longer: how a struck sound sounds tuned up or down.
fn pitched(frames: &[[f32; 2]], cents: i16) -> Vec<[f32; 2]> {
    if cents == 0 {
        return frames.to_vec();
    }
    resample_by(frames, 2f64.powf(f64::from(cents) / 1200.0))
}

/// `sound` at its own pitch as frames at `rate`.
fn tick(sound: Tick, rate: u32) -> Vec<[f32; 2]> {
    if let Some(wav) = sound.wav() {
        return sample(wav, rate);
    }
    // Pitch, length in seconds and peak level of each synthesised tick.
    let (hertz, seconds, level) = if sound == Tick::Click {
        (2_000.0, 0.015, 0.6)
    } else {
        (1_000.0, 0.06, 0.4)
    };
    let rate = f64::from(rate.max(1));
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // a few thousand frames
    let length = (seconds * rate).round() as usize;
    (0..length)
        .map(|frame| {
            #[allow(clippy::cast_precision_loss)] // frames of a short tone stay exact
            let at = frame as f64;
            let left = 1.0 - at / seconds / rate;
            let envelope = left * left;
            let wave = (std::f64::consts::TAU * hertz * at / rate).sin();
            #[allow(clippy::cast_possible_truncation)] // a sample within -1 to 1
            let sample = (wave * envelope * level) as f32;
            [sample, sample]
        })
        .collect()
}

/// The embedded WAV `wav` as frames at `rate`. Every one decodes, which a test checks.
fn sample(wav: &[u8], rate: u32) -> Vec<[f32; 2]> {
    Track::decode(wav.to_vec(), Some("wav"))
        .map(|track| resample(track.frames(), track.rate().get(), rate.max(1)))
        .unwrap_or_default()
}

/// `frames` at `from` frames per second, as frames at `to`, by straight lines between them:
/// plenty for a click tens of milliseconds long.
fn resample(frames: &[[f32; 2]], from: u32, to: u32) -> Vec<[f32; 2]> {
    if from == to {
        return frames.to_vec();
    }
    resample_by(frames, f64::from(from) / f64::from(to))
}

/// `frames` read `step` frames apart, by straight lines between them.
fn resample_by(frames: &[[f32; 2]], step: f64) -> Vec<[f32; 2]> {
    #[allow(clippy::cast_precision_loss)] // a short sound's frames stay exact
    let length = frames.len() as f64 / step;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // a few thousand frames
    let length = length.round() as usize;
    (0..length)
        .map(|frame| {
            #[allow(clippy::cast_precision_loss)] // as above
            let at = frame as f64 * step;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // within the sound
            let before = at.floor() as usize;
            #[allow(clippy::cast_possible_truncation)] // a weight between 0 and 1
            let weight = (at - at.floor()) as f32;
            let a = frames.get(before).copied().unwrap_or_default();
            let b = frames.get(before + 1).copied().unwrap_or(a);
            let ([a_left, a_right], [b_left, b_right]) = (a, b);
            [
                a_left + (b_left - a_left) * weight,
                a_right + (b_right - a_right) * weight,
            ]
        })
        .collect()
}

/// `track` at the music's volume in `volumes`, with each tone mixed in at its time, in seconds
/// into the audio, at the volume `volumes` gives its kind, so turning one down leaves the rest.
/// Tones past either end are left out.
#[must_use]
pub fn mix(track: &Track, clicks: &[(f64, Tone)], volumes: Volumes) -> Track {
    let rate = track.rate().get();
    let mut tones: Vec<(Tone, Vec<[f32; 2]>)> = Vec::new();
    let music = volumes.music.amplitude();
    let mut frames: Vec<[f32; 2]> = track
        .frames()
        .iter()
        .map(|&[left, right]| [left * music, right * music])
        .collect();
    for &(at, tone) in clicks {
        if !tones.iter().any(|(kind, _)| *kind == tone) {
            tones.push((tone, render(tone, rate)));
        }
        let Some((_, sound)) = tones.iter().find(|(kind, _)| *kind == tone) else {
            continue;
        };
        let start = at * f64::from(rate);
        if !start.is_finite() || start < 0.0 {
            continue;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // checked above
        let start = start.round() as usize;
        let Some(under) = frames.get_mut(start..) else {
            continue;
        };
        let gain = volumes.of(tone).amplitude();
        for (frame, added) in under.iter_mut().zip(sound) {
            for (sample, add) in frame.iter_mut().zip(added) {
                *sample = (*sample + add * gain).clamp(-1.0, 1.0);
            }
        }
    }
    Track::from_frames(rate, frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loudest(frames: &[[f32; 2]]) -> f32 {
        frames
            .iter()
            .flat_map(|frame| frame.iter())
            .fold(0.0, |most, sample| most.max(sample.abs()))
    }

    #[test]
    fn a_tone_is_short_audible_and_fades_to_silence() {
        for tone in [Tick::Click.tone(), Tick::Beep.tone()] {
            let frames = render(tone, 48_000);
            assert!(
                (480..=6_000).contains(&frames.len()),
                "{tone:?}: {}",
                frames.len()
            );
            assert!(loudest(&frames) > 0.1, "{tone:?}");
            assert!(loudest(&frames) <= 1.0, "{tone:?}");
            let tail = &frames[frames.len() - 10..];
            assert!(loudest(tail) < 0.01, "{tone:?}");
        }
    }

    #[test]
    fn every_metronome_sound_decodes_short_audible_and_within_range() {
        for kit in Kit::ALL {
            for tone in [Tone::Beat(kit), Tone::Accent(kit)] {
                let frames = render(tone, 48_000);
                assert!(
                    (10..=9_600).contains(&frames.len()),
                    "{tone:?}: {}",
                    frames.len()
                );
                assert!(loudest(&frames) > 0.05, "{tone:?}");
                assert!(loudest(&frames) <= 1.0, "{tone:?}");
            }
        }
    }

    #[test]
    fn every_hit_sound_decodes_short_audible_and_within_range() {
        for sound in [Tick::Hit, Tick::Clap, Tick::SoftClap, Tick::Finish] {
            let frames = render(sound.tone(), 48_000);
            assert!(
                (480..=48_000).contains(&frames.len()),
                "{sound:?}: {}",
                frames.len()
            );
            assert!(loudest(&frames) > 0.05, "{sound:?}");
            assert!(loudest(&frames) <= 1.0, "{sound:?}");
        }
    }

    #[test]
    fn a_tick_an_octave_up_is_half_as_long_and_one_down_twice() {
        let own = render(Tick::Clap.tone(), 44_100).len();
        let up = render(Tick::Clap.pitched(1200), 44_100).len();
        let down = render(Tick::Clap.pitched(-1200), 44_100).len();
        assert!(up.abs_diff(own / 2) <= 1, "{up} of {own}");
        assert!(down.abs_diff(own * 2) <= 1, "{down} of {own}");
    }

    #[test]
    fn a_sound_resampled_keeps_its_length_in_time() {
        let ramp: Vec<[f32; 2]> = (0..441u16).map(|n| [f32::from(n) / 441.0; 2]).collect();
        let resampled = resample(&ramp, 44_100, 48_000);
        assert_eq!(resampled.len(), 480);
        assert!((resampled[240][0] - 0.5).abs() < 0.01);
        assert_eq!(resample(&ramp, 44_100, 44_100), ramp);
    }

    #[test]
    fn a_metronome_accent_lands_where_its_time_says() {
        let silence = Track::silence(8000, 8000);
        let mixed = mix(
            &silence,
            &[(0.5, Tone::Accent(Kit::ToneGate))],
            Volumes::default(),
        );
        let frames = mixed.frames();
        assert!(loudest(&frames[..4000]) < f32::EPSILON);
        assert!(loudest(&frames[4000..4100]) > 0.05);
    }

    #[test]
    fn a_gain_stays_between_silence_and_full() {
        assert!((Gain::from(1.7).amplitude() - 1.0).abs() < f32::EPSILON);
        assert!(Gain::from(-0.2).amplitude().abs() < f32::EPSILON);
        assert!((Gain::from(f32::NAN).amplitude() - 1.0).abs() < f32::EPSILON);
        assert!((Gain::from(0.25).amplitude() - 0.25).abs() < f32::EPSILON);
    }

    #[test]
    fn the_music_volume_turns_down_the_track_and_not_the_clicks_over_it() {
        let hum = Track::from_frames(8000, vec![[0.5, -0.5]; 800]);
        let clicks = [(0.05, Tone::Accent(Kit::ToneGate))];
        let full = mix(&hum, &clicks, Volumes::default());
        let volumes = Volumes {
            music: Gain::from(0.0),
            ..Volumes::default()
        };
        let quiet = mix(&hum, &clicks, volumes);
        assert!(loudest(&quiet.frames()[..400]) < f32::EPSILON);
        let accent = |track: &Track| loudest(&track.frames()[400..700]);
        let alone = mix(&Track::silence(8000, 800), &clicks, Volumes::default());
        assert!((accent(&quiet) - accent(&alone)).abs() < 1e-6);
        assert!(accent(&full) > accent(&quiet) - 1e-6);
    }

    #[test]
    fn a_subdivision_click_is_the_beat_at_its_share_of_the_metronome() {
        let silence = Track::silence(8000, 800);
        let beat = mix(
            &silence,
            &[(0.0, Tone::Beat(Kit::ToneGate))],
            Volumes::default(),
        );
        let sub = mix(
            &silence,
            &[(0.0, Tone::Sub(Kit::ToneGate))],
            Volumes::default(),
        );
        assert!((loudest(sub.frames()) - loudest(beat.frames()) / 2.0).abs() < 1e-4);
    }

    #[test]
    fn a_gain_reads_in_decibels_down_to_a_floor() {
        assert!(Gain::FULL.decibels(-60.0).abs() < 1e-6);
        assert!((Gain::from(0.5).decibels(-60.0) + 6.0206).abs() < 1e-3);
        assert!((Gain::from(0.0).decibels(-60.0) + 60.0).abs() < f32::EPSILON);
        assert!((Gain::from(0.000_01).decibels(-60.0) + 60.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_click_is_mixed_at_its_group_s_volume() {
        let silence = Track::silence(8000, 800);
        let clicks = [
            (0.0, Tone::Accent(Kit::ToneGate)),
            (0.05, Tick::Click.tone()),
        ];
        let full = mix(&silence, &clicks, Volumes::default());
        let volumes = Volumes {
            metronome: Gain::from(0.5),
            ticks: Gain::from(0.0),
            ..Volumes::default()
        };
        let quiet = mix(&silence, &clicks, volumes);
        let (full_accent, quiet_accent) = (
            loudest(&full.frames()[..300]),
            loudest(&quiet.frames()[..300]),
        );
        assert!((quiet_accent - full_accent / 2.0).abs() < 1e-4);
        assert!(loudest(&quiet.frames()[400..]) < f32::EPSILON);
    }

    #[test]
    fn a_tone_lands_where_its_time_says() {
        let silence = Track::silence(8000, 8000);
        let mixed = mix(&silence, &[(0.5, Tick::Click.tone())], Volumes::default());
        let frames = mixed.frames();
        assert_eq!(frames.len(), 8000);
        assert!(loudest(&frames[..4000]) < f32::EPSILON);
        assert!(loudest(&frames[4000..4040]) > 0.1);
    }

    #[test]
    fn tones_past_the_ends_are_left_out() {
        let silence = Track::silence(1000, 100);
        let mixed = mix(
            &silence,
            &[(-1.0, Tick::Beep.tone()), (5.0, Tick::Click.tone())],
            Volumes::default(),
        );
        assert!(loudest(mixed.frames()) < f32::EPSILON);
        assert_eq!(mixed.frames().len(), 100);
    }

    #[test]
    fn tones_at_no_real_time_are_left_out() {
        let silence = Track::silence(8000, 800);
        let mixed = mix(
            &silence,
            &[
                (f64::NAN, Tick::Click.tone()),
                (f64::INFINITY, Tick::Beep.tone()),
            ],
            Volumes::default(),
        );
        assert!(loudest(mixed.frames()) < f32::EPSILON);
    }

    #[test]
    fn a_tone_mixed_over_loud_audio_stays_within_range() {
        let loud = Track::from_frames(1000, vec![[0.95, -0.95]; 200]);
        let mixed = mix(&loud, &[(0.0, Tick::Beep.tone())], Volumes::default());
        assert!(loudest(mixed.frames()) <= 1.0);
    }

    #[test]
    fn a_click_on_the_last_frame_is_clipped_without_extending_the_song() {
        let silent = Track::silence(8000, 800);
        let mixed = mix(
            &silent,
            &[(798.0 / 8000.0, Tick::Beep.tone())],
            Volumes::default(),
        );
        assert_eq!(mixed.frames().len(), 800);
        assert_eq!(&mixed.frames()[..798], &silent.frames()[..798]);
        assert!(mixed.frames()[799][0].abs() > 0.1);
    }
}
