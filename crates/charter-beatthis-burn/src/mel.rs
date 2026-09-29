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
/// Lower edge of the mel filterbank in hertz.
const F_MIN: f64 = 30.0;
/// Upper edge of the mel filterbank in hertz.
const F_MAX: f64 = 11_000.0;
/// Scale applied to mel magnitudes before `ln(1 + x)`.
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
    let input_rate = usize::try_from(rate).map_err(|e| why(&e))?;
    let mut resampler = Fft::<f32>::new(
        input_rate,
        usize::try_from(RATE).map_err(|e| why(&e))?,
        1024,
        1,
        FixedSync::Input,
    )
    .map_err(|e| why(&e))?;
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
    let padded = reflected(samples, WINDOW.div_euclid(2));
    let window = hann();
    // torch's "frame_length" normalisation, which torch.stft applies as normalized=True.
    let energy = count_float(WINDOW).sqrt();
    let bank = filterbank();
    let fft = FftPlanner::<f64>::new().plan_fft_forward(WINDOW);
    let frames = samples.len().div_euclid(HOP).saturating_add(1);
    let mut buffer = vec![Complex::new(0.0, 0.0); WINDOW];
    let mut spect = Vec::with_capacity(frames);
    for frame in 0..frames {
        let start = frame.saturating_mul(HOP);
        for (slot, (sample, weight)) in buffer.iter_mut().zip(
            padded
                .get(start..start.saturating_add(WINDOW))
                .unwrap_or_default()
                .iter()
                .zip(&window),
        ) {
            *slot = Complex::new(f64::from(*sample) * weight, 0.0);
        }
        fft.process(&mut buffer);
        let magnitudes: Vec<f64> = buffer
            .get(..=WINDOW.div_euclid(2))
            .unwrap_or_default()
            .iter()
            .map(|bin| bin.norm() / energy)
            .collect();
        let mut row = [0.0; BINS];
        for (value, band) in row.iter_mut().zip(&bank) {
            let power: f64 = band.iter().zip(&magnitudes).map(|(w, m)| w * m).sum();
            let power = narrow_float(power);
            *value = (LOG_MULTIPLIER * power).ln_1p();
        }
        spect.push(row);
    }
    spect
}

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "FFT and mel bin positions fit in a small integer"
)]
/// FFT and filterbank indices as floating-point positions.
fn count_float(count: usize) -> f64 {
    count as f64
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "the network consumes the mel magnitudes as f32"
)]
/// An FFT magnitude at the precision consumed by the network.
fn narrow_float(value: f64) -> f32 {
    value as f32
}

/// `samples` with `pad` samples at each end mirrored from inside it, not repeating the end
/// samples themselves, as torch's reflect padding does.
fn reflected(samples: &[f32], pad: usize) -> Vec<f32> {
    let len = samples.len();
    let at = |index: isize| -> f32 {
        let last = isize::try_from(len).unwrap_or(isize::MAX).saturating_sub(1);
        let mut reflected_index = index;
        // Very short signals reflect more than once.
        while reflected_index < 0 || reflected_index > last {
            reflected_index = if reflected_index < 0 {
                reflected_index.saturating_neg()
            } else {
                last.saturating_mul(2).saturating_sub(reflected_index)
            };
            if last == 0 {
                reflected_index = 0;
            }
        }
        usize::try_from(reflected_index)
            .ok()
            .and_then(|position| samples.get(position))
            .copied()
            .unwrap_or(0.0)
    };
    let pad = isize::try_from(pad).unwrap_or(0);
    let len = isize::try_from(len).unwrap_or(0);
    (pad.saturating_neg()..len.saturating_add(pad))
        .map(at)
        .collect()
}

/// A periodic Hann window of [`WINDOW`] samples.
fn hann() -> Vec<f64> {
    let size = count_float(WINDOW);
    (0..WINDOW)
        .map(|n| {
            let n = count_float(n);
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

/// Hertz corresponding to `mel` on the Slaney scale.
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
    let bins = WINDOW.div_euclid(2).saturating_add(1);
    let nyquist = f64::from(RATE) / 2.0;
    let freqs: Vec<f64> = (0..bins)
        .map(|bin| nyquist * count_float(bin) / count_float(bins.saturating_sub(1)))
        .collect();
    let (low, high) = (mel(F_MIN), mel(F_MAX));
    let edges: Vec<f64> = (0..BINS.saturating_add(2))
        .map(
            |edge| hz(low + (high - low) * count_float(edge) / count_float(BINS.saturating_add(1))),
        )
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
            for n in start..start.saturating_add(32).min(rate) {
                samples[usize::try_from(n).unwrap()] +=
                    0.4 * (1.0 - f64::from(n.saturating_sub(start)) / 32.0);
            }
        }
        samples.into_iter().map(narrow_float).collect()
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
        for (index, (actual, expected)) in got.iter().flatten().zip(&want).enumerate() {
            assert!(
                (actual - expected).abs() < 1e-3 * expected.abs().max(1.0),
                "frame {}, band {}: {actual} vs {expected}",
                index.div_euclid(BINS),
                index.rem_euclid(BINS)
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

    #[test]
    fn native_rate_is_unchanged_and_invalid_rate_is_reported() {
        let samples = [0.0, 0.25, -0.5, 1.0];
        assert_eq!(resample(&samples, RATE).unwrap(), samples);
        assert!(matches!(resample(&samples, 0), Err(Error::Resample(_))));
        assert!(resample(&[], 44_100).unwrap().is_empty());
    }

    #[test]
    fn non_integer_rate_conversion_preserves_duration_and_level() {
        let samples = vec![0.25; 4_800];
        let got = resample(&samples, 48_000).unwrap();
        assert!(got.len().abs_diff(2_205) <= 1, "{}", got.len());
        assert!(
            got[200..2_000]
                .iter()
                .all(|sample| (sample - 0.25).abs() < 1e-3)
        );
    }

    #[test]
    fn silence_and_a_single_sample_produce_finite_frames() {
        let silent = log_mel(&[0.0]);
        assert_eq!(silent.len(), 1);
        assert!(silent[0].iter().all(|band| band.abs() < f32::EPSILON));
        let single = log_mel(&[1.0]);
        assert_eq!(single.len(), 1);
        assert!(single[0].iter().all(|band| band.is_finite()));
        assert!(single[0].iter().any(|band| *band > 0.0));
    }
}
