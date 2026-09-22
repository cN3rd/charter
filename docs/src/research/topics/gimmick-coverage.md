---
type: Open Decision
title: Gimmick coverage
description: How much of the .ssc timing vocabulary the editor must support.
tags: [topic, scope, timing, open]
status: draft
---

# Gimmick coverage

## Observed

`.ssc` models `#STOPS`, `#DELAYS`, `#TIMESIGNATURES`, `#TICKCOUNTS`, `#COMBOS`, `#SPEEDS`,
`#SCROLLS`, `#FAKES`, `#LABELS`, attacks, display BPM and warps.

ArrowVortex exposes all of it: its SM5 dialogs cover delays, warps, time signatures, tick counts,
combo multipliers, speeds, scrolls, labels and attacks.

`.osu` mania represents none of the mines, rolls, lifts, fakes or warps.

## Open decision

- Stops only
- Stops, delays and time signatures
- The full `.ssc` vocabulary
- Full vocabulary in the data model, partial in the editing UI

The distinction between the last two matters: a format can be preserved on round trip without
being editable, which is cheaper and avoids silent loss.

## Evidence

Explicitly open. Earlier research separately leaned toward deferring full SM5 gimmick editing out
of an MVP as risk management. That is **asserted** scheduling, not a capability finding.

Non-bijective timing is the real cost driver here. Warps and stops break timestamp-to-row
inversion for everything downstream, so supporting them is an architectural commitment rather than
a feature. See [audio timing physics](../constraints/audio-timing-physics.md).

## Draft position

Provisionally addressed: everything is preserved, almost nothing is interpreted. See [timing](../../format.md#timing).