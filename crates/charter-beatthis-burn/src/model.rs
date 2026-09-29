//! The Beat This! network as the reference defines it in `beat_this/model`: a convolutional stem
//! over the spectrogram, three blocks that each attend across frequency and then across time
//! before halving the frequencies, six transformer layers over time, and one linear head whose
//! downbeat logit is added to its beat logit. Every attention rotates its queries and keys with
//! the same rotary embedding. Field names follow the checkpoint's module paths, so its weights
//! load by name once `lib.rs` remaps a few.

#![expect(
    clippy::shadow_same,
    reason = "burn's Module derive rebinds struct fields internally"
)]

use burn::module::Param;
use burn::nn::conv::{Conv2d, Conv2dConfig};
use burn::nn::{BatchNorm, BatchNormConfig, Linear, LinearConfig, PaddingConfig2d};
use burn::prelude::*;
use burn::tensor::activation::{gelu, sigmoid};
use burn::tensor::module::attention;
use burn::tensor::ops::AttentionModuleOptions;

use crate::mel::BINS;

/// The sizes a Beat This! network is built with.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Dims {
    /// Channels out of the stem; each frontend block doubles them.
    pub(crate) stem: usize,
    /// Width of every attention head.
    pub(crate) head: usize,
    /// Width of the transformer over time.
    pub(crate) transformer: usize,
    /// Number of attention and feed-forward layers over time.
    pub(crate) layers: usize,
    /// How much wider a feed-forward's inside is than its outside.
    pub(crate) ff_mult: usize,
}

impl Dims {
    /// The released model's.
    pub(crate) const FULL: Self = Self {
        stem: 32,
        head: 32,
        transformer: 512,
        layers: 6,
        ff_mult: 4,
    };

    /// The seeded model the parity fixtures were made with.
    #[cfg(test)]
    pub(crate) const TINY: Self = Self {
        stem: 8,
        head: 8,
        transformer: 16,
        layers: 1,
        ff_mult: 4,
    };
}

/// Normalisation by the root mean square, as `F.normalize(x) * sqrt(size) * gamma`.
#[derive(Module, Debug)]
struct RmsNorm<B: Backend> {
    /// Per-feature scale learned after RMS normalisation.
    gamma: Param<Tensor<B, 1>>,
    /// Square root of the feature count, restoring the input's scale after normalisation.
    scale: f64,
}

impl<B: Backend> RmsNorm<B> {
    /// Unit `gamma` for `size` features on `device`.
    fn new(size: usize, device: &B::Device) -> Self {
        let scale = width_float(size).sqrt();
        Self {
            gamma: Param::from_tensor(Tensor::ones([size], device)),
            scale,
        }
    }

    /// `x` normalised across its last dimension and scaled per feature.
    fn forward<const D: usize>(&self, x: Tensor<B, D>) -> Tensor<B, D> {
        let norm = x
            .clone()
            .powf_scalar(2.0)
            .sum_dim(D.saturating_sub(1))
            .sqrt()
            .clamp_min(1e-12);
        x.div(norm)
            .mul_scalar(self.scale)
            .mul(self.gamma.val().unsqueeze::<D>())
    }
}

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "network dimensions and chunk positions are converted to floating-point angles"
)]
/// `width` as an angle-compatible floating-point value.
const fn width_float(width: usize) -> f64 {
    width as f64
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "rotary angles are stored as f32 tensors"
)]
/// `angle` narrowed to the precision of burn's angle tensors.
const fn angle_float(angle: f64) -> f32 {
    angle as f32
}

/// Normalise, widen, GELU, narrow.
#[derive(Module, Debug)]
struct FeedForward<B: Backend> {
    /// Normalisation applied before the two projections.
    norm: RmsNorm<B>,
    /// Projection to `mult` times the input width.
    inner: Linear<B>,
    /// Projection back to the input width.
    outer: Linear<B>,
}

impl<B: Backend> FeedForward<B> {
    /// A feed-forward block of width `dim` with expansion factor `mult`.
    fn new(dim: usize, mult: usize, device: &B::Device) -> Self {
        Self {
            norm: RmsNorm::new(dim, device),
            inner: LinearConfig::new(dim, dim.saturating_mul(mult)).init(device),
            outer: LinearConfig::new(dim.saturating_mul(mult), dim).init(device),
        }
    }

    /// `x` after normalisation, expansion, GELU and projection to its original width.
    fn forward(&self, x: Tensor<B, 3>) -> Tensor<B, 3> {
        self.outer
            .forward(gelu(self.inner.forward(self.norm.forward(x))))
    }
}

