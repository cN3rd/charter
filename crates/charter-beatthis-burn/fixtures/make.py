"""Writes the parity fixtures from the reference implementation (github.com/CPJKU/beat_this).

Run once by hand, never by the build or tests:

    uv venv --python 3.12 && uv pip install torch torchaudio numpy einops \
        rotary-embedding-torch safetensors --extra-index-url https://download.pytorch.org/whl/cpu
    PYTHONPATH=path/to/beat_this python make.py

It writes, little-endian:
- tiny.safetensors: a seeded Beat This! small enough to embed, keyed as the real checkpoint's
  state_dict is ("model." prefix, same module paths);
- tiny_spect.f32: its input, 40 frames of 128 mel bins;
- tiny_beat.f32, tiny_downbeat.f32: its logits for that input;
- mel.f32: the reference log-mel spectrogram of the signal `signal()` below, which the Rust test
  builds the same way.
"""

import math

import numpy as np
import torch
from safetensors.torch import save_file

from beat_this.model.beat_tracker import BeatThis
from beat_this.preprocessing import LogMelSpect

torch.manual_seed(0)
model = BeatThis(spect_dim=128, transformer_dim=16, ff_mult=4, n_layers=1, head_dim=8, stem_dim=8)
for module in model.modules():
    if isinstance(module, (torch.nn.BatchNorm1d, torch.nn.BatchNorm2d)):
        module.running_mean.uniform_(-0.5, 0.5)
        module.running_var.uniform_(0.5, 2.0)
        module.weight.data.uniform_(0.5, 1.5)
        module.bias.data.uniform_(-0.2, 0.2)
model.eval()
state = {
    f"model.{key}": value.contiguous()
    for key, value in model.state_dict().items()
    # The rotary frequencies are one shared tensor fixed by a formula, which the Rust model
    # computes instead of loading.
    if not key.endswith("num_batches_tracked") and "rotary_embed" not in key
}
save_file(state, "tiny.safetensors")

spect = torch.rand(1, 40, 128) * 3.0
with torch.no_grad():
    out = model(spect)
spect.numpy().astype("<f4").tofile("tiny_spect.f32")
out["beat"][0].numpy().astype("<f4").tofile("tiny_beat.f32")
out["downbeat"][0].numpy().astype("<f4").tofile("tiny_downbeat.f32")


def signal():
    """One second at 22 050 Hz: a 440 Hz tone at half scale, and a click every quarter second."""
    rate = 22050
    samples = [0.5 * math.sin(2 * math.pi * 440 * n / rate) for n in range(rate)]
    for start in range(0, rate, rate // 4):
        for n in range(start, min(start + 32, rate)):
            samples[n] += 0.4 * (1 - (n - start) / 32)
    return np.array(samples, dtype=np.float32)


mel = LogMelSpect()(torch.tensor(signal()))
mel.numpy().astype("<f4").tofile("mel.f32")
print("frames", mel.shape, "logits", out["beat"].shape)
