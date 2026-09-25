---
type: Design Note
title: User interface draft
description: Low-fidelity layout of the editor window, with the reasoning behind each region.
tags: [ui, layout, draft]
status: draft
---

# User interface draft

**Draft.** A low-fidelity layout to iterate on. The [research topics](research/topics/index.md) it
draws on are all still open. `cargo run -p charter-ui --example mock` shows it as a static egui
mock, with a made-up song in each of StepMania, Tap Tap Revenge, osu!mania, Project Sekai and
Arcaea. `cargo run -p charter-app` is the editor so far. It draws charts in the shared vocabulary's
glyphs, since no ruleset plugin draws yet, and starts and edits songs of the built-in rulesets. Lane
keys and Ctrl with the mouse place taps, holds and mines; the mouse selects and Delete removes the
selection; undo and redo return to where each edit was made. The song panel shows the tempo at the
cursor: a BPM typed there and entered changes the tempo from the cursor on, as one step, and the BPM
change at the cursor can be removed, unless it is the only one. Below it the song's markers are
listed; clicking one jumps the cursor there, a name typed and entered names the section at the
cursor or starts one, and any marker can be removed, each as one step. Songs open and save through
native file dialogs, unsaved changes are asked about before they are lost and autosaved half a
minute after they are made, and a settings window holds what the user chooses, including which way
the notefield runs and the tick and metronome sounds, which play once charter plays audio. The
inspector shows the selection but cannot change it yet. A new song takes its game, the layout every
chart shares, its title, artist and chart names from a dialog, and asks for audio until an audio
file is dropped on the window; saving copies that file beside the song.

## Main window

```text
+--------------------------------------------------------------------------------------+
| File  Edit  Select  Transform  Timing  View  Help                 [Ctrl+K  commands] |
+-------------------+--------------------------------------------------+---------------+
| SONG              | Hard   dance-single   4 lanes                    | INSPECTOR     |
| Title   Anthem    |------+---------+----+----+----+----+---------+---| 2 taps        |
| Artist  Someone   |ruler |timing   | 1  | 2  | 3  | 4  |waveform |ov | at    13:2:0  |
| Audio   song.ogg  |------+---------+----+----+----+----+---------+---| lane  mixed   |
| Preview 45.0 s  > |  12  |bpm 180  |----+----+----+----|---===---| # | kind  tap     |
|                   |      |         |    | [] |    |    |  =====  | # | shape instant |
| CHARTS            |=now==|=========|====|====|=[]=|====|=========|[#]| sample -      |
|   Easy         4L |      |stop 0.5 | [] |    |    | [] |   ==    |[#]|               |
| > Hard         4L |      |         |    |    |    | || |  ====   | # |               |
|   Challenge    4L |  13  |         |----+----+----+----|---===---| # |               |
|   Doubles      8L |Chorus|~~~~~~~~~|~~~~|~~~~|~~~~|~~~~|~~~~~~~~~| - |               |
| [new] [copy] [del]|      |bpm 190  | [] | [] |    |    |  =====  | # |               |
|                   |      |         |    |    | xx |    |   ==    | ! |               |
| CHART SETTINGS    |  14  |         |----+----+----+----|--====---| # |               |
| difficulty  Hard  |      |         |    |    |    | [] |   ===   |   |               |
| meter       9     |      |         |    |    |    |    |         |   |               |
|                   |      |         |    |    |    |    |         |   |               |
| SAMPLES           |      |         |    |    |    |    |         |   |               |
| kick    kick.wav  |      |         |    |    |    |    |         |   |               |
+-------------------+------+---------+----+----+----+----+---------+---+---------------+
| PROBLEMS  3 notes inside the warp at 32:0:0 are unreachable                          |
+--------------------------------------------------------------------------------------+
| [>]  12:2:0  00:16.667 | rate 100% | snap 16th | zoom -o- | C-mod | metronome | EDIT |
+--------------------------------------------------------------------------------------+
```

The notefield gets the centre and most of the width. The other regions are fixed panels that
collapse; nothing docks or floats, since one notefield has nothing to rearrange against. Docking
can come with a second view. In a narrow window the timing column and the waveform give up width
before anything else, since the lanes and ruler are what charting needs.

The window is dark whatever the system prefers. Charting means long sessions against a game's
dark playfield, often in a dim room, and notes read best as bright shapes on dark lanes.

