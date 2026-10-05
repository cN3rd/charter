---
type: Open Decision
title: Selection and transforms
description: The selection vocabulary and pattern operations that define charting throughput.
tags: [topic, ux, editing, open]
status: draft
---

# Selection and transforms

## Observed

**ArrowVortex selection** is unusually rich: by quantization, by note type, by density, before or
after cursor, cursor-to-cursor region, and box marquee. Shift adds to a marquee, Alt subtracts,
right-click clears.

**Clipboard.** `Shift+Ctrl+V` pastes non-destructively. Two copy modes exist: row-based, and
time-based, which encodes notes against timing data and decodes at cursor time. With nothing
selected, copy captures the cursor timestamp and paste jumps to it.

**Transforms.** Horizontal mirror, vertical mirror, full mirror, expand `2:1` `3:2` `4:3`, compress
`1:2` `2:3` `3:4`, `Requantize`. Mirrors use style-specific lane tables.

**Conversions.** Notes to and from mines, fakes and lifts; holds to and from rolls; holds to steps
or mines; switch player; switch sides.

**Absent.** No turn, shuffle, rotate or generic transpose appears in the cited menu source, despite
those being commonly associated with the tool.

## Open decision

- Which selection predicates are core
- Whether time-based copy is a mode or the default
- Whether transforms are fixed commands or a composable operation set
- Whether to implement turn/shuffle/rotate at all

## Evidence

All **observed** from source. Earlier research ranked non-destructive paste "very high-value" and
horizontal mirror "the first parity transform", both **inferred**. It flagged `Requantize` as
high-risk because it touches timing, and noted that rotate/shuffle should not be claimed as
ArrowVortex parity without community validation, since the source does not show them.

## Draft position

A click selects the note under the pointer, and a drag selects the notes whose heads a box covers;
Shift adds to the selection and Alt takes from it, ArrowVortex's modifiers. Ctrl+A selects the
notes on shown, unlocked layers and Escape selects nothing. The selection supports deletion,
cut/copy/paste, duplication, movement in ticks and lanes, mirroring, quantization, resizing and
the plugin's note conversions, with each edit checked and undoable.

Selection is addressed by starting tick and first lane, not by a unique object ID. Ruleset-defined
gestures may coincide with taps at that address, so the key is not universally unique and does
not distinguish those coincident objects. See [well-formedness](../../format.md#well-formedness).
