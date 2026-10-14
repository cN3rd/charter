---
type: Design Note
title: Layers
description: Layers within a chart, from the model to drawing, editing, playback and saving.
tags: [ui, layers, format]
status: stable
---

# Layers

**Built.** The format, editor model, panel, drawing, playback and shortcuts are in the editor.
Dropping a selection onto a layer row remains for a later pass; use *Move to layer* or
Alt+Shift+1–9 instead. The choices at the end are still open.

A chart is built out of parts that follow different things in the music: the kick, the snare, the
melody, a vocal line. Charting them together means judging every note against all the others at
once. Layers let each part be charted on its own, shown alone or over a faint copy of the rest,
and then put together into the one chart the game plays, as layers in an image editor are.

Quaver's editor has layers already: named, coloured, hidden or shown, with objects moved between
them and selected by layer, and notes tinted by their layer's colour while *View layers* is on
([Quaver's editor interface](https://quavergame.com/docs/skins/editorinterface)). Charter takes
that as the base and adds what charting difficulties needs from it.

## What a layer is

- **A layer belongs to one chart.** Each difficulty has its own, since an Easy chart thins the
  parts an Expert one has in full. Timing, markers, metadata and samples stay the song's.
- **Every note is on exactly one layer.** When charter opens a chart without layers, it gives it
  a shown, unlocked blue *Layer 0*, holding every note. This is not an edit or undo step.
- **The chart is all its layers together.** The format's rules, the ruleset plugin's checks,
  saving and export see one chart, the layers merged. So layers never let a chart hold what it
  could not hold without them: two notes of shared core kinds cannot cover one lane on one tick,
  even on different layers. Ruleset-defined kinds keep their game's overlap rules, such as a Tap
  Tap Revenge gesture sharing a tap's lane and tick. The editor refuses placement onto a note of
  another layer, naming that layer whether hidden or shown.
- **A layer has a name and a colour.** Names are unique within the chart. Colours come from a
  fixed palette of eight, red, orange, yellow, green, teal, blue, purple and pink, the next unused
  one given to each new layer, and chosen by clicking the swatch. New names use the smallest
  unused *Layer N* for N of at least 1.
- **The list runs top to bottom.** The first layer is the top row; *below* means the next one
  down.

## The layers panel

Layers are the selected chart's, so they sit in the song panel under the chart list, as the
chart's settings do. It folds away with the rest of the panel.

```text
| CHARTS                  |
|   Easy               4L |
| > Hard               4L |
|                         |
| LAYERS           [+][…] |
| [o] [ ] ■ Kick       52 |
| [o] [L] ■ Snare      31 |
|>[o] [ ] ■ Melody    118 |   <- active: new notes go here
| [~] [ ] ■ Vocals     64 |   <- ghosted
| [ ] [ ] ■ Fills       9 |   <- hidden
```

Each row is, left to right:

- **Visibility**, `[o]` shown, `[~]` ghosted, `[ ]` hidden. A click cycles through them. Alt+click
  solos the layer, ghosting other shown layers and leaving hidden or already-ghosted layers as
  they were; Alt+click again puts them back as they were.
- **Lock**, `[L]`. A locked layer is drawn as usual but its notes cannot be selected or changed.
  The unlocked icon stays visible without hovering; screen readers hear whether *Lock [name]* is on.
- **The colour swatch.** A click opens the palette.
- **The name.** A double-click renames it in place, and Enter or leaving the field applies it.
  If the name is refused, the typed text stays in the focused field for correction; Escape cancels it.
- **Its note count.** Ctrl+click on the row, or *Select notes* in its menu, selects them all, as
  Ctrl+click on a layer's thumbnail selects its pixels in Photoshop.

The highlighted row is the **active layer**, where new notes go; a click on a row makes it active.
Drag a row by its handle to reorder it. `[+]` makes a layer above the active one and makes it
active. `[…]`, and right-click on a row, hold the rest: rename, colour, *Select notes*, *Merge
down*, *Copy to chart ›* and delete.
On a read-only song, choosing a row and *Select notes* still work, but controls that change
the layer or its saved visibility are disabled and say why on hover.

## Drawing

- **Shown** layers draw as now. **Ghosted** ones draw at about a third of their brightness and
  cannot be selected or edited: a reference to chart against, like tracing paper. **Hidden** ones
  are not drawn at all.
- Notes keep the ruleset's own look, since colour often carries the game's meaning, such as
  StepMania's beat colours. A layer's colour shows as a thin bar down each note's leading edge.
  *View › Colour notes by layer* tints the whole note instead, as Quaver's *View layers* does.
- A lower layer's notes are drawn first. Shared core kinds cannot occupy the same lane and tick;
  the order also applies where a ruleset permits its own kinds to overlap.
- The status bar shows the active layer's swatch and name. A click there lists the chart's layers
  to switch to.
- The minimap follows the notefield: hidden notes disappear and ghosted notes draw faintly, while
  section lines and the visible-range box remain.

## Editing

- **Placing** a note, by lane key, mouse, paste or recording, puts it on the active layer. If the
  active layer is hidden, ghosted or locked, nothing is placed and the status bar says why.
- **Selecting** takes notes from shown, unlocked layers only, so a marquee over a crowded stretch
  picks just the parts being worked on. Ctrl+A selects every such note.
- **Moving notes to another layer**: *Move to layer ›* in the notefield's right-click menu, or
  Alt+Shift+1 to 9 for the layer at that row. Dropping a selection on a layer's row is left for
  later.
- **Paste** puts notes on the active layer. **Duplicate** (Ctrl+D) keeps each note's layer, since
  it stays in the same chart.
- **Copy to chart ›** copies a layer, its name, colour, visibility, lock state and notes into
  another difficulty when that layer's name is new there. If the target already has a layer of
  that name, the notes go onto it and its existing colour, visibility and lock state stay.
  Notes that would land on a note already there are left out, and the status bar counts them.
- **Merge down** moves a layer's notes into the one below and removes it. **Delete** on a layer
  with notes asks whether to delete them or merge them down.
- **Recording** captures into the active layer, so a take can be a layer of its own, the "layered
  takes" the [live recording research](research/topics/live-recording.md) lists.

## Playback

The note ticks sound for shown layers only, so soloing a layer lets it be heard alone against the
song. Ghosted and hidden layers play no ticks. The metronome is unaffected.

## Undo

Making, deleting, renaming, recolouring, reordering, merging and copying layers, and moving notes
between them, are edits, each one undo step, returning to the chart and cursor as any edit does.
So are showing, ghosting, hiding and locking a layer, and soloing one: they are kept in the song,
as Quaver keeps a layer hidden, so a song reopens as it was left, and every change to the file
being one undo step keeps undo and *unsaved changes* in agreement. The active layer, and what a
solo will put back, are not saved: a song opens on its first layer, all as it was saved.

## In the file

The format gains two optional fields, described in the [format's Layers section](format.md#layers)
and following its [grouping by value](format.md): layers are
named, and a note names its layer.

```json
"charts": [{
  "name": "Hard",
  "layers": [
    { "name": "Kick", "colour": "red" },
    { "name": "Melody", "colour": "blue", "view": "ghosted", "locked": true }
  ],
  "objects": [
    { "tick": 0, "position": { "lane": { "index": 0 } }, "kind": "tap", "shape": "instant",
      "layer": "Kick" }
  ]
}]
```

- A note without `layer` is on the first layer, so files without layers stay valid and readable
  by other tools. Charter gives such charts *Layer 0* when opening them and writes the layer and
  every note's layer on save. Reordering the layers never moves a note from one to another.
- Validation adds two cross-field rules: a note's named layer must be in its chart, and layer
  names must be unique. A layer name must also be non-empty.
- `view` and `locked` are left out when shown and unlocked.
- Plugins see a note's layer and may ignore it. The built-in games' exported charts merge the
  layers. [Export loss reporting](research/topics/export-loss-reporting.md) requires reporting
  what is lost; **the current TTR exporter drops layer data without that warning**. Keep the charter
  JSON to preserve the names, colours, visibility and locks.

## Keys

| Key | Does |
|---|---|
| Ctrl+Shift+N | New layer above the active one |
| Alt+1 to 9 | Make the layer at that row active |
| Alt+Shift+1 to 9 | Move the selection to the layer at that row |
| Ctrl+Shift+Up, Ctrl+Shift+Down | Make the layer above or below active |
| Ctrl+E | Merge the active layer down |
| Ctrl+Shift+H | Show or hide the active layer |

## Open questions

1. **Per chart or per song?** This draft gives each chart its own layers. Layers shared by every
   difficulty, *Kick* on Easy and Expert alike, would make parts easier to compare across
   difficulties, but a layer would then mean different notes on each.
2. **Alternative versions on one spot.** Layers obey the same overlap rules as the merged chart.
   Allowing otherwise conflicting notes, with only shown layers going into the chart, would let
   alternative versions of a passage sit on top of each other, at the cost of a chart that can be
   invalid while layers are hidden. Ruleset-permitted gesture overlaps already remain legal.
3. **Is the view saved?** This draft keeps visibility and lock in the song file, each change an
   undo step. They could live in charter's settings per song instead, keeping the file free of how
   one person looks at it, and out of undo.
4. **Where paste goes.** This draft pastes onto the active layer. Keeping each note's layer where
   the target chart has one by that name would round-trip copies between difficulties better.
5. **Ticks for ghosted layers.** This draft silences them. Quietly ticking them would help hear
   the reference while charting against it.
