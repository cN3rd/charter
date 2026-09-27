//! The Beat This! network as the reference defines it in `beat_this/model`: a convolutional stem
//! over the spectrogram, three blocks that each attend across frequency and then across time
//! before halving the frequencies, six transformer layers over time, and one linear head whose
//! downbeat logit is added to its beat logit. Every attention rotates its queries and keys with
//! the same rotary embedding. Field names follow the checkpoint's module paths, so its weights
//! load by name once `lib.rs` remaps a few.

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
pub struct Dims {
    /// Channels out of the stem; each frontend block doubles them.
    pub stem: usize,
    /// Width of every attention head.
    pub head: usize,
    /// Width of the transformer over time.
    pub transformer: usize,
    pub layers: usize,
    /// How much wider a feed-forward's inside is than its outside.
    pub ff_mult: usize,
}

impl Dims {
    /// The released model's.
    pub const FULL: Self = Self {
        stem: 32,
        head: 32,
        transformer: 512,
        layers: 6,
        ff_mult: 4,
    };

    /// The seeded model the parity fixtures were made with.
    #[cfg(test)]
    pub const TINY: Self = Self {
        stem: 8,
        head: 8,
        transformer: 16,
        layers: 1,
        ff_mult: 4,
    };
}

/// Normalisation by the root mean square, as `F.normalize(x) * sqrt(size) * gamma`.
#[derive(Module, Debug)]
pub struct RmsNorm<B: Backend> {
    gamma: Param<Tensor<B, 1>>,
    scale: f64,
}

impl<B: Backend> RmsNorm<B> {
    fn new(size: usize, device: &B::Device) -> Self {
        #[allow(clippy::cast_precision_loss)] // a layer's width
        let scale = (size as f64).sqrt();
        Self {
            gamma: Param::from_tensor(Tensor::ones([size], device)),
            scale,
        }
    }

    fn forward<const D: usize>(&self, x: Tensor<B, D>) -> Tensor<B, D> {
        let norm = x
            .clone()
            .powf_scalar(2.0)
            .sum_dim(D - 1)
            .sqrt()
            .clamp_min(1e-12);
        x.div(norm)
            .mul_scalar(self.scale)
            .mul(self.gamma.val().unsqueeze::<D>())
    }
}

/// Normalise, widen, GELU, narrow.
#[derive(Module, Debug)]
pub struct FeedForward<B: Backend> {
    norm: RmsNorm<B>,
    inner: Linear<B>,
    outer: Linear<B>,
}

impl<B: Backend> FeedForward<B> {
    fn new(dim: usize, mult: usize, device: &B::Device) -> Self {
        Self {
            norm: RmsNorm::new(dim, device),
            inner: LinearConfig::new(dim, dim * mult).init(device),
            outer: LinearConfig::new(dim * mult, dim).init(device),
        }
    }

    fn forward(&self, x: Tensor<B, 3>) -> Tensor<B, 3> {
        self.outer
            .forward(gelu(self.inner.forward(self.norm.forward(x))))
    }
}

/// Self-attention over a sequence, its queries and keys rotated by position, each head's
/// output gated by a sigmoid of the normalised input.
#[derive(Module, Debug)]
pub struct Attention<B: Backend> {
    norm: RmsNorm<B>,
    to_qkv: Linear<B>,
    to_gates: Linear<B>,
    to_out: Linear<B>,
    heads: usize,
    head: usize,
}

