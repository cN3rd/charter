---
type: Open Decision
title: Note entry
description: The keyboard mechanics of placing taps, holds and variant note types.
tags: [topic, ux, input, open]
status: draft
---

# Note entry

## Observed

**ArrowVortex.** Digits place notes immediately. Holding a column key while moving the cursor grows
a ghost note committed on release. Shift plus a digit places a mine. With auto-jump enabled, release
advances the cursor to the next snap. During playback, edits commit immediately rather than waiting
for release. Variant types (fakes, lifts, rolls) are reached by **converting** existing notes, not
by dedicated entry keys.

**StepMania.** Column digits, Shift for rolls, `n`/`m` to cycle taps.

**osu!mania.** Separate Select, Note and Hold tools; holds are placed by dragging.

**Quaver.** `PlaceNoteAtLane1..10` with long-note-on-hold behaviour.

## Open decision

- Hold-to-extend, as ArrowVortex and Quaver do
- Drag-to-place holds, as osu! does
- Tool modes versus modeless entry
- Whether fakes, lifts and rolls get direct entry keys or stay conversion-first

Auto-jump-to-next-snap is worth isolating: it converts note entry from "place, move, place" into a
single repeated keystroke, which is a throughput difference rather than a convenience.

## Evidence

All **observed**. Earlier research called auto-jump "a charting-speed feature" to build early, which
is an **inferred** priority, and left the direct-entry-versus-conversion question explicitly open.
