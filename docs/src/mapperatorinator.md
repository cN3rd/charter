---
type: Design Note
title: Mapperatorinator port
description: What charter-mapperatorinator-burn is, how it times a song, and how it differs from Mapperatorinator's own model and code.
tags: [timing, machine-learning, osu, burn, draft]
status: draft
---

# Mapperatorinator port

`charter-mapperatorinator-burn` finds a song's osu! timing points from its audio with the timing
part of [Mapperatorinator](https://github.com/OliBomby/Mapperatorinator), OliBomby's osu! beatmap
generator, ported from Python to Rust. It runs Mapperatorinator's V32 model in
[burn](https://burn.dev), on the graphics card through wgpu or on the CPU through flex, and
reproduces its super timing generator around it. Timing › Find timing with Mapperatorinator… calls
it, and [Automatic timing](autotiming.md) says what charter does with the timing points it returns.

The crate's API is a `Timer` loaded from the model's GGUF file, `Timer::run` over mono samples at
16 kHz divided by their peak, and `graphics_card` to ask whether wgpu has a card to run on. No burn
type crosses it, so `charter-app` runs it without depending on burn.

## Timing a song

The model hears 16.4 seconds of audio at a time, a window of 262,016 samples. Each window becomes a
power spectrogram on the CPU (a 1,024-sample Hann window every 128 samples, the signal reflected at
both ends) and, on the device, a 128-band log-mel spectrogram through the filterbank stored with
the weights. A VarWhisper encoder reads it: two convolutions, the second halving the frames, then
12 transformer layers of width 768 with 12 heads. A 12-layer decoder writes the window's events
after a prompt holding upstream's default conditions (osu! standard, hitsounded, circle size 4,
slider multiplier 1.4), unknown difficulty, mapper, year and style, the song's length and the
window's place in it. Both stacks use RMS norms, exact GELU and rotary positions with theta 10,000
in self-attention.

Decoding is greedy, 16 windows at a time with a key and value cache. As upstream's
`MonotonicTimeShiftLogitsProcessor` rules, a time shift may not come before the previous one, and a
window is done at the timing context's end token.

Super timing makes 20 passes over the song. Each pass shifts the audio by its own offset, up to half
a window either way, so the windows split the song in different places. Every pass's beats,
measures and timing points go into histograms at 1 ms resolution, which are smoothed and read for
peaks. The local tempo at each peak comes from the spacing of nearby beats, and walking outwards
from the strongest peaks at that tempo, snapping to peaks on the way, builds one beat grid. Each
beat is labelled a measure or a timing point, and the grid becomes osu! timing points: one per
change of tempo or meter, each BPM rounded as far as the beats allow within 20 ms.

## Changes from Mapperatorinator

### Timing only

The port keeps the audio front end, the V32 model and the super timing generator. Mapperatorinator's
beatmap generation goes: object positions, hitsounds, the diffusion model, reference beatmaps, the
web UI and training. The port runs V32 alone: its sizes are fixed in the code, and V31 and older
models use other backbones. Timing comes from the audio alone, where upstream can also start from
an existing beatmap's timing.

### Weights

The weights come from `OliBomby/Mapperatorinator-v32` at revision
`74f22583400d259bf424819e11027c17933efe54`, checked by sha256, pruned to the tensors timing reads:

- the Whisper decoder's own token embedding, which Mapperatorinator bypasses with
  `decoder_embedder`, is dropped;
- the training loss weights are dropped;
- the 7,605 `decoder_embedder` rows for mapper styles are dropped. Timing prompts only carry the
  unknown mapper, so the tokenizer's mapper range narrows to that one value, and every other token
  keeps its id.

The tokenizer keeps only the fields timing reads, and the pruned vocabulary works with this port
alone. Each weight is rounded to its nearest float16, and the weights, the tokenizer and their
provenance are one GGUF file of 415,618,240 bytes, where upstream's checkpoint is about 870 MB.
Weights are widened to float32 on load, so storage is rounded and the arithmetic is not.

Smaller files were tried with ggml's quantized formats and GPTQ rounding. Teacher-forced against the
float32 model's own decoding, Q8_0 weights moved 4 of 12,708 tokens by more than one 10 ms step,
and super timing turns differences that small into different results: it places beats at 1 ms
histogram peaks and splits sections where the evidence tips over a threshold. End to end, on seven
pieces with fixed pass offsets:

| Weights | Size | Pieces with every timing point identical |
|---|---|---|
| float16, rounded to nearest | 416 MB | 7 of 7, and 2 more pieces |
| Q8_0, the output projection float16 | 224 MB | 4 of 7 |
| Q4_K, 6 groups Q8_0 | 125 MB | 0 of 2 tried |

Float16 needs no calibration, since rounding a weight depends on nothing but the weight. The GGUF
reader still decodes the quantized formats from those experiments (BF16, Q8_0, Q6_K, Q5_K, Q4_K,
Q4_1, Q4_0, IQ4_NL and MXFP4).

### Running the model

- Upstream's batched super timing decodes each window on through the whole map after its timing.
  The port stops at the timing context's end token, which leaves the timing tokens as they were,
  since decoding is greedy and causal, and finishes much sooner.
- Greedy decoding cannot be changed by a temperature or top-k, so neither is passed.
- The pass offsets come from SplitMix64 with seed 0 rather than NumPy's generator. They spread the
  same way and are the same every run, but they are not the offsets upstream draws.
- Charter decodes the audio with symphonia, mixes it to mono, resamples it to 16 kHz with rubato and
  divides it by its peak, where upstream decodes with pydub and ffmpeg. The samples differ slightly,
  and super timing can turn that into different timing points.
- The model computes in float32 throughout.
- Attention always runs as burn's plain matmuls and softmax. For the encoder burn 0.22 would pick a
  flash kernel instead, which on wgpu takes three times as long.
- The encoder's two convolutions run as one matmul each, over every output step's three input
  steps side by side. burn's direct convolution, the only one it has on wgpu, was the slowest part
  of the encoder.

## Matching upstream

Each module ports upstream's code and keeps its arithmetic, down to the order of floating-point
sums where the result depends on it. The smoothing reproduces scipy's bit for bit, since which
sample of a flat-topped peak wins is decided by the last bits, and the crate's tests compare the
smoothing and peak finding with scipy's output and the GGUF decoders with gguf-py's.

When the port was written, seven pieces of music from 1 to 6 minutes long (hardcore, J-core, rock,
house and EDM) were timed by both with the same 20 offsets. Every window decoded to the same tokens
as upstream's, 2,086 of 2,086, and every fused beat grid and timing point was identical, both in
float32 on a laptop RTX 3070 Ti with upstream's TF32 off. Upstream paths below are under
`osuT5/osuT5/`.

| Module | Ported from |
|---|---|
| `audio.rs` | `inference/preprocessor.py`, `Preprocessor.segment` |
| `mel.rs`, `model.rs` | `model/spectrogram.py`, `model/modeling_mapperatorinator.py`, `model/custom_transformers/modeling_varwhisper.py` |
| `tokenizer.rs` | `tokenizer.py`, `event.py` |
| `generate.rs` | `inference/processor.py`, `inference/logit_processors.py` |
| `super_timing.rs` | `inference/super_timing_generator.py` |
| `signal.rs` | scipy's `gaussian_filter1d` and `find_peaks` |
| `timing.rs` | `inference/postprocessor.py` `generate_timing`, `inference/timing_points_change.py` |
| `gguf.rs` | ggml's GGUF format and the `dequantize_row_*` functions of `ggml-quants.c` |

## Weights and licence

The GGUF file is published at
[cnf3rd/timingator-v32-f16-gguf](https://huggingface.co/cnf3rd/timingator-v32-f16-gguf); the
crate's README has its URL and checksum. Charter downloads it on first use, after asking, into its
data folder. Mapperatorinator is MIT-licensed, and so is the port: `LICENSE` is upstream's licence
and `NOTICE` lists what comes from where, scipy's peak finding and ggml's block formats included.
Both ship in every package as `licenses/mapperatorinator-*`.
