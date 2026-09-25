//! Short sounds mixed over a song as it plays: a tick on each note, a metronome on each beat.

use crate::Track;

/// A short sound the editor mixes over the song.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// A bright tick, over almost as soon as it starts.
    Click,
    /// A pitched blip.
    Beep,
}

/// `tone` as frames at `rate` frames per second.
#[must_use]
pub fn render(tone: Tone, rate: u32) -> Vec<[f32; 2]> {
    // Pitch, length in seconds and peak level of each tone.
    let (hertz, seconds, level) = match tone {
        Tone::Click => (2_000.0, 0.015, 0.6),
        Tone::Beep => (1_000.0, 0.06, 0.4),
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

/// `track` with each tone mixed in at its time, in seconds into the audio. Tones past either
/// end are left out.
#[must_use]
pub fn mix(track: &Track, clicks: &[(f64, Tone)]) -> Track {
    let rate = track.rate().get();
    let tones = [Tone::Click, Tone::Beep].map(|tone| (tone, render(tone, rate)));
    let mut frames = track.frames().to_vec();
    for &(at, tone) in clicks {
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
