---
type: Decision
title: Sync anchors
description: Whether audio-to-music anchors are persistent project data or ephemeral handles over a timing map.
tags: [topic, data-model, sync, timing]
status: draft
---

# Sync anchors

## Observed

Tempora's entire data model is anchors: `TimingPoint` pairs an audio `Offset` in seconds with a
musical `MeasurePosition`, and `.tmpr` persists exactly that. Segment tempo is **derived** from
neighbouring anchors rather than stored, `MPS = dMeasure / dOffset`.

StepMania-family formats invert this: they store BPM segments and stops, and the anchor
relationship is implicit in `#OFFSET`.

## Decision

Anchors persist as native timing data. Segment tempo is derived between neighbouring anchors, and
the tail tempo is stored after the last anchor.

## Evidence

Tempora demonstrates the derived-tempo model works for sync-only work. Whether it survives contact
with `.ssc` gimmicks is untested.

Grade: **decided** for charter format v1.

## Constraints and editing

- Stops and delays inside a segment keep their StepMania timing semantics and shorten the time
  available to that segment's beats.
- Anchors may not fall on warped ticks.
- Setting a segment's BPM keeps anchor seconds fixed and moves downstream anchor ticks to fit
  the new tempo. By default its notes keep their audio times too; the editor can instead keep
  them on their beats within the edited segment. Setting the tail changes `tempo`.

## Draft position

**Stored in v1.** Anchors are the chart format's tempo model because they keep authored sync times
exact and avoid cumulative BPM rounding drift.