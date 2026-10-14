---
type: Design Note
title: User interface draft
description: Layout of the editor window, the reasoning behind each region, and how what is built so far works inside.
tags: [ui, layout, draft]
status: draft
---

# User interface draft

**Draft.** A low-fidelity layout to iterate on, and a record of what is built. The
[research topics](research/topics/index.md) it draws on are still open. The
[user guide](guide/index.md) describes how to use the editor; this chapter covers why it is laid
out as it is and how its parts work.

`cargo run -p charter-app` is the editor. `cargo run -p charter-ui --example mock` is a static mock
of the layout below, with a made-up song in each of StepMania, Tap Tap Revenge, osu!mania, Project
Sekai and Arcaea. No ruleset plugin draws yet, so charts draw in the shared vocabulary's glyphs.

## Built so far

### Editing

- Entry, selection, the clipboard, patterns and the game's note changes work as the guide's
  [Charting](guide/charting.md) chapter describes. The right-click menu leaves the cursor where it
  is, and what it pastes or places goes on the line clicked.
- Each change is one undo step: a lane key press, a hold drawn from the note it grew out of, a move
  or deletion of a selection, a chord recorded while playing, a song panel field.
- Every typed value applies on Enter or when its field is left, and Escape restores it.
- Saving, opening, settings and help keep their keys while a text field has the keyboard.
- The [Layers](layers.md) chapter covers the song panel's Layers section and its keys.

### Song panel

- The Song section edits the facts in place. A plugin's text settings, such as Tap Tap Revenge's
  `theme`, follow as fields of their own, the song's there and the open chart's under the chart
  list. The ruleset shows as it is.
- An audio file's tags fill in a title and artist still at their defaults, in the new-song dialog
  or on a new song; where the user has written their own, charter asks before importing them.
  Until audio is chosen the panel shows "None yet" rather than a file name.
- The Timing section is laid out as osu!'s timing panel is. Point rows keep their values on one
  line, and hovering shows a long game event in full. The tempo shown is the shortest decimal that
  puts the next point on the same tick, which is the one typed.
- Marker rows stay compact, leaving room for every lane of an eight-lane chart at 800 pixels wide;
  hovering shows a long name in full.

### Timing points

The timing events at one tick form one timing point, as osu! has them: one line in the notefield,
its values stacked beneath it (`150 BPM`, then `3/4`), and one handle. Every tempo point stays at
its moment of the audio: a BPM set on one changes how many beats fit before the next point, and the
next point moves along the grid with everything after it, as EOF's anchors and osu!'s red lines do.
A drag shows a guide where the point will land, labelled with the time or the beat. F7 and F8 nudge
the tempo at the cursor.

### Sound

- Playback runs from 10% to 400%. From 10% to 200% the audio keeps its pitch, stretched as it plays
  by timestretch's wide-range keylock, a phase vocoder. One stretcher reaches down to 50%, so slower
  speeds pass the audio through several in turn. Stretching moves the audio by a few milliseconds
  that depend on the speed, so a click is played through when playback starts to measure by how
  much, and the audio is fed that much sooner or later to stay on the cursor.
- The metronome and note ticks are mixed in after stretching, each at its time scaled to the speed
  but at its own speed and pitch. Mixing them in before playback means they cannot drift from the
  music, at the cost of a volume change while playing restarting from where it was once its slider
  is let go. With no clicks on, the music's volume changes live.
- Tuned lanes pitch each lane a whole tone apart either side of the chart's middle, closer on charts
  wider than seven lanes so every chart spans an octave at most, by playing the tick faster or
  slower as a tuned drum sounds.
- The nine metronome kits and the four hit sounds are synthesised and dedicated to the public
  domain, bundled under `crates/charter-audio/assets/clicks` and `assets/hitsounds` with their CC0
  licence and a note on how they were made.

### Audio strip

- The waveform draws from a peaks file, `cache/<audio>.peaks` in the song's folder, keeping any
  relative audio subfolder: the finest min and max levels every view reads, so the notefield, the
  timing window and any view to come draw from the same data at any zoom. The file names the audio
  by a fingerprint of its bytes, so replaced audio gets new peaks, and a damaged or outdated file is
  ignored. With the peaks kept, the waveform shows before the audio has decoded. Nothing is written
  for audio outside the song's folder.
- Frequencies shows 48 bands from 30 Hz on the left to 16 kHz on the right. It draws from images of
  the spectrum, a band a column and 5 ms a row, which the GPU stretches over the strip and smooths
  between; zoomed out, each row holds the loudest of the frames it covers, so a short sound still
  shows.
