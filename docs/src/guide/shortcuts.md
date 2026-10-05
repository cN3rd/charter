---
type: Reference
title: Keyboard shortcuts
description: Every key and mouse gesture of the editor and the timing window.
tags: [guide, keys, reference]
status: draft
---

# Keyboard shortcuts

F1 shows this list in the editor, with a filter.

Settings, shortcut help and Import timing are modeless windows. Opening them does not disable
chart editing: lane keys can still change the song behind them when no text field has the
keyboard. A focused text field takes typed input; the timing window takes its own keys after a
click inside it.

## Note entry

| Keys | Action |
|---|---|
| 1 to 9, 0 | Place or remove a tap; while playing, record on the nearest snap |
| Hold 1 to 0, move | Draw a hold; while playing, hold past 200 ms to grow it |
| Shift+1 to 0 | Place or remove a mine |
| Ctrl+Click | Place or remove a tap |
| Ctrl+Shift+Click | Place or remove a mine |
| Ctrl+Drag | Draw a hold |
| `[`, `]` | Shorten or lengthen the selection, or the notes at the cursor, by the snap |

## Selection

| Keys | Action |
|---|---|
| Click, Drag | Select notes |
| Drag a note | Move it, or the selection it is in |
| Shift+Drag a note | Move it across lanes, keeping its time |
| Right-click | Menu: edits, patterns, the game's note changes |
| Ctrl+X, Ctrl+C, Ctrl+V | Cut, copy, or paste at the cursor |
| Ctrl+D | Duplicate the selection after itself |
| Ctrl+Left, Ctrl+Right | Move the selection a lane |
| Ctrl+Up, Ctrl+Down | Move the selection by the snap |
| Ctrl+M | Mirror the selection across the lanes |
| G | Snap the selection to the nearest snap lines |
| Shift+Click, Shift+Drag | Add to the selection |
| Alt+Click, Alt+Drag | Take from the selection |
| Ctrl+A, Escape | Select every note on shown, unlocked layers, or none |
| Delete, Backspace | Delete the selection |
| Ctrl+Z | Undo |
| Ctrl+Shift+Z, Ctrl+Y | Redo |

## Layers

| Keys | Action |
|---|---|
| Ctrl+Shift+N | Add a layer above the active one |
| Alt+1 to 9 | Make that layer active |
| Alt+Shift+1 to 9 | Move selected notes to that layer |
| Ctrl+Shift+Up, Ctrl+Shift+Down | Choose the adjacent layer |
| Ctrl+E | Merge the active layer down |
| Ctrl+Shift+H | Show or hide the active layer |

## Moving around

| Keys | Action |
|---|---|
| Up, Down, Wheel | Move the cursor by the snap |
| Page Up, Page Down, Shift+Wheel | Move the cursor by a measure |
| Middle-drag | Pan by moving the cursor; while playing, seek the audio |
| Home, End | Jump to the start, or to where the last note ends |
| Alt+Up, Alt+Down | Jump to the previous or next section |
| Left, Right | Coarser or finer snap |
| Ctrl+Wheel, Ctrl+Plus, Ctrl+Minus | Zoom |

## Timing

| Keys | Action |
|---|---|
| Ctrl+T | Show or hide the timing window |
| Ctrl+B | Add or remove a bookmark at the cursor |
| F7, F8 | Lower or raise the tempo at the cursor by 1 BPM |
| Alt+Left, Alt+Right | Lower or raise the offset by 1 ms, or by 10 ms with Shift |

## Playback

| Keys | Action |
|---|---|
| Space | Play from the cursor, or pause |
| Q | Play a moment before the cursor |
| W | Play a moment after the cursor |
| Shift+Left, Shift+Right | Play slower or faster by 10% |
| Ctrl+Shift+Left, Ctrl+Shift+Right | Play slower or faster by 1% |
| F3, F4 | Turn the metronome or the note ticks on or off |
| Shift+F4 | Tune each lane's note ticks to a pitch of its own, or back |

## Files and windows

| Keys | Action |
|---|---|
| Ctrl+N | New song |
| Ctrl+O, Ctrl+S, Ctrl+Shift+S | Open, save, save as |
| Ctrl+Comma | Settings |
| Ctrl+\\ | Show or hide the song panel |
| F1 | Keyboard shortcuts |

## The timing window

These work once the timing window has the keyboard, after a click in it. ? lists them there.

| Keys | Action |
|---|---|
| Click | Add an anchor at the strongest onset nearby |
| A | Add an anchor at the audio under the playhead |
| Drag an anchor, Ctrl+Drag | Move its beat, or its moment in the audio; Shift for finer and unsnapped |
| Drag elsewhere, Ctrl+A | Select the anchors inside, or every anchor |
| J, K | Focus the anchor before or after |
| 1 to 0 | Grid from 1/1 to 1/32 of a whole note |
| Left, Right | Nudge by 1 ms, 0.1 ms with Shift, 10 ms with Ctrl |
| Up, Down | Move a grid step |
| +, - | Raise or lower the last section's tempo by 0.1 BPM, 1 BPM with Shift |
| Delete, Backspace | Remove the anchor |
| F, Shift+F | Fit to the onsets, or to half BPMs |
| Enter, Escape | Apply or cancel a fit |
| D, Shift+D | Double or halve the tempo |
| N, Shift+N | Next or previous issue |
| Space | Play from the cursor, or pause |
| L, Shift+L | Loop the bar under the cursor, or the selection |
| R | Switch to the alternate speed and back |
| C, Shift+C | Turn the metronome on or off, or mute it while held |
| O | Scroll the rows with the playhead, or stop |
| V | Rows of audio time or rows of bars |