- The song panel follows the document: metadata, charts, the selected chart's settings and the
  sample bank. The chart list copies ArrowVortex's, which the
  [format](format.md#one-ruleset-per-file) names as the reference. Switching charts keeps the
  cursor on its tick, so two difficulties can be compared at one spot.
- Chart settings, plugin `ext` fields and plugin timing values are forms the owning plugin
  supplies. Without that plugin
  they show as raw text, read-only, since the format lets a reader carry data it cannot
  meaningfully edit.
- The inspector shows the fields of the selection: tick, lane, kind, shape, sample and `ext` for
  objects, the value of a timing event, the text of a marker. A value that differs across the
  selection reads `mixed`, and setting it sets it on all. Keys place notes; the inspector takes
  what keys cannot express, such as 174.5 BPM, a 0.25 s stop or a keysound.
- Every action is a named command with a rebindable key. Menus carry the common ones, and
  `Ctrl+K` searches all of them and shows each binding. All four major editors surveyed ship
  customizable shortcuts, and Tempora's drag modifiers stay invisible until learned
  ([shortcuts](research/topics/shortcuts.md)).
- There is no toolbar or tool palette. Entry is modeless, as in ArrowVortex: lane keys place notes
  at the cursor, and the mouse selects, with Shift adding to a marquee and Alt subtracting. Ctrl
  turns the mouse to placing: a click toggles a tap on the nearest snap line, Ctrl+Shift a mine,
  and a drag along a lane draws a hold.
- Undo takes back one edit at a time and returns to the chart and the cursor it was made at, as
  Moonscraper's returns its camera ([undo model](research/topics/undo-model.md)). One lane key
  press is one step, a hold drawn from the note it grew out of is one, and so is deleting a
  selection.
- The problems strip lists warnings about a valid document, such as the unreachable notes the
  format flags inside a warp, and clicking one jumps there. It never lists errors, because the
  editor never holds an invalid document. A file that breaks a rule does not open, and the message
  names where, such as `charts[1].objects[4]`. An edit that would break a rule is refused, and the
  object in its way flashes, as Tempora does with a blocking neighbour.
- The status bar holds what changes every few seconds while charting: transport, position as
  `measure:beat:tick` and time, playback rate, snap, zoom, view mode, metronome and mode.
  ArrowVortex allows rates from 10 to 400%, and its menu snaps, 4th to 192nd, are all exact at 960
  ticks per beat.

## The notefield

```text
  ruler    timing     lanes               waveform   overview
+--------+----------+----+----+----+----+-----------+---+
|        | song     | 1  | 2  | 3  | 4  |           |   | <- whose timing; lane keys
+--------+----------+----+----+----+----+-----------+---+
|   12   | bpm 180  |----+----+----+----|-----=-----| # | <- measure line
|        |          |    | [] |    |    |    ===    | # |
|        |          | [] |    |    |    |   =====   |[#]| <- overview: visible range
|==now===|==========|====|====|=[]=|====|===========|[#]| <- now line: cursor, playhead
|   13   |          |----+----+----+----|---=====---| # |
|        | stop 0.5 | [] |    |    | [] |  =======  | # |
|        |          |    |    |    | || |   =====   | # | <- stop: time passes, no ticks
|        |          |    |    |    | || |    ===    | - | <- overview: a section
| Chorus |~~~~~~~~~~|~~~~|~~~~|~~~~|~~~~|~~~~~~~~~~~| # | <- section marker
|   14   | bpm 190  |----+----+----+----|-----=-----| ! | <- overview: a warning
|        | sm.scroll| [] | [] |    |    |   =====   | # |
|        |   0.5    |    |    | xx |    |    ===    | # | <- plugin event, raw value
+--------+----------+----+----+----+----+-----------+---+
 '----- editor ----' '---- ruleset ----' '-- editor ---'

  []  tap    ||  hold    xx  mine    =  waveform    #  density
```

- It scrolls vertically, as ArrowVortex, StepMania, osu!mania, Quaver and Moonscraper do, and as
  all three rulesets here play. StepMania's editor runs time downward and osu!mania's upward, so
  direction is a view setting whose default comes from the ruleset. The settings window turns the
  notefield today: vertical with notes travelling up (the default) or down, or horizontal with
  them travelling left or right. The notefield lays itself out one way and is turned onto the
  screen as a whole, pointer included, so every edit and selection works in each direction. A
  default per ruleset is still to come.
- The columns share one time axis, and only the lanes column belongs to the ruleset. The ruleset
  draws and hit-tests its own kinds there, including ones the core has never heard of, such as a
  Tap Tap Revenge slice or a mega tap across every lane. A plane-based ruleset draws its own
  projection there, and may add a second view beside it on the same time axis. Arcaea needs one:
  from above, two arcs can meet where they are really at different heights, so a strip beside
  the floor and sky shows each arc's height from the side. The editor draws the other columns
  the same way in every game, which keeps game-specific code out of the UI crate.
- Without the ruleset plugin, the editor draws `lanes` plain columns with one glyph per shared kind,
  which is why the format keeps `lanes` on the chart. The chart then opens read-only, with a
  banner naming the missing plugin, because only the ruleset can check an edit against the game:
  moving a Tap Tap Revenge shake out of the lane of its direction is a plain lane move that breaks
  a rule.
- The lane header shows the key bound to each lane. Bindings differ by ruleset and layout, and the
  scheme above ten lanes is still [open](research/topics/wide-layouts.md); the header shows
  whichever is active. In record mode, clicking a header arms the lane.
- The now line is the cursor. It stays near one edge, leaving most of the view for what comes
  next, and during playback the chart scrolls past it as in the game. Lane keys place notes on it,
  and auto-jump then moves it one snap, so each note of a stream takes one keystroke
  ([note entry](research/topics/note-entry.md)). The cursor moves on once every lane key is up, so
  keys held together place a chord on one tick. A lane key on a note removes it and leaves the
  cursor where it is. Holding a lane key while the cursor moves stretches its tap into a hold to
  the cursor, and Shift with a lane key places a mine.
- The waveform has its own strip. ArrowVortex draws it behind the notes; a strip keeps one width
  whether the chart has 3 lanes or 20, and leaves the lanes column to the ruleset. A spectrogram
  can sit beside it as another strip. Measure and beat lines cross the lanes and the waveform
  alike, which is what lining a note up with an onset needs.

Both [view modes](research/topics/scroll-and-zoom.md) exist from the start, since each draws
something the other cannot:

| | C-mod | X-mod |
|---|---|---|
| Height is | seconds | beats |
| Waveform | evenly spaced | stretched per tempo |
| A stream | spacing follows tempo | same spacing at any tempo |
| A stop | time passes with no ticks | its audio gets no height |
| A warp | collapses to one line | keeps its height, shaded unreachable |

The notefield opens in C-mod. Sync is the first task on every song
([beat detection](research/topics/beat-detection.md)), and in C-mod a timing edit moves the grid
and the notes over a waveform that stays put.

- The timing gutter lists every event of the timing this chart uses: the five kinds the core
  interprets, and plugin events by key and raw value, since the core does not interpret those.
  Its header reads `song` or `chart`, so before a timing edit the author knows whether it reaches
  every chart sharing the song's timing or this chart alone. A chart's own timing
  [replaces](format.md#per-chart-timing) the song's entirely, offset included.
- Sync happens in the notefield. Ctrl-dragging a beat line onto an onset moves the origin, and
  Shift-dragging does it by changing the BPM leading up to that line. The modifiers are Tempora's,
  and the effect is ArrowVortex's visual sync, which moves a row to a target time through the
  offset or a BPM. Fine and coarse nudges take Tempora's 2 ms and 10 ms. The
  [sync workstation](research/topics/sync-workstation.md) topic finds no evidence that sync needs
  a view of its own, so this draft has none.
- The ruler numbers measures from the time-signature grid and labels markers. Measure 0 is the
  one holding tick 0, and measures before it count down from -1. Sections and bookmarks draw as
  lines across every column, since the core knows them without the ruleset.
- The overview is the whole chart in the notefield's direction: density, sections, bookmarks,
  warnings, the selection and a box around the visible range. Clicking or dragging seeks.
  ArrowVortex's minimap does the same job.

## Modes and dialogs

- Record mode keeps the layout. Armed lanes light up in the header, recording starts at the cursor
  after a count-in, and captured notes show as ghosts until the pass ends, then commit as one undo
  step. The rest is [open](research/topics/live-recording.md).
- Rare work goes in dialogs: a new song, whose audio is dropped on the window once it is open,
  and export, BPM detection and settings. Export lists what the target format will lose
  and asks before writing ([export loss reporting](research/topics/export-loss-reporting.md)).
  Detection lists candidates and applies none until one is picked
  ([suggestion UI](research/topics/suggestion-ui.md)).
- With no song open, the centre offers new, open and recent songs.
- Closing the window, opening another song or the demo over unsaved changes first asks whether to
  save them. The title stars a song with unsaved changes.
- Unsaved changes are autosaved to one recovery file in charter's own data folder, never beside
  the song, half a minute after the first change since the last autosave. Saving the song, undoing
  back to it or throwing the changes away removes the file. On the next start a recovery file
  left behind is offered for restoring: the song comes back unsaved, for the file it belongs to.
  Saves write beside the song and move over it, without forcing the write to disk, since some
  filesystems refuse that; the autosave is the cover for a crash.

## Open for the next pass

- The waveform behind the notes instead of beside them
- C-mod as the default, or a default per ruleset
- Tempora's measure-per-row view for sync, if the notefield proves clumsy
- Test play, which osu! and Quaver have and this draft leaves out
- Two charts side by side, which the column layout would allow
- Plugin timing keys too long for the gutter, such as `osu_mania.scroll_speed`