/// Self-attention over a sequence, its queries and keys rotated by position, each head's
/// output gated by a sigmoid of the normalised input.
#[derive(Module, Debug)]
struct Attention<B: Backend> {
    /// Normalisation shared by the query and gate projections.
    norm: RmsNorm<B>,
    /// Packed projections of queries, keys and values for all heads.
    to_qkv: Linear<B>,
    /// One sigmoid gate per attention head.
    to_gates: Linear<B>,
    /// Projection from the combined heads to the input width.
    to_out: Linear<B>,
    /// Number of heads fitted into the input width.
    heads: usize,
    /// Feature width of each head.
    head: usize,
}

impl<B: Backend> Attention<B> {
    /// Rotary, gated attention over `dim` features in heads of width `head`.
    fn new(dim: usize, head: usize, device: &B::Device) -> Self {
        let heads = dim.div_euclid(head);
        Self {
            norm: RmsNorm::new(dim, device),
            to_qkv: LinearConfig::new(dim, 3_usize.saturating_mul(heads).saturating_mul(head))
                .with_bias(false)
                .init(device),
            to_gates: LinearConfig::new(dim, heads).init(device),
            to_out: LinearConfig::new(heads.saturating_mul(head), dim)
                .with_bias(false)
                .init(device),
            heads,
            head,
        }
    }

    /// `x` of `[batch, sequence, dim]` attended.
    fn forward(&self, x: Tensor<B, 3>) -> Tensor<B, 3> {
        let [batch, len, _] = x.dims();
        let (heads, head) = (self.heads, self.head);
        let normalized = self.norm.forward(x);
        let qkv = self.to_qkv.forward(normalized.clone());
        // The projection holds queries, then keys, then values, each head after head.
        let part = |index: usize| {
            qkv.clone()
                .narrow(
                    2,
                    index.saturating_mul(heads).saturating_mul(head),
                    heads.saturating_mul(head),
                )
                .reshape([batch, len, heads, head])
                .swap_dims(1, 2)
        };
        let (q, k, v) = (rotated(part(0)), rotated(part(1)), part(2));
        // softmax(q kᵀ / √head) v, fused, so the scores of a chunk's 1500 frames are never all
        // held at once.
        let out = attention(q, k, v, None, None, AttentionModuleOptions::default());
        let gates = sigmoid(self.to_gates.forward(normalized))
            .swap_dims(1, 2)
            .unsqueeze_dim::<4>(3);
        let out = out
            .mul(gates)
            .swap_dims(1, 2)
            .reshape([batch, len, heads.saturating_mul(head)]);
        self.to_out.forward(out)
    }
}

/// `x` of `[batch, heads, sequence, width]` with each pair of features rotated by its position
/// in the sequence, pair `i` at `10000^(-2i / width)` radians a step, as `rotary_embedding_torch`
/// rotates interleaved pairs.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "tensor addition combines the rotated features elementwise"
)]
fn rotated<B: Backend>(x: Tensor<B, 4>) -> Tensor<B, 4> {
    let [batch, heads, len, width] = x.dims();
    let device = x.device();
    let pairs = width.div_euclid(2);
    let mut angles = Vec::with_capacity(len.saturating_mul(width));
    for position in 0..len {
        for feature in 0..width {
            let (position_float, pair) =
                (width_float(position), width_float(feature.div_euclid(2)));
            let rate = 1.0 / 10_000_f64.powf(2.0 * pair / width_float(width));
            angles.push(angle_float(position_float * rate));
        }
    }
    let angles =
        Tensor::<B, 1>::from_floats(angles.as_slice(), &device).reshape([1, 1, len, width]);
    let halves = x.clone().reshape([batch, heads, len, pairs, 2]);
    let first = halves.clone().narrow(4, 0, 1);
    let second = halves.narrow(4, 1, 1);
    let turned = Tensor::cat(vec![second.neg(), first], 4).reshape([batch, heads, len, width]);
    x.mul(angles.clone().cos()) + turned.mul(angles.sin())
}

/// Attention and feed-forward across frequency, then across time, each added to its input.
#[derive(Module, Debug)]
struct PartialFt<B: Backend> {
    /// Attention across mel frequencies at each time step.
    attn_f: Attention<B>,
    /// Feed-forward projection after frequency attention.
    ff_f: FeedForward<B>,
    /// Attention across time at each mel frequency.
    attn_t: Attention<B>,
    /// Feed-forward projection after time attention.
    ff_t: FeedForward<B>,
}

