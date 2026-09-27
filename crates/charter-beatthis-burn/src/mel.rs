//! The log-mel spectrogram Beat This! listens to, computed as its reference front end computes
//! it with torchaudio: 22 050 Hz audio, a 1024-sample periodic Hann window every 441 samples, so
//! 50 frames a second, the signal reflected at either end so each frame is centred on its hop,
//! magnitudes divided by the square root of the window's length, 128 Slaney mel bands from 30 Hz to 11 kHz without
//! area normalisation, and `ln(1 + 1000 x)`.

use std::f64::consts::PI;

use audioadapter_buffers::owned::InterleavedOwned;
use rubato::{Fft, FixedSync, Resampler};
use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

use crate::Error;

/// The sample rate the network was trained at.
pub const RATE: u32 = 22_050;
/// Frames a second of the spectrogram, and of the network's predictions.
pub const FPS: u32 = 50;
/// Mel bands a frame holds.
pub const BINS: usize = 128;
/// Samples a frame's window spans.
const WINDOW: usize = 1024;
/// Samples from one frame to the next.
const HOP: usize = 441;
const F_MIN: f64 = 30.0;
const F_MAX: f64 = 11_000.0;
const LOG_MULTIPLIER: f32 = 1000.0;

/// `samples`, mono audio at `rate`, at the network's [`RATE`].
///
/// # Errors
///
/// Returns [`Error::Resample`] for a rate the resampler cannot convert from.
pub fn resample(samples: &[f32], rate: u32) -> Result<Vec<f32>, Error> {
    if rate == RATE || samples.is_empty() {
        return Ok(samples.to_vec());
    }
    let why = |error: &dyn std::fmt::Display| Error::Resample(error.to_string());
    let input =
        InterleavedOwned::new_from(samples.to_vec(), 1, samples.len()).map_err(|e| why(&e))?;
    let rate = usize::try_from(rate).map_err(|e| why(&e))?;
    let mut resampler =
        Fft::<f32>::new(rate, RATE as usize, 1024, 1, FixedSync::Input).map_err(|e| why(&e))?;
    let output = resampler
        .process_all(&input, samples.len(), None)
        .map_err(|e| why(&e))?;
    Ok(output.take_data())
}

/// The log-mel spectrogram of `samples`, mono audio at [`RATE`], a frame every 441 samples from
/// the first, each centred on its hop.
#[must_use]
pub fn log_mel(samples: &[f32]) -> Vec<[f32; BINS]> {
    if samples.is_empty() {
        return Vec::new();
    }
    let padded = reflected(samples, WINDOW / 2);
    let window = hann();
    // torch's "frame_length" normalisation, which torch.stft applies as normalized=True.
    #[allow(clippy::cast_precision_loss)] // a window's length
    let energy = (WINDOW as f64).sqrt();
    let bank = filterbank();
    let fft = FftPlanner::<f64>::new().plan_fft_forward(WINDOW);
    let frames = 1 + samples.len() / HOP;
    let mut buffer = vec![Complex::new(0.0, 0.0); WINDOW];
    let mut spect = Vec::with_capacity(frames);
    for frame in 0..frames {
        let start = frame * HOP;
        for (slot, (sample, weight)) in buffer.iter_mut().zip(
            padded
                .get(start..start + WINDOW)
                .unwrap_or_default()
                .iter()
                .zip(&window),
        ) {
            *slot = Complex::new(f64::from(*sample) * weight, 0.0);
        }
        fft.process(&mut buffer);
        let magnitudes: Vec<f64> = buffer
            .get(..=WINDOW / 2)
            .unwrap_or_default()
            .iter()
            .map(|bin| bin.norm() / energy)
            .collect();
        let mut row = [0.0; BINS];
        for (value, band) in row.iter_mut().zip(&bank) {
            let power: f64 = band.iter().zip(&magnitudes).map(|(w, m)| w * m).sum();
            #[allow(clippy::cast_possible_truncation)] // a band's magnitude
            let power = power as f32;
            *value = (LOG_MULTIPLIER * power).ln_1p();
        }
        spect.push(row);
    }
    spect
}

/// `samples` with `pad` samples at each end mirrored from inside it, not repeating the end
/// samples themselves, as torch's reflect padding does.
fn reflected(samples: &[f32], pad: usize) -> Vec<f32> {
    let len = samples.len();
    let at = |index: isize| -> f32 {
        let last = isize::try_from(len).unwrap_or(isize::MAX) - 1;
        let mut index = index;
        // Very short signals reflect more than once.
        while index < 0 || index > last {
            index = if index < 0 { -index } else { 2 * last - index };
            if last == 0 {
                index = 0;
            }
        }
        usize::try_from(index)
            .ok()
            .and_then(|index| samples.get(index))
            .copied()
            .unwrap_or(0.0)
    };
    let pad = isize::try_from(pad).unwrap_or(0);
    let len = isize::try_from(len).unwrap_or(0);
    (-pad..len + pad).map(at).collect()
}

