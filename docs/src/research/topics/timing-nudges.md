---
type: Open Decision
title: Timing nudges
description: Increment sizes for fine offset adjustment during sync.
tags: [topic, ux, sync, timing, open]
status: draft
---

# Timing nudges

## Observed

| Tool | Control | Increment |
|---|---|---|
| Tempora | Ctrl-wheel on nearest point | `0.002 s` |
| Tempora | Shift-Ctrl-wheel | `0.01 s` |
| Tempora | Alt-wheel BPM | integer, Shift +/-5, Ctrl +/-0.1 |
| ArrowVortex | Music offset field | 0.001 s resolution, range -100 to 100 s |
| ArrowVortex | First-beat controls | half beat, one beat |

Two distinct granularities appear in both tools: a fine step for sub-perceptual correction and a
coarse one for audible adjustment.

## Open decision

Whether 2 ms and 10 ms are the right defaults, whether they should be configurable, and whether the
coarse step should be musical (half beat) rather than absolute (10 ms). ArrowVortex offers both
kinds; Tempora only absolute.

## Evidence

Increments are **observed** in shipped tools. Their suitability as defaults is **unresolved**;
earlier research flagged it as an open question rather than adopting them.

Note that 2 ms is below the MP3 start-delay error described in
[audio timing physics](../constraints/audio-timing-physics.md). Nudge precision finer than the
origin error is precision against an uncertain reference.
