---
type: Guide
title: Charting
description: Moving the cursor, placing, selecting and editing notes, and editing the song's details, charts and markers.
tags: [guide, editing, notes]
status: draft
---

# Charting

## Moving around

The cursor is the bright line across the notefield. Notes go on it, and playback starts from it.

| To move | Use |
|---|---|
| by the snap | Up and Down, or the mouse wheel |
| by a measure | Page Up and Page Down, or Shift with the wheel |
| to the start, or where the last note ends | Home, End |
| to the previous or next section | Alt+Up, Alt+Down |
| anywhere | a click or drag on the minimap, or a drag of the cursor's handle on the ruler |

Left and Right make the snap coarser or finer: 4th, 8th, 12th, 16th, 24th, 32nd, 48th, 64th, 96th
or 192nd notes, shown on the toolbar. Ctrl with the wheel, Ctrl+Plus and Ctrl+Minus zoom.
Dragging with the middle button pans by moving the cursor; while playing, it also seeks the audio.

## Placing notes

The number keys 1 to 9 and 0 are the lanes from left to right; the lane header shows each key.

- A lane key places a tap on the cursor, or removes the note there.
- Keys pressed together place a chord. Once they are all up, the cursor moves on one snap, ready
  for the next note; the Editing page of the settings turns that off.
- Holding a lane key while moving the cursor stretches the tap into a hold.
- Shift with a lane key places a mine, in games that have them.
- With the mouse, Ctrl+click places or removes a tap, Ctrl+Shift+click a mine, and Ctrl+drag along
  a lane draws a hold.
- `]` lengthens the selected notes, or those at the cursor, by the snap, making a tap a hold, and
  `[` shortens them back.

Lane keys also work while the song plays: each note lands on the snap line nearest what you heard,
allowing for the input latency [calibrated](listening.md#latency), and a key held past 200 ms grows
into a hold until it comes up.

Every edit is checked against the chart format and the game's rules. One the game cannot play,
such as a Tap Tap Revenge shake out of the lane of its direction, is refused: a message says why,
and the note in the way flashes.

## Selecting and editing

- A click selects a note, and a drag across empty space selects the notes it boxes from shown,
  unlocked layers. Shift adds to the selection and Alt takes from it. Ctrl+A selects every
  editable note, Escape none.
- Dragging a note moves it, or the selection it is in, by whole snaps and across lanes, with an
  outline where the notes will land. Shift keeps the notes' time and moves them across lanes only.
- Delete or Backspace removes the selection.
- Ctrl+X, Ctrl+C and Ctrl+V cut, copy and paste at the cursor. Copies go on the system clipboard
  too, so another charter window can paste them. Ctrl+D duplicates the selection just after
  itself.
- Ctrl+Left and Ctrl+Right move the selection a lane, Ctrl+Up and Ctrl+Down by the snap, and Ctrl+M
  mirrors it across the lanes. G snaps it to the nearest snap lines, for notes placed by ear.

A right click opens a menu of these edits without moving the cursor. It also holds:

- **Change to**, the game's own changes for the selected notes. Tap Tap Revenge offers Arrow,
  which turns a note into the shake of its lane, Slice, Big tap across every lane, and Tap.
- **Insert pattern here**, 1-1-1-1, 1-2-3-1-2-3 or 1-2-1-2 from the lane clicked, its notes a beat
  down to a 64th apart.

Pastes and patterns go on the line clicked, on the active layer. They skip occupied starting
positions on that layer; a collision with a note on another layer refuses the whole operation,
even if that layer is hidden. A hidden, ghosted or locked active layer also refuses placement.

Ctrl+Z undoes and Ctrl+Shift+Z or Ctrl+Y redoes, one edit at a time. Each returns to the chart and
the place it was made, so the change is in view.

## Layers

Each chart's notes can be split into named, coloured layers, such as one per instrument or one per
idea being tried. A layer can be shown, faded, hidden, soloed or locked, and merged into the one
below. The Layers section of the song panel lists them, and Alt+1 to 9 makes one active, so new
notes go there; Alt+Shift+1 to 9 moves the selected notes. View > Colour notes by layer tints
whole notes in their layer's colour. [Layers](../layers.md) describes them in full.
In a read-only song, the list still lets you choose a layer or select its notes, but adding,
renaming, recolouring, reordering, showing, hiding and locking layers are disabled. Visibility
is saved in the song, so changing it is an edit.

## The song panel

The panel on the left holds everything about the song that is not a note. Each change in it is one
undo step. A value typed in a field takes effect on Enter or when the field is left; Escape puts it
back.

- **Song**: the title and artist; the audio, with a button to choose another file; the preview, the
  time and length a song list plays; and the artwork, a PNG or JPEG beside the song. The artwork's
  button picks a picture, or an audio file whose tags hold a cover, which is saved beside the song
  as `cover.jpg` or `cover.png`. A song without artwork takes the cover of the audio chosen for it.
  Settings the game keeps as text, such as Tap Tap Revenge's theme, follow.
- **Charts**: the song's charts. Switching chart keeps the cursor where it is, to compare two
  difficulties at one spot. The open chart's own settings, such as a StepMania meter, follow.
- Layers, described above.
- **Timing**: the offset, the tempo and every timing point; see [Timing](timing.md).
- **Markers**: the song's sections and bookmarks. A click jumps there. A name typed and entered
  names the section at the cursor, or starts one; Ctrl+B adds or removes a bookmark at the cursor.
