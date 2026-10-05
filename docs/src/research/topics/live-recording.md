---
type: Open Decision
title: Live recording
description: Real-time note capture by tapping along to playback, including quantization and hold thresholds.
tags: [topic, ux, input, recording, open]
status: draft
---

# Live recording

## Observed

Three shipped tools capture input during playback:

- StepMania separates Edit, Play, Record and RecordPaused contexts, with record-from-cursor and
  record-selection bindings.
- Quaver treats it as first class: `EditorLiveMapping`, `EditorLiveMapOffset`,
  `EditorLiveMapSnap`, long-note-on-hold. While playing, it offsets `Track.Time` and snaps to the
  nearest tick before placing. Input offset range: -200 to +200 ms.
- Editor on Fire has real-time Guitar Tap/Strum recording.

No surveyed editor is shown to implement the full combination proposed below.

## Open decision

### Priority

Launch-critical, late-MVP, or post-MVP. Explicitly deferred to roadmap.

### Quantization default

Quantize on place (Quaver's behaviour), capture raw, or capture raw then
quantize as a reviewable second step. Raw capture is the only option that preserves the performance
for later requantization at a different snap.

### Hold threshold

How long a key must be held before a tap becomes a long note. ArrowVortex's
paused-mode equivalent grows a ghost note while the key is down, with no time threshold at all.

### Session shape

Lead-in of 0, 1 or 2 measures, or a seconds-based count-in; armed lanes;
overdub versus replace; loop passes; layered takes.

### Undo granularity

Whether a pass is one transaction. See [undo model](undo-model.md).

## Evidence

The individual capabilities are **observed** across three tools. The specific proposed combination
(countdown, raw takes, take layering, armed lanes, hold thresholds, lane replacement, one-command
take undo) is **asserted**: earlier research proposed it as charter's primary interpretation of
"tap to the rhythm", without a tool implementing the whole.

Depends on [input latency calibration](input-latency-calibration.md); without a calibrated offset,
captured timestamps are systematically early or late.

Also unresolved: behaviour around stops and warps, where the timing map is not invertible.