impl<B: Backend> Attention<B> {
    fn new(dim: usize, head: usize, device: &B::Device) -> Self {
        let heads = dim / head;
        Self {
            norm: RmsNorm::new(dim, device),
            to_qkv: LinearConfig::new(dim, 3 * heads * head)
                .with_bias(false)
                .init(device),
            to_gates: LinearConfig::new(dim, heads).init(device),
            to_out: LinearConfig::new(heads * head, dim)
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
        let x = self.norm.forward(x);
        let qkv = self.to_qkv.forward(x.clone());
        // The projection holds queries, then keys, then values, each head after head.
        let part = |index: usize| {
            qkv.clone()
                .narrow(2, index * heads * head, heads * head)
                .reshape([batch, len, heads, head])
                .swap_dims(1, 2)
        };
        let (q, k, v) = (rotated(part(0)), rotated(part(1)), part(2));
        // softmax(q kᵀ / √head) v, fused, so the scores of a chunk's 1500 frames are never all
        // held at once.
        let out = attention(q, k, v, None, None, AttentionModuleOptions::default());
        let gates = sigmoid(self.to_gates.forward(x))
            .swap_dims(1, 2)
            .unsqueeze_dim::<4>(3);
        let out = out
            .mul(gates)
            .swap_dims(1, 2)
            .reshape([batch, len, heads * head]);
        self.to_out.forward(out)
    }
}

/// `x` of `[batch, heads, sequence, width]` with each pair of features rotated by its position
/// in the sequence, pair `i` at `10000^(-2i / width)` radians a step, as `rotary_embedding_torch`
/// rotates interleaved pairs.
fn rotated<B: Backend>(x: Tensor<B, 4>) -> Tensor<B, 4> {
    let [batch, heads, len, width] = x.dims();
    let device = x.device();
    let pairs = width / 2;
    let mut angles = Vec::with_capacity(len * width);
    for position in 0..len {
        for feature in 0..width {
            #[allow(clippy::cast_precision_loss)] // positions and widths of a chunk
            let (position, pair) = (position as f64, (feature / 2) as f64);
            #[allow(clippy::cast_precision_loss)]
            let rate = 1.0 / 10_000_f64.powf(2.0 * pair / width as f64);
            #[allow(clippy::cast_possible_truncation)] // an angle
            angles.push((position * rate) as f32);
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
pub struct PartialFt<B: Backend> {
    attn_f: Attention<B>,
    ff_f: FeedForward<B>,
    attn_t: Attention<B>,
    ff_t: FeedForward<B>,
}

impl<B: Backend> PartialFt<B> {
    fn new(dim: usize, head: usize, device: &B::Device) -> Self {
        Self {
            attn_f: Attention::new(dim, head, device),
            ff_f: FeedForward::new(dim, 4, device),
            attn_t: Attention::new(dim, head, device),
            ff_t: FeedForward::new(dim, 4, device),
        }
    }

    /// `x` of `[batch, channels, freqs, time]`.
    fn forward(&self, x: Tensor<B, 4>) -> Tensor<B, 4> {
        let [batch, channels, freqs, time] = x.dims();
        let x = x
            .permute([0, 3, 2, 1])
            .reshape([batch * time, freqs, channels]);
        let x = x.clone() + self.attn_f.forward(x);
        let x = x.clone() + self.ff_f.forward(x);
        let x = x
            .reshape([batch, time, freqs, channels])
            .permute([0, 2, 1, 3])
            .reshape([batch * freqs, time, channels]);
        let x = x.clone() + self.attn_t.forward(x);
        let x = x.clone() + self.ff_t.forward(x);
        x.reshape([batch, freqs, time, channels])
            .permute([0, 3, 1, 2])
    }
}

/// Across frequency and time, then a convolution halving the frequencies and doubling the
/// channels.
#[derive(Module, Debug)]
pub struct FrontendBlock<B: Backend> {
    partial: PartialFt<B>,
    conv2d: Conv2d<B>,
    norm: BatchNorm<B>,
}

impl<B: Backend> FrontendBlock<B> {
    fn new(dim: usize, head: usize, device: &B::Device) -> Self {
        Self {
            partial: PartialFt::new(dim, head, device),
            conv2d: Conv2dConfig::new([dim, dim * 2], [2, 3])
                .with_stride([2, 1])
                .with_padding(PaddingConfig2d::Explicit(0, 1, 0, 1))
                .with_bias(false)
                .init(device),
            norm: BatchNormConfig::new(dim * 2).init(device),
        }
    }

    fn forward(&self, x: Tensor<B, 4>) -> Tensor<B, 4> {
        gelu(
            self.norm
                .forward(self.conv2d.forward(self.partial.forward(x))),
        )
    }
}

/// Normalise each mel band, then a convolution over four bands at a time.
#[derive(Module, Debug)]
pub struct Stem<B: Backend> {
    bn1d: BatchNorm<B>,
    conv2d: Conv2d<B>,
    bn2d: BatchNorm<B>,
}

impl<B: Backend> Stem<B> {
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

#[derive(Module, Debug)]
pub struct Frontend<B: Backend> {
    stem: Stem<B>,
    blocks: Vec<FrontendBlock<B>>,
    linear: Linear<B>,
}

impl<B: Backend> Frontend<B> {
    fn new(dims: Dims, device: &B::Device) -> Self {
        let blocks = (0..3)
            .map(|block| FrontendBlock::new(dims.stem << block, dims.head, device))
            .collect();
        // Three blocks double the stem's channels and halve its quarter of the bands each.
        let channels = dims.stem << 3;
        let freqs = (BINS / 4) >> 3;
        Self {
            stem: Stem::new(dims.stem, device),
            blocks,
            linear: LinearConfig::new(channels * freqs, dims.transformer).init(device),
        }
    }

    fn forward(&self, x: Tensor<B, 3>) -> Tensor<B, 3> {
        let mut x = self.stem.forward(x);
        for block in &self.blocks {
            x = block.forward(x);
        }
        let [batch, channels, freqs, time] = x.dims();
        let x = x
            .permute([0, 3, 1, 2])
            .reshape([batch, time, channels * freqs]);
        self.linear.forward(x)
    }
}

#[derive(Module, Debug)]
pub struct Layer<B: Backend> {
    attn: Attention<B>,
    ff: FeedForward<B>,
}

#[derive(Module, Debug)]
pub struct Transformer<B: Backend> {
    layers: Vec<Layer<B>>,
    norm: RmsNorm<B>,
}

impl<B: Backend> Transformer<B> {
    fn new(dims: Dims, device: &B::Device) -> Self {
        let layers = (0..dims.layers)
            .map(|_| Layer {
                attn: Attention::new(dims.transformer, dims.head, device),
                ff: FeedForward::new(dims.transformer, dims.ff_mult, device),
            })
            .collect();
        Self {
            layers,
            norm: RmsNorm::new(dims.transformer, device),
        }
    }

    fn forward(&self, x: Tensor<B, 3>) -> Tensor<B, 3> {
        let mut x = x;
        for layer in &self.layers {
            x = layer.attn.forward(x.clone()) + x;
            x = layer.ff.forward(x.clone()) + x;
        }
        self.norm.forward(x)
    }
}

/// A beat and a downbeat logit a frame, the downbeat's added to the beat's.
#[derive(Module, Debug)]
pub struct SumHead<B: Backend> {
    beat_downbeat_lin: Linear<B>,
}

#[derive(Module, Debug)]
pub struct BeatThis<B: Backend> {
    frontend: Frontend<B>,
    transformer_blocks: Transformer<B>,
    task_heads: SumHead<B>,
}

impl<B: Backend> BeatThis<B> {
    pub fn new(dims: Dims, device: &B::Device) -> Self {
        Self {
            frontend: Frontend::new(dims, device),
            transformer_blocks: Transformer::new(dims, device),
            task_heads: SumHead {
                beat_downbeat_lin: LinearConfig::new(dims.transformer, 2).init(device),
            },
        }
    }

    /// The beat and downbeat logits of each frame of `spect`, `[1, time, bins]`.
    pub fn forward(&self, spect: Tensor<B, 3>) -> (Tensor<B, 1>, Tensor<B, 1>) {
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
