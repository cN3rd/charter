//! A summary of a track for drawing its frequencies: how loud each of a few dozen bands spaced
//! evenly in pitch, from 30 Hz to 16 kHz, is every 5 ms, in decibels below the track's loudest
//! scaled to a byte. A kick and a hi-hat that sound together show apart, which a waveform cannot.

use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

use crate::Track;

/// Frames of the spectrum a second.
const PER_SECOND: f64 = Spectrum::PER_SECOND;
/// Samples each frame's window spans at 44.1 kHz, about 46 ms: enough to tell 30 Hz from 60.
const WINDOW: usize = 2048;
const LOWEST: f64 = 30.0;
const HIGHEST: f64 = 16_000.0;
/// How far below the track's loudest, in decibels, a band reads as silent.
const RANGE: f64 = 80.0;
/// A band's level, in decibels, below which a track counts as silent: far under any sound.
const SILENT: f64 = -40.0;

/// How loud each band of a track is over time.
#[derive(Clone, Debug, PartialEq)]
pub struct Spectrum {
    /// Frames of `BANDS` levels each, from 0 for silent to 255 for the track's loudest.
    levels: Vec<[u8; Spectrum::BANDS]>,
}

impl Spectrum {
    /// Bands a frame holds, lowest first.
    pub const BANDS: usize = 48;
    /// Frames a second, frame `n` centred on `n / PER_SECOND` seconds.
    pub const PER_SECOND: f64 = 200.0;

    /// Every frame, from the track's start: each band's level, 0 for silent to 255 for the
    /// track's loudest.
    #[must_use]
    pub fn frames(&self) -> &[[u8; Self::BANDS]] {
        &self.levels
    }

    /// The spectrum of `track`, a frame every 5 ms from its start, each centred on its time.
    #[must_use]
    pub fn of(track: &Track) -> Self {
        let rate = f64::from(track.rate().get());
        let mono: Vec<f32> = track
            .frames()
            .iter()
            .map(|[left, right]| (left + right) / 2.0)
            .collect();
        let window: Vec<f32> = (0..WINDOW)
            .map(|n| {
                #[allow(clippy::cast_precision_loss)] // a window's length
                let phase = std::f32::consts::TAU * n as f32 / WINDOW as f32;
                0.5 - 0.5 * phase.cos()
            })
            .collect();
        let bands = bands(rate);
        let fft = FftPlanner::<f32>::new().plan_fft_forward(WINDOW);
        let mut buffer = vec![Complex::new(0.0, 0.0); WINDOW];
        let mut scratch = vec![Complex::new(0.0, 0.0); fft.get_inplace_scratch_len()];
        #[allow(clippy::cast_precision_loss)] // a track's samples, far inside f64
        let seconds = mono.len() as f64 / rate;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // a frame count
        let frames = (seconds * PER_SECOND).ceil() as usize;
        let mut decibels: Vec<[f64; Self::BANDS]> = Vec::with_capacity(frames);
        for frame in 0..frames {
            // Centred on the sample nearest its time, rather than a rounded hop apart, which
            // drifts at rates 200 does not divide, such as 44.1 kHz. Silence before the track
            // and after it.
            #[allow(clippy::cast_precision_loss)] // a frame index
            let time = frame as f64 / PER_SECOND;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // a sample index
            let centre = (time * rate).round() as usize;
            for (at, (slot, weight)) in buffer.iter_mut().zip(&window).enumerate() {
                let sample = (centre + at)
                    .checked_sub(WINDOW / 2)
                    .and_then(|index| mono.get(index))
                    .copied()
                    .unwrap_or(0.0);
                *slot = Complex::new(sample * weight, 0.0);
            }
            fft.process_with_scratch(&mut buffer, &mut scratch);
            let mut row = [0.0; Self::BANDS];
            for (level, &(from, to)) in row.iter_mut().zip(&bands) {
                let bins = buffer.get(from..to).unwrap_or_default();
                let power: f64 = bins.iter().map(|bin| f64::from(bin.norm_sqr())).sum();
                #[allow(clippy::cast_precision_loss)] // a few bins
                let mean = power / bins.len().max(1) as f64;
                *level = 10.0 * (mean + 1e-12).log10();
            }
            decibels.push(row);
        }
        let loudest = decibels
            .iter()
            .flatten()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        // Near-silent audio has no loudest worth scaling to.
        let loudest = loudest.max(SILENT);
        let levels = decibels
            .into_iter()
            .map(|row| {
                row.map(|level| {
                    let scaled = ((level - loudest + RANGE) / RANGE).clamp(0.0, 1.0) * 255.0;
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // 0 to 255
                    let level = scaled.round() as u8;
                    level
                })
            })
            .collect();
        Self { levels }
    }

