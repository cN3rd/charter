//! Mapperatorinator's timing model behind an API without burn's generics, so its kernels compile
//! here, once, rather than in every crate that runs it.

use anyhow::{Context, Result};
use burn::backend::flex::{Flex, FlexDevice};
use burn::backend::wgpu::{Wgpu, WgpuDevice};
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

/// The model loaded on one backend.
enum Loaded {
    /// On the graphics card.
    Gpu(Model<Wgpu>),
    /// On the CPU.
    Cpu(Model<Flex>),
}

/// Mapperatorinator's timing model and its tokenizer, ready to time songs.
pub struct Timer {
    /// The model on its device.
    model: Loaded,
    /// The tokenizer the model's GGUF file holds.
    tokenizer: Tokenizer,
}

impl std::fmt::Debug for Timer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let device = match self.model {
            Loaded::Gpu(_) => Device::Gpu,
            Loaded::Cpu(_) => Device::Cpu,
        };
        f.debug_struct("Timer")
            .field("device", &device)
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
        let model = match device {
            Device::Gpu => Loaded::Gpu(Model::load(gguf, &WgpuDevice::DefaultDevice)?),
            Device::Cpu => Loaded::Cpu(Model::load(gguf, &FlexDevice)?),
        };
        Ok(Self { model, tokenizer })
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
        let found = match &self.model {
            Loaded::Gpu(model) => {
                timingator::run(model, &self.tokenizer, samples, &options, progress)
            }
            Loaded::Cpu(model) => {
                timingator::run(model, &self.tokenizer, samples, &options, progress)
            }
        }?;
        Ok(found.timing_points)
    }
}

/// Whether wgpu finds a graphics card, rather than none or a software renderer that would be
/// slower than the CPU backend. Asking sets the device up.
///
/// cubecl panics when wgpu has no adapter; that panic is caught here, so a caller with a panic
/// hook may want to keep it from being reported.
#[must_use]
pub fn graphics_card() -> bool {
    use burn::backend::wgpu::graphics::AutoGraphicsApi;
    use burn::backend::wgpu::{RuntimeOptions, init_setup};

    // ponytail: the adapter's type is named only in wgpu's own enum; its Debug name saves
    // depending on that wgpu for one comparison.
    std::panic::catch_unwind(|| {
        init_setup::<AutoGraphicsApi>(&WgpuDevice::DefaultDevice, RuntimeOptions::default())
    })
    .is_ok_and(|setup| format!("{:?}", setup.adapter.get_info().device_type) != "Cpu")
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
