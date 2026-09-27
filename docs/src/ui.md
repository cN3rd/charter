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
selection; undo and redo return to where each edit was made. The song panel's Timing section is laid out as osu!'s timing panel is.
The song's offset comes first, on its own: where beat 0 falls in the audio, in milliseconds,
stepped by 1 ms with its buttons or Alt+Left and Alt+Right, 10 ms with Shift, beside a button that
opens the timing window. Below it every timing point is listed, a tick's tempo, meter, stops and
game events together as one point, with the point in effect at the cursor highlighted; clicking a
point moves the cursor to it. Under the list are that point's BPM, with − and + buttons, and its
meter, such as 3/4 or 7/8, each showing what is in effect there and setting it on the point, and a
button that removes the point, except the one that starts the tempo. "Add a timing point here"
starts one at the cursor at the tempo it has there, and says why it cannot when the cursor is on
one already. F7 and F8 still nudge the tempo in effect at the cursor. A value typed
into a field applies on Enter or when the field is left; Escape leaves it as it was. Below it the song's markers are
listed; clicking one jumps the cursor there, a name typed and entered names the section at the
cursor or starts one, and any marker can be removed, each as one step. Songs open and save through
native file dialogs, unsaved changes are asked about before they are lost and autosaved, half a
minute after they are made unless the settings say otherwise, and a settings window holds what the
user chooses, including which way the notefield runs and the tick on each note and the metronome on
each beat, which play over the audio and which F3 and F4 turn on and off. The music, the note ticks and
the metronome each have a volume, set on the Sounds page or from the toolbar's speaker; each
turns down only its own sound. With no clicks on, the music's volume changes as it plays; clicks
are mixed into the audio before it plays, so with them on, a volume changed while playing plays
on from where it was once the slider is let go. The metronome accents the
first beat of each measure and has nine sounds to pick from, each with a preview. They are
synthesised click pairs dedicated to the public domain, bundled under
`crates/charter-audio/assets/clicks` with their CC0 licence and a provenance note on how they
were made. A new song takes its game,
the layout every chart shares, its title, artist and chart names from a dialog, and asks for audio
until an audio file is dropped on the window or chosen from the banner that asks for it or from
File > Choose audio…. Under Wayland nothing can be dropped, since winit takes no drops there yet
([winit#1881](https://github.com/rust-windowing/winit/issues/1881)), so the window offers only
choosing and says why; saving
copies that file beside the song. Saving a song somewhere new makes it a project: the name chosen
becomes a folder holding the song's JSON file under the same name and a copy of its audio, where
anything worked out from the audio goes. Audio in a relative subfolder keeps that subfolder;
different audio already at the destination, or a failed copy, refuses the save without changing
the open song or its JSON. The waveform's peaks are the first such file,
`cache/<audio>.peaks`, keeping any relative audio subfolder: the finest of the min and max levels
every view reads, so the notefield,
the timing window and any view to come draw from the same data at any zoom. The file names the
audio by a fingerprint of its bytes, so a replaced audio file gets its peaks worked out afresh,
and a damaged or outdated file is ignored; deleting the folder only costs the time to work them
out again. With the peaks kept, the waveform shows as soon as a song opens, before its audio has
decoded. Nothing is written for audio outside the project folder. Choosing the song's own file in its folder
saves over it, and a folder that already holds a song under that name is left alone. Space plays the song's audio from the cursor and pauses it, the
cursor follows the playback, and moving the cursor while playing seeks. A lane key pressed while
playing places its note on the snap line nearest the audio and leaves the playback where it is;
held past 200 ms, the tap grows into a hold with the playback, ending on the snap line nearest
where the key comes up, all one step. Shift+Left and Shift+Right,
or the Playback menu, play slower or faster from 10% to 400%, which changes the audio's pitch with
its speed and leaves the chart alone. The status bar says whether the audio is ready, loading,
missing or unreadable, and every message has a Dismiss button. The Edit, Timing and Playback menus
carry the selection, tempo, marker and playback commands with their keys, F1 lists every key by
group with a filter, and saving, opening, settings and help keep working while a text field has the
keyboard. With no song open the centre offers Open song…, New song and the demo. The look follows
pro audio and creative tools: a transport toolbar under the menus with a large position readout and
grouped tempo, snap, speed and zoom controls, collapsible sections in the side panels, the notefield
in a dark well, one blue accent for selection and primary actions, Inter for text and Phosphor for
icons, bundled under `crates/charter-style/assets/fonts` with Inter's OFL licence, and the system's
Japanese, Chinese and Korean fonts after them, whatever the locale, so titles in any of those
scripts draw; the app finds them with the `system-fonts` crate and reads each file once; messages float over
the canvas rather than moving it. The settings window has a sidebar of pages, Editing, Display,
Sounds and Autosave, each setting with a line on what it does and on and off choices as switches;
the new-song dialog picks the game from a segmented control, sketches the layout's lanes, and takes
the song's audio dropped on it or chosen. An audio file's tags fill in a title and artist still at
their defaults, in the dialog or on a new song; where the user has written their own, charter asks
whether to import them. The song panel folds away with Ctrl+\\, and the tempo section sets the
offset, where tick 0 sits in the audio, for songs that open with silence. Once the audio has loaded,
its waveform draws in a strip between the timing column and the lanes, lined up with the notes. The
Display page's Sixteenth lines setting draws a grid line on every sixteenth, sixteen to a measure
of 4/4, with each beat's line brighter; the metronome keeps to the beats. In
the notefield, the timing events at one tick form a single timing point, as osu! has them: one
line, its values stacked beneath it (`150 BPM`, then `3/4`), and one handle. Timing points drag
along the timing column and markers along the ruler to another snap line, each as one step, with a
guide where they will land; the point holding the song's first BPM change stays put. The cursor's handle on the ruler drags to scrub, and seeks while playing.

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
- There is no inspector panel; the notefield keeps the width. The status bar counts the selection,
  the song panel sets the tempo, offset and sections at the cursor, and what keys cannot express
  on a note, such as a keysound, is still to find a place.
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
  offset or a BPM. Fine and coarse nudges take Tempora's 2 ms and 10 ms.
- The timing window (Timing > Timing window, Ctrl+T), in its own crate, `charter-timing`, follows
  the Beatline specification for the parts in scope. Each row holds half a bar to four bars laid
  out by bar position, a tenth of the bar before shown ahead of each downbeat, so a correct map
  lines beats up down the rows; downbeats draw thick, beats solid, the lines between dashed, or
  dotted for triplets, with beats numbered. The grid is 1/1 to 1/32 of a whole note, keys 1 to 0,
  and restarts on every downbeat. Every BPM change is an anchor pinning a beat to a moment of the
  audio, as in [sync anchors](research/topics/sync-anchors.md); edits rebuild the tempo from
  the anchors, so the others keep their moments. The window measures the map against the onsets
  the host finds in the audio: ticks along each row's foot, a strip under it coloured by how
  early or late each onset near a line falls, a badge on bars more than 5 ms off, and an issue
  list (bars off, segments shorter than a beat, tempo jumps over 3%, anchors more than 2 ms from
  their onset, metre changes without an anchor) that N and Shift+N step through.
  A click adds an anchor, its moment snapped to the strongest onset within 25 ms and its beat to
  the grid, refused within half a grid step of another, and auditioned from a beat before. A drag
  on an anchor moves its beat, Ctrl-drag its moment, Shift finer and unsnapped; a drag elsewhere
  selects the anchors inside. An anchor never passes its neighbours: a drag stops short and the
  neighbour pulses, a refused edit says why in the status line. Arrows nudge 1 ms (Shift 0.1 ms,
  Ctrl 10 ms) and move a grid step, + and − set the tail tempo, Del removes, and a run of nudges
  undoes as one step. The inspector shows the focused anchor's moment, bar position, tempo and
  distance to its onset, each editable, and its segment's fit error and coverage; F fits the
  focused anchor or the selection to the onsets by least squares, Shift+F to half BPMs, shown
  before Enter applies it. D and Shift+D double and halve the tempo. The window takes the keyboard
  after a click in it and gives it back after a click elsewhere; ? lists its keys.
  V, or the toolbar's Layout, switches the rows to audio time: each row one to eight seconds,
  labelled by its start, so the waveform never moves and an edit moves the grid lines and anchors
  over it instead. The bar layout keeps the grid still and slides the audio under it, as Tempora
  and the Beatline specification do; both draw the audio at the times the song's anchors give it.
  To listen, L loops the bar under the cursor and Shift+L the selection, with the metronome; the
  loop's ends follow the map, and a committed edit plays it again from its start so the fix is
  heard at once. R switches to the alternate speed, 60% unless the Sounds page says otherwise,
  and back. C turns the metronome on and off, and it stays muted while Shift+C is held; it can
  follow the window's grid, with the clicks between beats at their own volume. Settings > Sounds
  calibrates latency: a click plays twenty times, the median of how late sixteen taps come is kept,
  and the cursor then shows what is heard rather than what plays; the status line shows it. The
  clicks are mixed into the music before it plays, so they cannot drift from it and need no
  correction of their own, and muting or changing them while playing plays on from where it was.
  Not yet built from the specification: scrub, pan split, count-in, pitch-preserving slow
  playback, the magnifier, spectrogram, locks, the metre dialog, the command palette, the
  overview, beat-stack view, auto-seed, suggested anchors, tap tempo and band solo; selection
  changes are not undo steps.
  The waveform shows here whether or not the notefield's is on.
- Timing > Reset timing… asks first, then puts 120 BPM in 4/4 from 0 ms in place of the open
  chart's timing, as one step: every BPM change, meter, stop and game timing event goes, and the
  notes keep their beats.
- Timing > Import timing… reads another program's timing, pasted or from a file, and shows what
  it holds before it replaces the open chart's tempo, meters and offset as one step. Each ruleset
  plugin may read a format through `RulesetPlugin::import_timing`; osu!mania reads osu!'s
  `[TimingPoints]`. A red line becomes a BPM change on the nearest tick, placed so rounding never
  adds up past half a tick, with a time signature where the meter changes or the red line falls
  inside a measure, since osu! starts a measure at each; the first sets the offset. Green lines
  become `osu_mania.scroll_speed` in an osu!mania song, reset to 1 at each red line as osu! does,
  and are left out of any other game's, which the window says, as it does for volumes, hit sounds
  and kiai.
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
- Unsaved changes are autosaved to one recovery file in charter's own data folder, never beside the
  song, half a minute after the first change since the last autosave; the settings window sets the
  wait, from 5 seconds to 10 minutes, or turns autosave off, which stops new autosaves and leaves
  one already kept for the next start. Saving the song, undoing back to it or throwing the changes
  away removes the file. On the next start a recovery file left behind is offered for restoring: the
  song comes back unsaved, for the file it belongs to. Saves write beside the song and move over it,
  without forcing the write to disk, since some filesystems refuse that; the autosave is the cover
  for a crash.

## Open for the next pass

- The waveform behind the notes instead of beside them
- C-mod as the default, or a default per ruleset
- Tempora's plain drag of an anchor's musical position, and its multi-select, in the timing window
- Test play, which osu! and Quaver have and this draft leaves out
- Two charts side by side, which the column layout would allow
- Plugin timing keys too long for the gutter, such as `osu_mania.scroll_speed`
