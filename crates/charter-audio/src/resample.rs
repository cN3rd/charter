//! Mono audio brought from one sample rate to another, band-limited, for the models that hear it
//! at a rate of their own.

use audioadapter_buffers::owned::InterleavedOwned;
use rubato::{Fft, FixedSync, Resampler};

/// Why audio would not resample.
#[derive(Debug, thiserror::Error)]
#[error("the audio would not resample: {0}")]
pub struct ResampleError(String);

/// `samples`, mono audio at `from` samples a second, at `to`.
///
/// # Errors
///
/// Returns [`ResampleError`] for a rate the resampler cannot convert between.
pub fn resample(samples: &[f32], from: u32, to: u32) -> Result<Vec<f32>, ResampleError> {
    if from == to || samples.is_empty() {
        return Ok(samples.to_vec());
    }
    let why = |error: &dyn std::fmt::Display| ResampleError(error.to_string());
    let input =
        InterleavedOwned::new_from(samples.to_vec(), 1, samples.len()).map_err(|e| why(&e))?;
    let mut resampler = Fft::<f32>::new(
        usize::try_from(from).map_err(|e| why(&e))?,
        usize::try_from(to).map_err(|e| why(&e))?,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_at_twice_the_rate_resamples_to_half_the_samples() {
        let samples = vec![0.25; 44_100];
        let resampled = resample(&samples, 44_100, 22_050).unwrap();
        assert!(resampled.len().abs_diff(22_050) <= 1, "{}", resampled.len());
        assert!(
            resampled[1000..21_000]
                .iter()
                .all(|s| (s - 0.25).abs() < 1e-3)
        );
    }

    #[test]
    fn non_integer_rate_conversion_preserves_duration_and_level() {
        let samples = vec![0.25; 4_800];
        let got = resample(&samples, 48_000, 22_050).unwrap();
        assert!(got.len().abs_diff(2_205) <= 1, "{}", got.len());
        assert!(
            got[200..2_000]
                .iter()
                .all(|sample| (sample - 0.25).abs() < 1e-3)
        );
        let sixteen = resample(&samples, 44_100, 16_000).unwrap();
        assert!(sixteen.len().abs_diff(1_741) <= 1, "{}", sixteen.len());
    }

    #[test]
    fn the_same_rate_is_unchanged_and_an_invalid_rate_is_reported() {
        let samples = [0.0, 0.25, -0.5, 1.0];
        assert_eq!(resample(&samples, 22_050, 22_050).unwrap(), samples);
        resample(&samples, 0, 22_050).unwrap_err();
        assert!(resample(&[], 44_100, 22_050).unwrap().is_empty());
    }
}
