//! The power spectrogram under the model's mel spectrogram, as torchaudio's `Spectrogram` makes
//! it for upstream's `MelSpectrogram` (`osuT5/osuT5/model/spectrogram.py`): a 1024-sample Hann
//! window every 128 samples, the signal reflected at both ends so frames centre on their hop,
//! squared magnitudes of the one-sided FFT. The mel filterbank and the log follow on the
//! device, in `model.rs`, with the filterbank and window the checkpoint stores.

use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

use crate::audio::{FRAMES, HOP};

pub const N_FFT: usize = 1024;
pub const BINS: usize = N_FFT / 2 + 1;
/// Frames of a window: one per hop plus the one centring adds.
pub const MEL_FRAMES: usize = FRAMES + 1;

/// The power spectrogram of each window in `windows`, `[MEL_FRAMES, BINS]` row-major one after
/// another, computed on all cores.
pub fn power_spectrograms(windows: &[&[f32]], hann: &[f32]) -> Vec<f32> {
    let mut out = vec![0.0_f32; windows.len() * MEL_FRAMES * BINS];
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(windows.len().max(1));
    let per = windows.len().div_ceil(threads.max(1)).max(1);
    std::thread::scope(|scope| {
        for (chunk_windows, chunk_out) in windows
            .chunks(per)
            .zip(out.chunks_mut(per * MEL_FRAMES * BINS))
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

fn power_spectrogram(
    x: &[f32],
    hann: &[f32],
    fft: &dyn rustfft::Fft<f32>,
    buffer: &mut [Complex<f32>],
    out: &mut [f32],
) {
    let half = N_FFT / 2;
    let n = x.len() as isize;
    // torch.nn.functional.pad(mode="reflect"): the edge sample is not repeated.
    let sample = |i: isize| -> f32 {
        let j = i - half as isize;
        let j = if j < 0 {
            -j
        } else if j >= n {
            2 * (n - 1) - j
        } else {
            j
        };
        x[j as usize]
    };
    for (frame, row) in out.chunks_mut(BINS).enumerate() {
        let start = (frame * HOP) as isize;
        for (k, b) in buffer.iter_mut().enumerate() {
            *b = Complex::new(sample(start + k as isize) * hann[k], 0.0);
        }
        fft.process(buffer);
        for (bin, value) in row.iter_mut().enumerate() {
            *value = buffer[bin].norm_sqr();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sine_on_a_bin_puts_its_power_there() {
        let hann: Vec<f32> = (0..N_FFT)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / N_FFT as f32).cos())
            .collect();
        let bin = 64;
        let x: Vec<f32> = (0..crate::audio::WINDOW)
            .map(|i| (2.0 * std::f32::consts::PI * (bin * i) as f32 / N_FFT as f32).sin())
            .collect();
        let spec = power_spectrograms(&[&x], &hann);
        let row = &spec[1000 * BINS..1001 * BINS];
        let peak = row
            .iter()
            .enumerate()
            .fold(0, |best, (i, v)| if *v > row[best] { i } else { best });
        assert_eq!(peak, bin);
        // A unit sine through a Hann window: (N/4)^2 at its bin.
        assert!((row[bin] / (N_FFT as f32 / 4.0).powi(2) - 1.0).abs() < 1e-3);
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
