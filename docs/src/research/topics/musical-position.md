---
type: Open Decision
title: Musical position representation
description: Whether musical position is an integer row or a floating-point measure coordinate.
tags: [topic, data-model, timing, open]
status: draft
---

# Musical position representation

## Observed

Tempora ships a float `MeasurePosition` where the integer part is the measure and the fraction the
intra-measure position: `0.75` in 4/4 is the fourth quarter-note of measure 0. It converts to audio
time as `(time - Offset) * MeasuresPerSecond + MeasurePosition`.

It has shipped a beta with this model and enforces monotonicity in both coordinates, rejecting
edits that would cross a neighbour.

StepMania, ArrowVortex and Moonscraper all use integers instead.

## Open decision

- Integer rows with a canonical segment map
- Float measure positions, as Tempora does
- Integer rows with float sync anchors at the audio boundary only

## Evidence

Earlier research leaned to integer rows with a `BTreeMap<row, Segment>`, citing assumed float drift
and round-trip concerns plus `.ssc` requirements. **No observed Tempora failure is recorded.** The
concern is reasonable but untested, and the counter-example is a shipped tool.

Grade: **inferred**, against an **observed** working counter-example.

## Draft position

Provisionally resolved: integer ticks. See [time base](../../format.md#time-base).
