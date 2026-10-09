//! Mapperatorinator's timing model behind an API that names no burn type, so nothing past it
//! depends on burn.

use anyhow::{Context, Result};
use burn::tensor::DeviceKind;
use burn::tensor::wgpu::wgpu::DeviceType;
use timingator::model::Model;
use timingator::tokenizer::Tokenizer;

#[expect(
    clippy::pub_use,
    clippy::useless_attribute,
    reason = "the facade hands on timingator's sample rate and timing points as they are"
)]
pub use timingator::{audio::SAMPLE_RATE, timing::TimingPoint};

/// Where the model runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Device {
    /// The graphics card, through wgpu.
    Gpu,
    /// The CPU, through flex.
    Cpu,
}

/// Mapperatorinator's timing model and its tokenizer, ready to time songs.
pub struct Timer {
    /// The model, on `device`.
    model: Model,
    /// Where the model runs.
    device: Device,
    /// The tokenizer the model's GGUF file holds.
    tokenizer: Tokenizer,
}

impl std::fmt::Debug for Timer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Timer")
            .field("device", &self.device)
            .finish_non_exhaustive()
    }
}

impl Timer {
    /// The model and tokenizer in `gguf`, the model's file, loaded on `device`.
    ///
    /// # Errors
    ///
    /// When `gguf` is not the model's file.
    pub fn load(gguf: &[u8], device: Device) -> Result<Self> {
        let tokenizer = timingator::gguf::Gguf::parse(gguf)?
            .metadata
            .get("timingator.tokenizer")
            .and_then(|value| value.as_str().map(str::to_owned))
            .context("no timingator.tokenizer")?;
        let tokenizer = Tokenizer::from_json(&tokenizer)?;
        let backend = match device {
            Device::Gpu => burn::tensor::Device::wgpu(DeviceKind::DefaultDevice),
            Device::Cpu => burn::tensor::Device::flex(),
        };
        let model = Model::load(gguf, &backend)?;
        Ok(Self {
            model,
            device,
            tokenizer,
        })
    }

    /// The timing points of `samples`, mono at [`SAMPLE_RATE`] and divided by their peak,
    /// calling `progress` with the windows decoded so far and in all.
    ///
    /// # Errors
    ///
    /// When the model fails on the audio.
    pub fn run(
        &self,
        samples: &[f32],
        progress: impl FnMut(usize, usize),
    ) -> Result<Vec<TimingPoint>> {
        let options = timingator::Options::default();
        let found = timingator::run(&self.model, &self.tokenizer, samples, &options, progress)?;
        Ok(found.timing_points)
    }
}

/// Whether wgpu finds a graphics card, rather than none or a software renderer that would be
/// slower than the CPU backend. Asking sets the device up, so ask once.
#[must_use]
pub fn graphics_card() -> bool {
    burn::tensor::Device::wgpu_options()
        .init_with_setup()
        .is_ok_and(|(_, setup)| setup.adapter.get_info().device_type != DeviceType::Cpu)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_that_are_not_the_model_s_file_are_refused() {
        Timer::load(b"not a gguf file", Device::Cpu).unwrap_err();
        Timer::load(&[], Device::Cpu).unwrap_err();
    }
}
