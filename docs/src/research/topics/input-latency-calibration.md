---
type: Decision
title: Input latency calibration
description: The user-adjustable offset compensating for the gap between hearing audio and a keypress registering.
tags: [topic, ux, audio, timing]
status: draft
---

# Input latency calibration

## Observed

Quaver exposes an editor input offset with a range of **-200 to +200 ms**, applied by offsetting
`Track.Time` before snapping.

The gap being compensated is real and physical: output buffering, display latency, keyboard scan
rate and the user's own anticipation all contribute, and they vary per machine and per user.

## Decided

Two global values, kept in the settings. The audio latency, set by ear against a flash, is how
much later than it plays a sound is heard; the cursor takes it off the playhead. The input latency,
measured by a guided tap-along, is how much later than that a tap comes; notes placed while playing
take it off. Positive means late in both. The click track follows the song's first tempo and the
metronome's sound. The audio latency ranges over +/-500 ms, wider than Quaver's.

## Was open

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