impl<B: Backend> PartialFt<B> {
    /// Frequency and time residual blocks operating on `dim` channels.
    fn new(dim: usize, head: usize, device: &B::Device) -> Self {
        Self {
            attn_f: Attention::new(dim, head, device),
            ff_f: FeedForward::new(dim, 4, device),
            attn_t: Attention::new(dim, head, device),
            ff_t: FeedForward::new(dim, 4, device),
        }
    }

    /// `x` of `[batch, channels, freqs, time]`.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "tensor additions are the network's residual connections"
    )]
    fn forward(&self, x: Tensor<B, 4>) -> Tensor<B, 4> {
        let [batch, channels, freqs, time] = x.dims();
        let sequence =
            x.permute([0, 3, 2, 1])
                .reshape([batch.saturating_mul(time), freqs, channels]);
        let sequence = sequence.clone() + self.attn_f.forward(sequence);
        let sequence = sequence.clone() + self.ff_f.forward(sequence);
        let sequence = sequence
            .reshape([batch, time, freqs, channels])
            .permute([0, 2, 1, 3])
            .reshape([batch.saturating_mul(freqs), time, channels]);
        let sequence = sequence.clone() + self.attn_t.forward(sequence);
        let sequence = sequence.clone() + self.ff_t.forward(sequence);
        sequence
            .reshape([batch, freqs, time, channels])
            .permute([0, 3, 1, 2])
    }
}

/// Across frequency and time, then a convolution halving the frequencies and doubling the
/// channels.
#[derive(Module, Debug)]
struct FrontendBlock<B: Backend> {
    /// Frequency and time residual attention before downsampling.
    partial: PartialFt<B>,
    /// Convolution halving the frequency count and doubling channels.
    conv2d: Conv2d<B>,
    /// Normalisation of the doubled channels.
    norm: BatchNorm<B>,
}

impl<B: Backend> FrontendBlock<B> {
    /// One frequency-halving block starting with `dim` channels.
    fn new(dim: usize, head: usize, device: &B::Device) -> Self {
        Self {
            partial: PartialFt::new(dim, head, device),
            conv2d: Conv2dConfig::new([dim, dim.saturating_mul(2)], [2, 3])
                .with_stride([2, 1])
                .with_padding(PaddingConfig2d::Explicit(0, 1, 0, 1))
                .with_bias(false)
                .init(device),
            norm: BatchNormConfig::new(dim.saturating_mul(2)).init(device),
        }
    }

    /// `x` after attention, downsampling, normalisation and GELU.
    fn forward(&self, x: Tensor<B, 4>) -> Tensor<B, 4> {
        gelu(
            self.norm
                .forward(self.conv2d.forward(self.partial.forward(x))),
        )
    }
}

/// Normalise each mel band, then a convolution over four bands at a time.
#[derive(Module, Debug)]
struct Stem<B: Backend> {
    /// Normalisation across the input's mel bands.
    bn1d: BatchNorm<B>,
    /// Strided convolution reducing mel bands by four.
    conv2d: Conv2d<B>,
    /// Normalisation across the stem's output channels.
    bn2d: BatchNorm<B>,
}

impl<B: Backend> Stem<B> {
    /// The spectrogram-to-channel projection at width `stem`.
    fn new(stem: usize, device: &B::Device) -> Self {
        Self {
            bn1d: BatchNormConfig::new(BINS).init(device),
            conv2d: Conv2dConfig::new([1, stem], [4, 3])
                .with_stride([4, 1])
                .with_padding(PaddingConfig2d::Explicit(0, 1, 0, 1))
                .with_bias(false)
                .init(device),
            bn2d: BatchNormConfig::new(stem).init(device),
        }
    }

    /// `x` of `[batch, time, bins]` as `[batch, channels, bins / 4, time]`.
    fn forward(&self, x: Tensor<B, 3>) -> Tensor<B, 4> {
        let x = self.bn1d.forward(x.swap_dims(1, 2)).unsqueeze_dim::<4>(1);
        gelu(self.bn2d.forward(self.conv2d.forward(x)))
    }
}

/// The spectrogram stem and three frequency-halving blocks, projected into transformer width.
#[derive(Module, Debug)]
struct Frontend<B: Backend> {
    /// First convolution from mel bands into channels.
    stem: Stem<B>,
    /// Three blocks of attention and frequency downsampling.
    blocks: Vec<FrontendBlock<B>>,
    /// Flattens each time step's channels and frequencies into transformer width.
    linear: Linear<B>,
}

