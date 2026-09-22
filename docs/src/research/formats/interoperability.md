---
type: Reference
title: Format interoperability
description: The recorded structural incompatibilities between chart formats, independent of implementation effort.
tags: [format, interop, constraints]
status: draft
---

# Format interoperability

These are structural differences, not implementation gaps. No amount of effort makes them go away.

- `.osu` mania represents notes, holds and its control-point model, but not StepMania mines,
  rolls, lifts, fakes or warps. Conversion from `.ssc` is lossy in at least those dimensions.
- `.ssc` carries delays, warps, speeds, scrolls, fakes, labels, attacks and per-chart timing that
  simpler targets cannot hold.
- Quaver's `.qua`-first model and its converters do not establish full round-trip fidelity.
- StepMania row/note types, osu!mania millisecond timing, Quaver scroll velocity and timing groups,
  guitar events, and Malody beat/effect arrays do not form one equivalent data model.
- Tempora's mapping requires positive `MeasuresPerSecond`, so its timing model cannot represent
  warps or negative-BPM, non-bijective timing.
- Timing maps may contain stops, warps, anchors and denominator changes, so timestamp-to-row
  mapping cannot rely on simple inversion.

## Demonstrated lossiness in shipped tools

- ArrowVortex's SM loader ignores keysounds while its saver writes `#KEYSOUNDS`, so a same-format
  round trip through it loses data silently.
- Tempora's osu export removes no-op points, adds downbeat points, resets unsupported signatures
  and adjusts BPMs — all without a report to the user.

Both are concrete instances of the failure mode discussed in
[round-trip fidelity](../topics/round-trip-fidelity.md) and
[export loss reporting](../topics/export-loss-reporting.md).

## Constants across formats

| Format | Time base |
|---|---|
| `.sm` / `.ssc` | rows, `ROWS_PER_BEAT = 48` |
| `.chart` | ticks, conventionally 192/beat |
| `.osu` | milliseconds |
| `.qua` | milliseconds with scroll-velocity layers |
| `.mc` | beats, with optional `endbeat` |
| `.tmpr` | seconds paired with float measure positions |

The mismatch between 48 rows/beat and 192 ticks/beat is the first concrete decision this forces:
see [row resolution](../topics/row-resolution.md).
