//! Short sounds mixed over a song as it plays: a tick on each note, a metronome on each beat.
//! The metronome's sounds are the CC0 pairs under `assets/clicks`, whose `PROVENANCE.md` says
//! how they were made.

use serde::{Deserialize, Serialize};

use crate::Track;

/// A short sound the editor mixes over the song.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// A bright tick, over almost as soon as it starts.
    Click,
    /// A pitched blip.
    Beep,
    /// A metronome's sound for a beat.
    Beat(Kit),
    /// A metronome's sound for the first beat of a measure.
    Accent(Kit),
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

/// `tone` as frames at `rate` frames per second.
#[must_use]
pub fn render(tone: Tone, rate: u32) -> Vec<[f32; 2]> {
    // Pitch, length in seconds and peak level of each tone.
    let (hertz, seconds, level) = match tone {
        Tone::Click => (2_000.0, 0.015, 0.6),
        Tone::Beep => (1_000.0, 0.06, 0.4),
        Tone::Beat(kit) => return sample(kit.wavs().1, rate),
        Tone::Accent(kit) => return sample(kit.wavs().0, rate),
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
    let step = f64::from(from) / f64::from(to);
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

/// `track` with each tone mixed in at its time, in seconds into the audio. Tones past either
/// end are left out.
#[must_use]
pub fn mix(track: &Track, clicks: &[(f64, Tone)]) -> Track {
    let rate = track.rate().get();
    let mut tones: Vec<(Tone, Vec<[f32; 2]>)> = Vec::new();
    let mut frames = track.frames().to_vec();
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
        for (frame, added) in under.iter_mut().zip(sound) {
            for (sample, add) in frame.iter_mut().zip(added) {
                *sample = (*sample + add).clamp(-1.0, 1.0);
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
        for tone in [Tone::Click, Tone::Beep] {
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
        let mixed = mix(&silence, &[(0.5, Tone::Accent(Kit::ToneGate))]);
        let frames = mixed.frames();
        assert!(loudest(&frames[..4000]) < f32::EPSILON);
        assert!(loudest(&frames[4000..4100]) > 0.05);
    }

    #[test]
    fn a_tone_lands_where_its_time_says() {
        let silence = Track::silence(8000, 8000);
        let mixed = mix(&silence, &[(0.5, Tone::Click)]);
        let frames = mixed.frames();
        assert_eq!(frames.len(), 8000);
        assert!(loudest(&frames[..4000]) < f32::EPSILON);
        assert!(loudest(&frames[4000..4040]) > 0.1);
    }

    #[test]
    fn tones_past_the_ends_are_left_out() {
        let silence = Track::silence(1000, 100);
        let mixed = mix(&silence, &[(-1.0, Tone::Beep), (5.0, Tone::Click)]);
        assert!(loudest(mixed.frames()) < f32::EPSILON);
        assert_eq!(mixed.frames().len(), 100);
    }

    #[test]
    fn tones_at_no_real_time_are_left_out() {
        let silence = Track::silence(8000, 800);
        let mixed = mix(
            &silence,
            &[(f64::NAN, Tone::Click), (f64::INFINITY, Tone::Beep)],
        );
        assert!(loudest(mixed.frames()) < f32::EPSILON);
    }

    #[test]
    fn a_tone_mixed_over_loud_audio_stays_within_range() {
        let loud = Track::from_frames(1000, vec![[0.95, -0.95]; 200]);
        let mixed = mix(&loud, &[(0.0, Tone::Beep)]);
        assert!(loudest(mixed.frames()) <= 1.0);
    }
}