    /// Each band's loudest over the frames from `from` to `to` seconds, or none outside the
    /// track.
    #[must_use]
    pub fn between(&self, from: f64, to: f64) -> Option<[u8; Self::BANDS]> {
        let frame = |seconds: f64| {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // a frame
            let frame = (seconds * PER_SECOND).round().max(0.0) as usize;
            frame
        };
        if to < 0.0 {
            return None;
        }
        let (first, last) = (frame(from.min(to)), frame(from.max(to)));
        let rows = self
            .levels
            .get(first..=last.min(self.levels.len().checked_sub(1)?))?;
        let mut most = [0; Self::BANDS];
        for row in rows {
            for (most, level) in most.iter_mut().zip(row) {
                *most = (*most).max(*level);
            }
        }
        Some(most)
    }
}

/// The FFT bins each band sums, as a range from its first to past its last, every band at least
/// one bin wide.
fn bands(rate: f64) -> Vec<(usize, usize)> {
    #[allow(clippy::cast_precision_loss)] // a window's length
    let per_bin = rate / WINDOW as f64;
    let nyquist = WINDOW / 2;
    let ratio =
        (HIGHEST / LOWEST).powf(1.0 / f64::from(u16::try_from(Spectrum::BANDS).unwrap_or(1)));
    (0..Spectrum::BANDS)
        .map(|band| {
            let low = LOWEST * ratio.powi(i32::try_from(band).unwrap_or(0));
            let high = low * ratio;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // bins of a window
            let from = ((low / per_bin).floor() as usize).min(nyquist);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let to = ((high / per_bin).ceil() as usize).clamp(from + 1, nyquist + 1);
            (from, to)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;

    fn tone(hz: f64, seconds: f64) -> Track {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let length = (seconds * f64::from(RATE)) as usize;
        let frames = (0..length)
            .map(|n| {
                #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
                let sample =
                    (0.5 * (std::f64::consts::TAU * hz * n as f64 / f64::from(RATE)).sin()) as f32;
                [sample; 2]
            })
            .collect();
        Track::from_frames(RATE, frames)
    }

    /// The band `hz` falls in.
    fn band_of(hz: f64) -> usize {
        let ratio = (HIGHEST / LOWEST).powf(1.0 / 48.0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let band = ((hz / LOWEST).ln() / ratio.ln()).floor() as usize;
        band
    }

    #[test]
    fn a_tone_is_loudest_in_its_own_band_and_quiet_far_from_it() {
        let spectrum = Spectrum::of(&tone(440.0, 1.0));
        assert_eq!(spectrum.levels.len(), 200);
        let middle = spectrum.between(0.5, 0.5).unwrap();
        let (loudest, _) = middle
            .iter()
            .enumerate()
            .max_by_key(|(_, level)| **level)
            .unwrap();
        assert_eq!(loudest, band_of(440.0));
        assert!(middle[band_of(440.0)] > 240);
        assert!(middle[band_of(5000.0)] < 60, "{middle:?}");
    }

    #[test]
    fn a_click_late_in_a_44100_hz_track_lands_on_the_frame_of_its_time() {
        // Ten seconds of silence at 44.1 kHz, which 200 frames a second do not divide, with a
        // click at 9.5 s.
        let mut frames = vec![[0.0f32; 2]; 10 * 44_100];
        for frame in frames.iter_mut().skip(9 * 44_100 + 22_050).take(64) {
            *frame = [0.9, 0.9];
        }
        let spectrum = Spectrum::of(&Track::from_frames(44_100, frames));
        let loudest = spectrum
            .frames()
            .iter()
            .enumerate()
            .max_by_key(|(_, frame)| frame.iter().map(|&level| u32::from(level)).sum::<u32>())
            .map(|(at, _)| at)
            .unwrap();
        assert!(loudest.abs_diff(1900) <= 1, "frame {loudest}, not 1900");
        assert_eq!(spectrum.frames().len(), 2000);
    }

    #[test]
    fn silence_reads_as_silent_and_time_outside_the_track_as_nothing() {
        let quiet = Spectrum::of(&Track::silence(RATE, 44_100));
        assert!(
            quiet
                .between(0.2, 0.8)
                .unwrap()
                .iter()
                .all(|level| *level == 0)
        );
        let spectrum = Spectrum::of(&tone(440.0, 1.0));
        assert!(spectrum.between(-1.0, -0.5).is_none());
        assert!(spectrum.between(5.0, 6.0).is_none());
    }

    #[test]
    fn a_stretch_of_time_reads_as_each_bands_loudest_frame_in_it() {
        let spectrum = Spectrum::of(&tone(440.0, 1.0));
        let one = spectrum.between(0.3, 0.3).unwrap();
        let many = spectrum.between(0.1, 0.9).unwrap();
        assert!(one.iter().zip(&many).all(|(one, many)| many >= one));
    }
}
