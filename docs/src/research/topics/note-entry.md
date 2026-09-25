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

## Draft position

Digits 1 to 9 and 0 place a tap on lanes 1 to 10 at the cursor, or remove the note that starts
there, and Shift with a digit places a mine, as in ArrowVortex. Holds are ArrowVortex's and
Quaver's hold-to-extend: a tap whose key is still down stretches into a hold to wherever the
cursor moves. Auto-jump follows ArrowVortex's release rule: the cursor moves one snap once every
lane key is up after a note was placed, so keys held together place a chord on one tick. A setting
turns it off. Removing a note leaves the cursor.

The mouse places with Ctrl held, since a plain click selects: a click toggles a tap on the nearest
snap line, Ctrl+Shift a mine, and a drag draws a hold, osu!'s gesture without its tool modes.
Conversions have no entry yet.
