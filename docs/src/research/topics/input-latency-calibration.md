---
type: Open Decision
title: Input latency calibration
description: The user-adjustable offset compensating for the gap between hearing audio and a keypress registering.
tags: [topic, ux, audio, timing, open]
status: draft
---

# Input latency calibration

## Observed

Quaver exposes an editor input offset with a range of **-200 to +200 ms**, applied by offsetting
`Track.Time` before snapping.

The gap being compensated is real and physical: output buffering, display latency, keyboard scan
rate and the user's own anticipation all contribute, and they vary per machine and per user.

## Open decision

- Sign convention. "Positive means I tap late" and "positive means shift notes later" are opposite,
  and the wrong choice makes every support conversation confusing.
- Range. Quaver's +/-200 ms is the only cited precedent.
- Whether it is one global value, per-device, or per-project.
- Whether calibration is manual entry or a guided tap-along routine.

## Evidence

The range is **observed** in one tool. Everything else is **unresolved** — earlier research
explicitly listed the sign convention and exact range as open.

This blocks [live recording](live-recording.md): uncalibrated capture is systematically offset, and
no amount of quantization fixes a constant bias.