- The grid draws lines between beats down to the chosen note value, each beat's brighter. Where the
  snap has lines the grid leaves out, the notefield draws the snap's instead, so the cursor always
  lands on a drawn line.

### Files

- A song saved somewhere new becomes a folder holding its JSON file under the same name, a copy of
  its audio, and anything worked out from the audio. Audio in a relative subfolder keeps it.
  Different audio already at the destination, or a failed copy, refuses the save without changing
  the open song or its JSON.
- Saves write beside the song and move over it, without forcing the write to disk, since some
  filesystems refuse that; the autosave covers a crash.
- Errors shown in the window name each cause behind them, `: ` apart. A panic prints color-eyre's
  report on a terminal, with a backtrace when `RUST_BACKTRACE=1` is set, writes a crash file with a
  backtrace, and shows a dialog when the window's own thread panicked. A panic on a worker thread,
  such as the timing finder's, writes the file alone.
- Under Wayland nothing can be dropped on the window, since winit takes no drops there yet
  ([winit#1881](https://github.com/rust-windowing/winit/issues/1881)), so the window offers only its
  choose buttons and says why.

### Look

- The look follows pro audio and creative tools: a transport toolbar under the menus with a large
  position readout and grouped tempo, snap, speed and zoom controls, collapsible sections in the
  side panels, the notefield in a dark well, and one blue accent for selection and primary actions.
- Inter is the text font and Phosphor the icon font, bundled under
  `crates/charter-style/assets/fonts` with Inter's OFL licence. The system's Japanese, Chinese and
  Korean fonts follow them whatever the locale, found with the `system-fonts` crate and read once,
  so titles in those scripts draw.
- Colours are kept apart by what shares a surface, not by taste. A layer's colour stays at least
  15° of hue from the section and bookmark markers and from the accent, and its stripe down a
  note's leading edge sits on a lane-dark gutter, so it reads whatever kind of note it marks. The
  timing window gives blue to the waveform, orange to anchors and a green-and-magenta pair to the
  early and late estimates, which lie on top of both; early ones fill the top half of the strip
  under a row and late ones the bottom, so the pair reads without colour. A selected anchor draws
  at twice the playhead's width, since both are near-white. A refused edit pulses red rather than
  the warning amber an anchor is already drawn in. `charter-style`'s tests hold each of these.
- Messages float over the canvas rather than moving it, and each is an assertive live alert, so a
  screen reader reads a failure out as it appears. A toolbar button that holds a state, such as the
  metronome, reports that state under one name as well as lighting up, since the accent alone says
  nothing to a screen reader. An off switch or checkbox is a dark well whose edge reaches 3:1
  against the panels behind it, that edge being all there is to find the control by, and it stays
  unlit beside the accent fill of one that is on.
- As the window narrows, the transport drops its secondary tempo, snap, speed, grid and zoom
  controls, keeping playback, position, sound and settings; the menus, keys and settings still
  offer the rest. The settings window puts its page choice above its controls, the shortcut sheet
  stacks each key above its explanation, and the new-song and import forms scroll so their actions
  stay in reach.
- The settings window has a sidebar of pages, each setting with a line on what it does and on and
  off choices as switches. Only settings changed from their defaults are kept, so one left alone
  follows its default when a later version changes it, and one a later version cannot read falls
  back to its default alone.
- Settings, shortcut help and Import timing are modeless. Opening them does not disable lane
  keys; notes can still change behind them when no text field has the keyboard. The timing
  window has its own keyboard ownership after a click inside it.

## Main window

The sketches include proposed regions and controls. The current window has the transport under
the menu bar, no inspector or problems strip, and the waveform between the timing gutter and
the lanes. The user guide describes the shipped layout; planned behavior is labelled below.

```text
+--------------------------------------------------------------------------------------+
| File  Edit  Select  Transform  Timing  View  Help                  [F1  shortcuts]   |
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
| LAYERS       [+][…]|      |         |    |    | xx |    |   ==    | ! |               |
| [o] [ ] ■ Kick  52|      |         |    |    |    |    |         |   |               |
|>[o] [ ] ■ Snare 31|      |         |    |    |    |    |         |   |               |
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

The notefield gets the centre and most of the width. The song panel collapses; nothing docks or
floats, since one notefield has nothing to rearrange against. Docking can come with a second view.
In a narrow window the timing column gives up width before the lanes; the audio strip has a fixed
width.

The window is dark whatever the system prefers. Charting means long sessions against a game's
dark playfield, often in a dim room, and notes read best as bright shapes on dark lanes.

- The song panel follows the document: metadata, charts, the selected chart's settings and the
  sample bank. The chart list copies ArrowVortex's, which the
  [format](format.md#one-ruleset-per-file) names as the reference. Switching charts keeps the
  cursor on its tick, so two difficulties can be compared at one spot.
- Plugins currently supply editable text settings, such as a TTR theme. Other chart settings,
  such as StepMania's meter and osu!mania's HP and OD, show as read-only values. Forms for
  arbitrary plugin `ext` fields and timing values remain planned.
- There is no inspector panel; the notefield keeps the width. The status bar counts the selection,
  the song panel sets the tempo, offset and sections at the cursor, and what keys cannot express
  on a note, such as a keysound, is still to find a place.
- Menus carry the common actions, and `F1` opens a searchable keyboard shortcut sheet. Rebindable
  keys and a command palette are still planned; all four major editors surveyed ship customizable
  shortcuts, and Tempora's drag modifiers stay invisible until learned
  ([shortcuts](research/topics/shortcuts.md)).
- There is a transport toolbar, but no tool palette. Entry is modeless, as in ArrowVortex: lane
  keys place notes at the cursor, and the mouse selects, with Shift adding to a marquee and Alt
  subtracting. Ctrl turns the mouse to placing: a click toggles a tap on the nearest snap line,
  Ctrl+Shift a mine,
  and a drag along a lane draws a hold, showing its body to the snap line under the pointer before
  release. A plain drag from a note moves it, or the selection it is
  in, by whole snaps and across lanes, kept on the lanes, with an outline where the notes will
  land; letting go moves them as one step, and a move onto a note that stays is refused.
- Undo takes back one edit at a time and returns to the chart and the cursor it was made at, as
  Moonscraper's returns its camera ([undo model](research/topics/undo-model.md)). One lane key
  press is one step, a hold drawn from the note it grew out of is one, and so is deleting or
  moving a selection.
- A problems strip is planned for warnings about a valid document, such as the unreachable notes
  the format flags inside a warp, with clicks jumping to them. It would not list errors, because
  the editor never holds an invalid document. A file that breaks a rule does not open, and the message
  names where, such as `charts[1].objects[4]`. An edit that would break a rule is refused, and the
  object in its way flashes, as Tempora does with a blocking neighbour.
- The transport toolbar holds position as `measure:beat:tick` and time, playback rate, snap,
  zoom, grid and sound controls. The status bar shows audio state, selection count and active layer.
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

- Direction is a view setting: vertical with notes travelling up (the default) or down, or
  horizontal with them travelling left or right. The notefield lays itself out one way and is
  turned onto the screen as a whole, pointer included, so editing and selection work in each
  direction. A default per ruleset remains planned; StepMania's editor runs time downward and
  osu!mania's upward.
- Planned plugin drawing keeps the columns on one time axis, with the ruleset drawing and
  hit-testing its own kinds in the lanes column. Today every chart uses the generic glyphs.
  A future plane-based ruleset could draw its own projection and add a second view beside it.
  Arcaea needs one:
  from above, two arcs can meet where they are really at different heights, so a strip beside
  the floor and sky shows each arc's height from the side. The editor draws the other columns
  the same way in every game, which keeps game-specific code out of the UI crate.
- Without the ruleset plugin, the editor draws `lanes` plain columns with one glyph per shared kind,
  which is why the format keeps `lanes` on the chart. The chart then opens read-only, with a
  banner naming the missing plugin, because only the ruleset can check an edit against the game:
  moving a Tap Tap Revenge shake out of the lane of its direction is a plain lane move that breaks
  a rule.
- The lane header shows the fixed digit bindings for the first ten lanes. Wider layouts can be
  edited with the mouse; per-ruleset bindings and the scheme above ten lanes remain
  [open](research/topics/wide-layouts.md). Clicking a header to arm a recording lane is planned.
- The now line is the cursor. It stays near one edge, leaving most of the view for what comes
  next, and during playback the chart scrolls past it as in the game. Lane keys place notes on it,
  and auto-jump then moves it one snap, so each note of a stream takes one keystroke
  ([note entry](research/topics/note-entry.md)). The cursor moves on once every lane key is up, so
  keys held together place a chord on one tick. A lane key on a note removes it and leaves the
  cursor where it is. Holding a lane key while the cursor moves stretches its tap into a hold to
  the cursor, and Shift with a lane key places a mine.
- The waveform has its own strip. ArrowVortex draws it behind the notes; a strip keeps one width
  whether the chart has 3 lanes or 20. Frequencies replaces the waveform in that strip; a separate
  spectrum strip remains an option. Measure and beat lines cross the lanes and the waveform
  alike, which is what lining a note up with an onset needs.

The current view is time-based C-mod. Beat-based X-mod remains planned; this table compares
the two [view modes](research/topics/scroll-and-zoom.md):

| | C-mod (built) | X-mod (planned) |
|---|---|---|
| Height is | seconds | beats |
| Waveform | evenly spaced | stretched per tempo |
| A stream | spacing follows tempo | same spacing at any tempo |
| A stop | time passes with no ticks | its audio gets no height |
| A warp | collapses to one line | keeps its height, shaded unreachable |

The notefield opens in C-mod. Sync is the first task on every song
([beat detection](research/topics/beat-detection.md)), and in C-mod a timing edit moves the grid
and the notes over a waveform that stays put.

- The timing gutter lists the anchors and events of the timing this chart uses: the four
  interpreted event kinds and plugin events by key and raw value.
  Its header reads `song` or `chart`, so before a timing edit the author knows whether it reaches
  every chart sharing the song's timing or this chart alone. A chart's own timing
  [replaces](format.md#per-chart-timing) the song's entirely, offset included.
- Beat-line modifier sync is planned: Ctrl-drag would move the origin and Shift-drag would
  change the preceding BPM, borrowing Tempora's modifiers and ArrowVortex's visual-sync effect.
  Today tempo points drag in the timing gutter and anchors move in the timing window. Offset
  nudges use 1 ms, or 10 ms with Shift.
- The ruler numbers measures from the time-signature grid and labels markers. Measure 0 is the
  one holding tick 0, and measures before it count down from -1. Sections and bookmarks draw as
  lines across every column, since the core knows them without the ruleset.
- The minimap shows notes by lane, sections and a box around the visible range; clicking or
  dragging seeks. Density, bookmark, warning and selection overlays from the sketch remain
  proposals. ArrowVortex's minimap is the reference.

## The timing window

Timing > Timing window, Ctrl+T, opens a window in its own crate, `charter-timing`, that follows the
Beatline specification for the parts in scope. It hands its edits back as commands, so they go
through the song model like any other.

### Layout

- It opens on rows of audio time, each one to eight seconds and labelled by its start, so the
  waveform never moves and an edit moves the grid lines and anchors over it, since every timing
  point is a moment of the audio. V, or the toolbar's Layout, switches to rows of bars, half a bar
  to four bars each, a tenth of the bar before shown ahead of each downbeat. Those keep the grid
  still and slide the audio under it, as Tempora and the Beatline specification do. Both draw the
  audio at the times the song's anchors give it.
- Downbeats draw thick and beats solid, with the beats numbered. Lines between them are dashed,
  or dotted on triplets. The grid runs from 1/1 to 1/32 of a whole note on keys 1 to 0, and
  restarts on every downbeat.
- The waveform shows here whether or not the notefield's is on.

### Anchors

- Every BPM change is an anchor pinning a beat to a moment of the audio, as in
  [sync anchors](research/topics/sync-anchors.md). Edits rebuild the tempo from the anchors, so
  the others keep their moments.
- A click adds an anchor, its moment snapped to the strongest onset within 25 ms and its beat to
  the grid, refused within half a grid step of another, and auditioned from a beat before.
- A drag on an anchor moves its beat, Ctrl-drag its moment, Shift finer and unsnapped; a drag
  elsewhere selects the anchors inside. An anchor never passes its neighbours: a drag stops short
  and the neighbour pulses.
- Arrows nudge 1 ms (Shift 0.1 ms, Ctrl 10 ms) and move a grid step. + and - set the tail tempo,
  and Delete removes. A run of nudges undoes as one step.
- The toolbar's Start BPM sets the first anchor's tempo from anywhere.

### Measuring

- The window measures the map against the onsets the host finds in the audio: ticks along each
  row's foot, a strip under it coloured by how early or late each onset near a line falls, and a
  badge on bars more than 5 ms off.
- The issue list holds bars off, segments shorter than a beat, tempo jumps over 3%, anchors more
  than 2 ms from their onset, and metre changes without an anchor. N and Shift+N step through it.
  Issues and refusals name the moment they are at, and a refused edit says why in the status line.
- The inspector shows the focused anchor's moment, bar position, tempo and distance to its onset,
  each editable. Its segment's fit error and coverage show beside them. F fits the focused anchor or
  the selection to the onsets by least squares, Shift+F to half BPMs, shown before Enter applies it.
  D and Shift+D double and halve the tempo.

### Listening

- L loops the bar under the cursor and Shift+L the selection, with the metronome. The loop's ends
  follow the map, and a committed edit plays it again from its start so the fix is heard at once.
- R switches to the alternate speed, 60% unless the Sounds page says otherwise, and back.
- C turns the metronome on and off, and it stays muted while Shift+C is held. It can follow the
  window's grid, with the clicks between beats at their own volume.

### Keyboard

The window takes the keyboard after a click in it and gives it back after a click elsewhere. ?
lists its keys in a scrolling sheet, stacked when narrow so the rows and inspector stay in view.

### Not built yet

From the specification: scrub, pan split, count-in, the magnifier, locks, the metre dialog, the
command palette, the overview, beat-stack view, auto-seed, suggested anchors, tap tempo and band
solo. Selection changes are not undo steps.

## Latency calibration

Settings > Sounds calibrates latency, as does Playback > Calibrate latency. A click plays 32 times
at the song's first tempo, folded into 60 to 180 BPM, in the metronome's sound, its measures
accented.

- The audio latency is set by ear, moving it until a flash lands on each click heard. The cursor
  then shows what is heard rather than what plays, and the status line shows it.
- The input latency is measured, the median of how late sixteen taps along come past the audio
  latency. A lane key pressed while playing places its note that much earlier than the cursor,
  holds ending likewise.
- Settings kept by older versions, with the two latencies as one, keep it as the input latency.

The clicks are mixed into the music before it plays, so they need no correction of their own.

## Timing menu

- Reset timing… asks first, then puts 120 BPM in 4/4 from 0 ms in place of the open chart's timing,
  as one step: every BPM change, meter, stop and game timing event goes, and the notes keep their
  beats.
- Find timing with Beat This!… and Find timing with Mapperatorinator… draft the open chart's timing
  from its audio and put it in place as one step, as [automatic timing](autotiming.md) describes.
  The first time each runs it asks before downloading its model, and the status bar shows its
  progress. Without a graphics card Mapperatorinator asks whether to run on the CPU.
- Import timing… reads another program's timing, pasted or from a file, and shows what
  it holds before it replaces the open chart's tempo, meters and offset as one step. Each ruleset
  plugin may read a format through `RulesetPlugin::import_timing`; osu!mania reads osu!'s
  `[TimingPoints]`. A red line becomes a BPM change on the nearest tick, placed so rounding never
  adds up past half a tick, with a time signature where the meter changes or the red line falls
  inside a measure, since osu! starts a measure at each; the first sets the offset. Green lines
  become `osu_mania.scroll_speed` in an osu!mania song, reset to 1 at each red line as osu! does,
  and are left out of any other game's, which the window says, as it does for volumes, hit sounds
  and kiai. On a small window, the import form scrolls so its Cancel and Replace actions can still
  be reached.

## Modes and dialogs

- Current playback recording places snapped notes immediately on the active layer, with a chord
  and its holds undoing together. A dedicated mode with armed lanes, count-in, ghost takes and
  one undo step per pass remains [planned](research/topics/live-recording.md).
- New song, format selection, timing import, settings and model-download consent have windows
  or questions. Export reports known losses after conversion, without a loss-confirmation step;
  layer losses are still unreported ([export loss reporting](research/topics/export-loss-reporting.md)).
  Automatic timing applies the requested result as one undo step, with no candidate picker
  ([suggestion UI](research/topics/suggestion-ui.md)).
- With no song open, the centre offers new, open and recent songs.
- Closing the window, opening another song or the demo over unsaved changes first asks whether to
  save them. Starting a new song does too; Cancel keeps its filled-in draft.
- The recovery file lives in charter's data folder, never beside the song, and is written half a
  minute after the first change since the last autosave. Turning autosave off stops new autosaves
  and leaves one already kept for the next start. Snapshots are separate from it, and remove only
  their own song's dated copies. [Files and recovery](guide/files.md) describes both.
- Import song… and Export song… go through the plugin's song formats, as
  [Ruleset plugins](rulesets.md) describes. When several formats share the file's extension, a
  dialog asks which game the file is for. Both report known conversion losses; the current
  reports are incomplete.
- File > Open recent lists the last ten songs opened or saved, the latest first, and clears them; a
  song that no longer opens leaves the list, and Restore defaults keeps it. Settings… sits at the
  bottom of Edit, and the demo song in Help.

## Open for the next pass

- The waveform behind the notes instead of beside them
- C-mod as the default, or a default per ruleset
- A dedicated recording mode with armed lanes, count-in and whole-pass undo
- Test play, which osu! and Quaver have and this draft leaves out
- Dropping selected notes onto a layer row; the [Layers](layers.md) menu and keys move them today
- Two charts side by side, which the column layout would allow
- Plugin timing keys too long for the gutter, such as `osu_mania.scroll_speed`