/// A periodic Hann window of [`WINDOW`] samples.
fn hann() -> Vec<f64> {
    #[allow(clippy::cast_precision_loss)] // a window's length
    let size = WINDOW as f64;
    (0..WINDOW)
        .map(|n| {
            #[allow(clippy::cast_precision_loss)]
            let n = n as f64;
            0.5 - 0.5 * (2.0 * PI * n / size).cos()
        })
        .collect()
}

/// Hertz on the Slaney mel scale: linear to 1 kHz, logarithmic above.
fn mel(hz: f64) -> f64 {
    let linear = hz * 3.0 / 200.0;
    if hz < 1000.0 {
        linear
    } else {
        15.0 + (hz / 1000.0).ln() * 27.0 / 6.4_f64.ln()
    }
}

fn hz(mel: f64) -> f64 {
    if mel < 15.0 {
        mel * 200.0 / 3.0
    } else {
        1000.0 * ((mel - 15.0) * 6.4_f64.ln() / 27.0).exp()
    }
}

/// Each band's weight for every FFT bin: triangles between band edges spaced evenly in mel, as
/// torchaudio's `melscale_fbanks` makes them without normalisation.
fn filterbank() -> Vec<Vec<f64>> {
    let bins = WINDOW / 2 + 1;
    let nyquist = f64::from(RATE) / 2.0;
    #[allow(clippy::cast_precision_loss)] // small counts
    let freqs: Vec<f64> = (0..bins)
        .map(|bin| nyquist * bin as f64 / (bins - 1) as f64)
        .collect();
    let (low, high) = (mel(F_MIN), mel(F_MAX));
    #[allow(clippy::cast_precision_loss)]
    let edges: Vec<f64> = (0..BINS + 2)
        .map(|edge| hz(low + (high - low) * edge as f64 / (BINS + 1) as f64))
        .collect();
    edges
        .windows(3)
        .filter_map(|edges| {
            let &[left, centre, right] = edges else {
                return None;
            };
            Some(
                freqs
                    .iter()
                    .map(|freq| {
                        let rising = (freq - left) / (centre - left);
                        let falling = (right - freq) / (right - centre);
                        rising.min(falling).max(0.0)
                    })
                    .collect(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One second at 22 050 Hz: a 440 Hz tone at half scale, and a click every quarter second,
    /// as the fixture script builds it.
    fn signal() -> Vec<f32> {
        let rate: u32 = 22_050;
        let mut samples: Vec<f64> = (0..rate)
            .map(|n| 0.5 * (2.0 * PI * 440.0 * f64::from(n) / f64::from(rate)).sin())
            .collect();
        // The script's clicks start every rate // 4 samples.
        for start in (0..rate).step_by(5512) {
            for n in start..(start + 32).min(rate) {
                samples[usize::try_from(n).unwrap()] += 0.4 * (1.0 - f64::from(n - start) / 32.0);
            }
        }
        #[allow(clippy::cast_possible_truncation)]
        samples.into_iter().map(|sample| sample as f32).collect()
    }

    #[test]
    fn the_spectrogram_matches_the_reference_front_end() {
        let want: Vec<f32> = include_bytes!("../fixtures/mel.f32")
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| f32::from_le_bytes(*word))
            .collect();
        let got = log_mel(&signal());
        assert_eq!(got.len() * BINS, want.len());
        for (index, (got, want)) in got.iter().flatten().zip(&want).enumerate() {
            assert!(
                (got - want).abs() < 1e-3 * want.abs().max(1.0),
                "frame {}, band {}: {got} vs {want}",
                index / BINS,
                index % BINS
            );
        }
    }

    #[test]
    fn a_frame_is_centred_on_each_hop_from_the_first_sample() {
        assert_eq!(log_mel(&vec![0.0; 22_050]).len(), 51);
        assert_eq!(log_mel(&vec![0.0; 440]).len(), 1);
        assert!(log_mel(&[]).is_empty());
        assert_eq!(
            reflected(&[1.0, 2.0, 3.0], 2),
            [3.0, 2.0, 1.0, 2.0, 3.0, 2.0, 1.0]
        );
    }

    #[test]
    fn audio_at_twice_the_rate_resamples_to_half_the_samples() {
        let samples = vec![0.25; 44_100];
        let resampled = resample(&samples, 44_100).unwrap();
        assert!(resampled.len().abs_diff(22_050) <= 1, "{}", resampled.len());
        assert!(
            resampled[1000..21_000]
                .iter()
                .all(|s| (s - 0.25).abs() < 1e-3)
        );
        assert_eq!(resample(&samples, RATE).unwrap().len(), 44_100);
    }
}
