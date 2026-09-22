---
type: Open Decision
title: Sync anchors
description: Whether audio-to-music anchors are persistent project data or ephemeral handles over a timing map.
tags: [topic, data-model, sync, timing, open]
status: draft
---

# Sync anchors

## Observed

Tempora's entire data model is anchors: `TimingPoint` pairs an audio `Offset` in seconds with a
musical `MeasurePosition`, and `.tmpr` persists exactly that. Segment tempo is **derived** from
neighbouring anchors rather than stored, `MPS = dMeasure / dOffset`.

StepMania-family formats invert this: they store BPM segments and stops, and the anchor
relationship is implicit in `#OFFSET`.

## Open decision

- Anchors persist as native project data, with tempo derived
- Anchors are ephemeral editing handles compiled into stored timing channels
- Both, with anchors as an authoring layer above a canonical timing map

## Evidence

Tempora demonstrates the derived-tempo model works for sync-only work. Whether it survives contact
with `.ssc` gimmicks is untested.

Grade: **observed** for the model; **unresolved** for charter.

## Unresolved

- How anchors interact with `.ssc` stops, delays and warps.
- Whether an anchor may target a post-warp row at all, given warps skip time.
- What manual BPM editing does in anchor mode: move the next anchor's offset, move its row, or
  split the segment.

## Draft position

**Not in v0.** Excluded as redundant with `bpm` events and unresolved; if it returns it should be editor project data, not chart data. See [known gaps](../../format.md#known-gaps).