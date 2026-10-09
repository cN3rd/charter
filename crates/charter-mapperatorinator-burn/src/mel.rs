//! The power spectrogram under the model's mel spectrogram, as torchaudio's `Spectrogram` makes
//! it for upstream's `MelSpectrogram` (`osuT5/osuT5/model/spectrogram.py`): a 1024-sample Hann
//! window every 128 samples, the signal reflected at both ends so frames centre on their hop,
//! squared magnitudes of the one-sided FFT. The mel filterbank and the log follow on the
//! device, in `model.rs`, with the filterbank and window the checkpoint stores.

use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

use crate::audio::{FRAMES, HOP};

/// Samples in an FFT, and in the Hann window over it.
pub(crate) const N_FFT: usize = 1024;
/// Frequency bins of the one-sided FFT.
pub(crate) const BINS: usize = N_FFT.div_euclid(2) + 1;
/// Frames of a window: one per hop plus the one centring adds.
pub(crate) const MEL_FRAMES: usize = FRAMES + 1;

/// The power spectrogram of each window in `windows`, `[MEL_FRAMES, BINS]` row-major one after
/// another, computed on all cores.
pub(crate) fn power_spectrograms(windows: &[&[f32]], hann: &[f32]) -> Vec<f32> {
    let mut out = vec![0.0_f32; windows.len().saturating_mul(MEL_FRAMES * BINS)];
    let threads = std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .min(windows.len().max(1));
    let per = windows.len().div_ceil(threads.max(1)).max(1);
    std::thread::scope(|scope| {
        for (chunk_windows, chunk_out) in windows
            .chunks(per)
            .zip(out.chunks_mut(per.saturating_mul(MEL_FRAMES * BINS)))
        {
            scope.spawn(move || {
                let fft = FftPlanner::<f32>::new().plan_fft_forward(N_FFT);
                let mut buffer = vec![Complex::new(0.0, 0.0); N_FFT];
                for (window, spec) in chunk_windows
                    .iter()
                    .zip(chunk_out.chunks_mut(MEL_FRAMES * BINS))
                {
                    power_spectrogram(window, hann, &*fft, &mut buffer, spec);
                }
            });
        }
    });
    out
}

/// The power spectrogram of the window `x`, written to `out` a row of [`BINS`] a frame, through
/// `fft` and its scratch `buffer` of [`N_FFT`] values.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "sample positions in a window are far from isize's limits"
)]
#[expect(
    clippy::indexing_slicing,
    reason = "reflection keeps positions inside the window, and the Hann window is N_FFT long"
)]
fn power_spectrogram(
    x: &[f32],
    hann: &[f32],
    fft: &dyn rustfft::Fft<f32>,
    buffer: &mut [Complex<f32>],
    out: &mut [f32],
) {
    let half = N_FFT.div_euclid(2);
    let n = signed(x.len());
    // torch.nn.functional.pad(mode="reflect"): the edge sample is not repeated.
    let sample = |i: isize| -> f32 {
        let j = i - signed(half);
        let j = if j < 0 {
            -j
        } else if j >= n {
            2 * (n - 1) - j
        } else {
            j
        };
        x[unsigned(j)]
    };
    for (frame, row) in out.chunks_mut(BINS).enumerate() {
        let start = signed(frame * HOP);
        for (k, b) in buffer.iter_mut().enumerate() {
            *b = Complex::new(sample(start + signed(k)) * hann[k], 0.0);
        }
        fft.process(buffer);
        for (value, bin) in row.iter_mut().zip(buffer.iter()) {
            *value = bin.norm_sqr();
        }
    }
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_wrap,
    reason = "positions in a window of samples are far below isize::MAX"
)]
/// A sample position as a signed one, to reflect about the window's ends.
const fn signed(position: usize) -> isize {
    position as isize
}

#[expect(
    clippy::as_conversions,
    clippy::cast_sign_loss,
    reason = "reflection leaves a position inside the window, so it is never negative"
)]
/// A reflected sample position as an index.
const fn unsigned(position: isize) -> usize {
    position as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[expect(
        clippy::as_conversions,
        clippy::cast_precision_loss,
        reason = "the tests' sample and bin positions stay below 2^24, where f32 is exact"
    )]
    /// A sample or bin position as a float.
    const fn position(index: usize) -> f32 {
        index as f32
    }

    #[test]
    fn a_sine_on_a_bin_puts_its_power_there() {
        let hann: Vec<f32> = (0..N_FFT)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * position(i) / position(N_FFT)).cos())
            .collect();
        let bin = 64;
        let x: Vec<f32> = (0..crate::audio::WINDOW)
            .map(|i| (2.0 * std::f32::consts::PI * position(bin * i) / position(N_FFT)).sin())
            .collect();
        let spec = power_spectrograms(&[&x], &hann);
        let row = &spec[1000 * BINS..1001 * BINS];
        let peak = row
            .iter()
            .enumerate()
            .fold(0, |best, (i, v)| if *v > row[best] { i } else { best });
        assert_eq!(peak, bin);
        // A unit sine through a Hann window: (N/4)^2 at its bin.
        assert!((row[bin] / (position(N_FFT) / 4.0).powi(2) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn reflection_mirrors_about_the_edge_sample() {
        let hann = vec![1.0; N_FFT];
        let mut x = vec![0.0_f32; crate::audio::WINDOW];
        x[1] = 1.0;
        let spec = power_spectrograms(&[&x], &hann);
        // Frame 0 sees x[512..=1] reflected, then x[0..512]: the impulse twice, at 511 and 513,
        // so its bin 0 power is (1 + 1)^2.
        assert!((spec[0] - 4.0).abs() < 1e-6);
    }
}