impl<B: Backend> Frontend<B> {
    /// A frontend with the channel and attention widths in `dims`.
    fn new(dims: Dims, device: &B::Device) -> Self {
        let blocks = (0..3)
            .map(|block| FrontendBlock::new(dims.stem << block, dims.head, device))
            .collect();
        // Three blocks double the stem's channels and halve its quarter of the bands each.
        let channels = dims.stem << 3;
        let freqs = BINS.div_euclid(4) >> 3;
        Self {
            stem: Stem::new(dims.stem, device),
            blocks,
            linear: LinearConfig::new(channels.saturating_mul(freqs), dims.transformer)
                .init(device),
        }
    }

    /// Mel frames `x` projected into one transformer-width vector per time step.
    fn forward(&self, x: Tensor<B, 3>) -> Tensor<B, 3> {
        let mut x = self.stem.forward(x);
        for block in &self.blocks {
            x = block.forward(x);
        }
        let [batch, channels, freqs, time] = x.dims();
        let x = x
            .permute([0, 3, 1, 2])
            .reshape([batch, time, channels.saturating_mul(freqs)]);
        self.linear.forward(x)
    }
}

/// One attention and feed-forward residual pair over time.
#[derive(Module, Debug)]
struct Layer<B: Backend> {
    /// Rotary self-attention across time.
    attn: Attention<B>,
    /// Feed-forward network after attention.
    ff: FeedForward<B>,
}

/// Stacked temporal residual pairs with a final RMS normalisation.
#[derive(Module, Debug)]
struct Transformer<B: Backend> {
    /// The `dims.layers` attention and feed-forward pairs.
    layers: Vec<Layer<B>>,
    /// Output normalisation after the last residual pair.
    norm: RmsNorm<B>,
}

impl<B: Backend> Transformer<B> {
    /// `dims.layers` temporal blocks of width `dims.transformer`.
    fn new(dims: Dims, device: &B::Device) -> Self {
        let layers = std::iter::repeat_with(|| Layer {
            attn: Attention::new(dims.transformer, dims.head, device),
            ff: FeedForward::new(dims.transformer, dims.ff_mult, device),
        })
        .take(dims.layers)
        .collect();
        Self {
            layers,
            norm: RmsNorm::new(dims.transformer, device),
        }
    }

    #[expect(
        clippy::arithmetic_side_effects,
        reason = "tensor additions are the transformer's residual connections"
    )]
    /// `x` after temporal residual layers and final normalisation.
    fn forward(&self, x: Tensor<B, 3>) -> Tensor<B, 3> {
        let mut transformed = x;
        for layer in &self.layers {
            transformed = layer.attn.forward(transformed.clone()) + transformed;
            transformed = layer.ff.forward(transformed.clone()) + transformed;
        }
        self.norm.forward(transformed)
    }
}

/// A beat and a downbeat logit a frame, the downbeat's added to the beat's.
#[derive(Module, Debug)]
struct SumHead<B: Backend> {
    /// Projection to separate beat and downbeat logits before the latter is added to the former.
    beat_downbeat_lin: Linear<B>,
}

/// The full spectrogram-to-beat network whose field paths match the released checkpoint.
#[derive(Module, Debug)]
pub(crate) struct BeatThis<B: Backend> {
    /// Stem and frequency-reducing attention blocks.
    frontend: Frontend<B>,
    /// Attention across the projected frame sequence.
    transformer_blocks: Transformer<B>,
    /// Beat and downbeat projections.
    task_heads: SumHead<B>,
}

impl<B: Backend> BeatThis<B> {
    /// An unweighted model with the dimensions needed to load its checkpoint.
    pub(crate) fn new(dims: Dims, device: &B::Device) -> Self {
        Self {
            frontend: Frontend::new(dims, device),
            transformer_blocks: Transformer::new(dims, device),
            task_heads: SumHead {
                beat_downbeat_lin: LinearConfig::new(dims.transformer, 2).init(device),
            },
        }
    }

    /// The beat and downbeat logits of each frame of `spect`, `[1, time, bins]`.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "tensor addition combines beat and downbeat logits elementwise"
    )]
    pub(crate) fn forward(&self, spect: Tensor<B, 3>) -> (Tensor<B, 1>, Tensor<B, 1>) {
        let x = self
            .transformer_blocks
            .forward(self.frontend.forward(spect));
        let [_, time, _] = x.dims();
        let logits = self
            .task_heads
            .beat_downbeat_lin
            .forward(x)
            .reshape([time, 2]);
        let downbeat = logits.clone().narrow(1, 1, 1).reshape([time]);
        let beat = logits.narrow(1, 0, 1).reshape([time]) + downbeat.clone();
        (beat, downbeat)
    }
}
