---
type: Constraint
title: Audio timing physics
description: Millisecond-scale timing floors that bound sync accuracy regardless of design.
tags: [constraints, audio, timing, sync]
status: draft
---

# Audio timing physics

## MP3 start delay

**Encoder and decoder start delay can separate the decoded sample origin from the playback and
export origin by tens of milliseconds.** Three distinct origins must be reconciled:

1. Waveform sample zero
2. Playback seek zero
3. Export zero

If they disagree, every chart synced against the waveform is wrong by that amount on export. The
effect is observed in Tempora's source comments; the magnitude was never measured for this project.

## Transport boundaries

Real-time recording requires loaded audio, at least one timing segment, timestamp capture at the
input boundary, and an available timing or audio clock. Without all four, capture has nothing to
anchor to.

Input latency, playback synchronisation, keysound and metronome scheduling, and multi-stem drift
are all transport-contract boundaries rather than UI concerns.

## Non-invertible timing

Timing maps containing stops, delays, warps and anchors are not bijective between time and musical
position. A timestamp does not uniquely determine a row. Any code converting in that direction
needs an explicit policy for warped and stopped spans, not an inverse function.
