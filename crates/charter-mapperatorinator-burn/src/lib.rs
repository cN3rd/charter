//! Song timing from audio: Mapperatorinator's super timing generator in Rust, running the V32
//! model in burn.
//!
//! A port of the timing part of OliBomby/Mapperatorinator (MIT). The model hears 16.4 seconds
//! of audio at a time and writes beat, measure and timing point events for it. Super timing
//! makes many passes over the song, each with the audio shifted by a random offset so the
//! windows split it in different places, and fuses the events into one beat grid, which becomes
//! osu! timing points.
//!
//! [`Timer`] runs it on the graphics card or the CPU behind an API that names no burn type, so
//! nothing past this crate depends on burn.

pub mod audio;
pub mod generate;
pub mod gguf;
pub mod mel;
pub mod model;
pub mod signal;
pub mod super_timing;
pub mod timing;
pub mod tokenizer;

use anyhow::{Context, Result};
use burn::tensor::DeviceKind;
use burn::tensor::wgpu::wgpu::DeviceType;
use serde::Serialize;

pub use crate::audio::SAMPLE_RATE;
use crate::audio::WINDOW_MS;
use crate::generate::{Conditions, Window};
use crate::model::Model;
use crate::super_timing::fuse;
use crate::timing::TimingBuilder;
pub use crate::timing::TimingPoint;
use crate::tokenizer::{Event, Tokenizer};

/// How super timing runs.
#[derive(Clone, Debug)]
pub struct Options {
    /// Passes over the song; upstream's `timer_iterations`.
    pub iterations: usize,
    /// How clearly one beat length must lead near a peak to set its tempo; higher values give
    /// fewer tempo changes. Upstream's `timer_bpm_threshold`.
    pub bpm_threshold: f64,
    /// Milliseconds a beat may be off the grid before a new timing point; `timing_leniency`.
    pub leniency: f64,
    /// Windows decoded at once.
    pub batch: usize,
    /// Windows encoded at once.
    pub encoder_batch: usize,
    /// Seed of the pass offsets.
    pub seed: u64,
    /// The pass offsets in milliseconds, instead of drawing them from `seed`.
    pub offsets: Option<Vec<i64>>,
    pub conditions: Conditions,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            iterations: 20,
            bpm_threshold: 0.1,
            leniency: 20.0,
            batch: 16,
            encoder_batch: 4,
            seed: 0,
            offsets: None,
            conditions: Conditions::default(),
        }
    }
}

/// A window of one pass: where it starts and the tokens decoded for it, `None` when decoding
/// stopped without closing the timing context.
#[derive(Clone, Debug, Serialize)]
pub struct WindowTokens {
    pub frame_time: i64,
    pub tokens: Option<Vec<u32>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Pass {
    pub offset: i64,
    pub windows: Vec<WindowTokens>,
}

/// Everything a run found, from each window's tokens to the timing points.
pub struct Timing {
    pub passes: Vec<Pass>,
    /// The fused beat grid, as time shift and type events.
    pub grid: Vec<Event>,
    pub timing_points: Vec<TimingPoint>,
}

/// SplitMix64, to draw pass offsets from a seed without a dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in `low..high`.
    fn range(&mut self, low: i64, high: i64) -> i64 {
        low + (self.next() % (high - low) as u64) as i64
    }
}

/// The pass offsets for `options`: its own, or `iterations` drawn uniformly from
/// `-(window // 2)..window // 2` milliseconds, as upstream's `np.random.randint`.
pub fn offsets(options: &Options) -> Vec<i64> {
    options.offsets.clone().unwrap_or_else(|| {
        let half = (WINDOW_MS / 2.0).floor() as i64;
        let mut rng = Rng(options.seed);
        (0..options.iterations)
            .map(|_| rng.range(-half, half))
            .collect()
    })
}

/// Times the song `samples`, normalized mono at 16 kHz, calling `progress` with the windows
/// decoded so far and in all.
pub fn run(
    model: &Model,
    tokenizer: &Tokenizer,
    samples: &[f32],
    options: &Options,
    mut progress: impl FnMut(usize, usize),
) -> Result<Timing> {
    let offsets = offsets(options);
    let per_ms = (SAMPLE_RATE / 1000) as i64;
    let segments: Vec<audio::Segments> = offsets
        .iter()
        .map(|&offset| {
            let begin_pad = (offset * per_ms).max(0) as usize;
            let begin_remove = ((-offset * per_ms).max(0) as usize).min(samples.len());
            audio::segment(&samples[begin_remove..], begin_pad)
        })
        .collect();
    let jobs: Vec<(usize, usize)> = segments
        .iter()
        .enumerate()
        .flat_map(|(p, s)| (0..s.windows.len()).map(move |w| (p, w)))
        .collect();

    let mut tokens: Vec<Option<Vec<u32>>> = Vec::with_capacity(jobs.len());
    for chunk in jobs.chunks(options.batch.max(1)) {
        let windows: Vec<Window<'_>> = chunk
            .iter()
            .map(|&(p, w)| Window {
                samples: &segments[p].windows[w],
                frame_time: segments[p].frame_times[w],
                song_length: segments[p].song_length,
            })
            .collect();
        tokens.extend(generate::decode(
            model,
            tokenizer,
            &options.conditions,
            &windows,
            options.encoder_batch,
        )?);
        progress(tokens.len(), jobs.len());
    }

    let mut passes: Vec<Pass> = offsets
        .iter()
        .map(|&offset| Pass {
            offset,
            windows: Vec::new(),
        })
        .collect();
    let mut pass_events: Vec<(i64, Vec<Event>)> =
        offsets.iter().map(|&o| (o, Vec::new())).collect();
    for (&(p, w), t) in jobs.iter().zip(tokens) {
        let frame_time = segments[p].frame_times[w];
        if let Some(t) = &t {
            pass_events[p]
                .1
                .extend(generate::events(tokenizer, t, frame_time));
        }
        passes[p].windows.push(WindowTokens {
            frame_time,
            tokens: t,
        });
    }

    let num_ms = samples.len() * 1000 / SAMPLE_RATE;
    let grid = fuse(num_ms, &pass_events, options.bpm_threshold)?;
    let timing_points = TimingBuilder {
        leniency: options.leniency,
    }
    .generate_timing(&grid);
    Ok(Timing {
        passes,
        grid,
        timing_points,
    })
}

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
        let tokenizer = gguf::Gguf::parse(gguf)?
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
        let options = Options::default();
        let found = run(&self.model, &self.tokenizer, samples, &options, progress)?;
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

    #[test]
    fn offsets_repeat_for_a_seed_and_stay_within_half_a_window() {
        let o = Options {
            seed: 7,
            ..Options::default()
        };
        assert_eq!(offsets(&o), offsets(&o));
        assert_eq!(offsets(&o).len(), 20);
        assert!(offsets(&o).iter().all(|&x| (-8188..8188).contains(&x)));
        let given = Options {
            offsets: Some(vec![5, -3]),
            ..Options::default()
        };
        assert_eq!(offsets(&given), vec![5, -3]);
    }
}
