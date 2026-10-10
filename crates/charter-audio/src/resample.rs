//! Mono audio brought from one sample rate to another, band-limited, for the models that hear it
//! at a rate of their own.

use audioadapter_buffers::owned::InterleavedOwned;
use rubato::{Fft, FixedSync, Resampler};

use crate::SampleRate;

/// Mono samples with their sample rate.
#[derive(Clone, Debug, PartialEq)]
pub struct Mono {
    /// Samples per second.
    rate: SampleRate,
    /// Audio from first to last sample.
    samples: Vec<f32>,
}

impl Mono {
    /// Mono `samples` recorded at `rate`.
    #[must_use]
    pub const fn new(rate: SampleRate, samples: Vec<f32>) -> Self {
        Self { rate, samples }
    }

    /// Samples per second.
    #[must_use]
    pub const fn rate(&self) -> SampleRate {
        self.rate
    }

    /// Samples in playback order.
    #[must_use]
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// Takes the samples for an API with its own fixed rate.
    #[must_use]
    pub fn into_samples(self) -> Vec<f32> {
        self.samples
    }

    /// The same audio at `to` samples per second.
    ///
    /// # Errors
    ///
    /// Returns [`ResampleError`] when the resampler cannot convert between the rates.
    pub fn resampled(&self, to: SampleRate) -> Result<Self, ResampleError> {
        Ok(Self {
            rate: to,
            samples: resample(&self.samples, self.rate.get(), to.get())?,
        })
    }
}

/// Why audio would not resample.
#[derive(Debug, thiserror::Error)]
#[error("the audio would not resample: {0}")]
pub struct ResampleError(String);

/// `samples`, mono audio at `from` samples a second, at `to`.
///
/// # Errors
///
/// Returns [`ResampleError`] for a rate the resampler cannot convert between.
fn resample(samples: &[f32], from: u32, to: u32) -> Result<Vec<f32>, ResampleError> {
    if from == to || samples.is_empty() {
        return Ok(samples.to_vec());
    }
    let input = InterleavedOwned::new_from(samples.to_vec(), 1, samples.len()).map_err(why)?;
    let mut resampler = Fft::<f32>::new(
        usize::try_from(from).map_err(why)?,
        usize::try_from(to).map_err(why)?,
        1024,
        1,
        FixedSync::Input,
    )
    .map_err(why)?;
    let output = resampler
        .process_all(&input, samples.len(), None)
        .map_err(why)?;
    Ok(output.take_data())
}

/// `error` as the reason audio would not resample.
fn why(error: impl std::fmt::Display) -> ResampleError {
    ResampleError(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SampleRate, Track};

    #[test]
    fn mono_keeps_its_source_rate_and_resampling_sets_the_target_rate() {
        let mono = Track::from_frames(48_000, vec![[0.0, 0.5]; 4_800]).to_mono();
        assert_eq!(mono.rate(), SampleRate::new(48_000).unwrap());
        assert_eq!(mono.samples()[0], 0.25);
        let slower = mono.resampled(SampleRate::new(24_000).unwrap()).unwrap();
        assert_eq!(slower.rate(), SampleRate::new(24_000).unwrap());
        assert!(slower.samples().len().abs_diff(2_400) <= 1);
        assert!(
            slower.samples()[200..2_000]
                .iter()
                .all(|sample| (sample - 0.25).abs() < 1e-3)
        );
        assert_eq!(mono.samples().len(), 4_800);
    }

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
