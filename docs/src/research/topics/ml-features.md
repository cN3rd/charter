---
type: Open Decision
title: ML features
description: Stem separation and other model-driven features, and whether they belong in a chart editor at all.
tags: [topic, stack, ml, open]
status: draft
---

# ML features

## Observed

`ort` is a Rust interface to ONNX Runtime, MIT/Apache-2.0, and the successor to the inactive
`onnxruntime-rs`.

Stem separation has an existing non-ML precedent in the surveyed set: **Editor on Fire supports
multi-stem playback with mute and solo**, using stems the user already has. The ML version
generates stems that do not exist, which is a different proposition.

## Open decision

- Whether stem separation is a goal at all
- Whether difficulty estimation uses a model or heuristics. ArrowVortex ships automatic difficulty
  estimation and an estimate-rating button in Chart Properties, so the feature has precedent even
  if the method does not
- Whether any ML feature ships in-process or as an optional download

## Evidence

Earlier research judged stem separation "research-grade for MVP" and suitable post-1.0, reasoning
from model size, weights licensing, memory, inference cost and GPU-backend uncertainty. Grade
**inferred**.

The spike was never run: test `ort` with a small ONNX separation model for 30-second offline
inference, timing and memory.

Weights licensing deserves its own attention if this is pursued; a permissively licensed runtime
says nothing about the terms on the model.
