---
type: Design Note
title: Ruleset plugins
description: How the built-in rulesets read and write documents, and the vocabulary each one owns.
tags: [plugins, rulesets, format, draft]
status: draft
---

# Ruleset plugins

A ruleset plugin is a crate under `plugins/` that turns a `ValidDocument` into the game's typed
model and back. It depends on `charter-core` alone, the surface a third-party plugin would use;
how plugins are loaded is still [open](research/topics/plugins.md).

- Reading fails on anything the game cannot represent, such as a stop in osu!mania, a keysound
  in Tap Tap Revenge, or a key in the ruleset's own namespace that it does not define.
- Data other plugins left behind is skipped, since their keys and `other` events are theirs to
  read.
- Writing produces a validated document holding the ruleset's own data. The typed model is a
  view, so reading and writing back drops other plugins' data.
- Every ruleset keeps the song's markers of the shared kinds, `section` and `bookmark`, and none
  defines a kind of its own. StepMania's `#LABELS` are section markers.
- A song is checked when built, by reading or by `Song::new`, against the ruleset and every
  document rule, and cannot change after. Writing one therefore cannot fail.

Each plugin also exports `Plugin`, which implements `charter-core`'s `RulesetPlugin`: `id` is the
ruleset id its documents carry, and `check` reads a document as a song and keeps only whether
that worked. The editor opens a song for editing only when the plugin for its ruleset accepts it,
and checks every edit, undo and redo through the same plugin, so an edit the game cannot represent
is refused. Without its plugin a song is read-only. `charter-app` compiles the built-in plugins in,
and nothing loads a plugin at runtime yet.

A plugin can also start songs:

- `name` is the game's name.
- `layouts` lists the lane layouts a new song can have, the usual one first.
- `layout_kind` says what the game calls one.

`new_song` makes a song with no notes from metadata, a layout and chart names. Every chart sits on
that layout, at 120 BPM in 4/4. StepMania's layouts are its styles, and a chart whose name spells a
difficulty, such as `Hard`, gets it; others are `edit`, all at meter 1. osu!mania's are its key
counts, 4K first, with HP and OD 5. Tap Tap Revenge's are its 3, 4 and 5 lanes. A plugin without
layouts cannot start songs.

A game whose charts come in standard levels says so with `difficulties`. It holds the levels,
easiest first, alongside presets that each pick some of them and name their charts. A new song's
chart then carries its level by index beside its name, so the name can be what the game calls it
while the level stays known. Tap Tap Revenge's levels are Easy, Medium, Hard and Extreme, kept as a
chart's `difficulty` setting (`easy` to `extreme`). Its presets are the two shapes custom charts
take, standard Extreme alone or every level, and the renamings of two of its games: Tap Tap Reloaded
calls Medium, Hard and Extreme `Metallica`, `Downloads` and `Customs` and has no Easy, and Tap Tap
Hardcore calls the four `Extreme`, `Intense`, `Severe` and `Customs`.

A plugin names the settings it reads as text with `text_settings`: each a key, a name, what it
does, and whether it sits on the song or on each chart. The song panel shows a field for each, the
song's under the song's facts and a chart's under the chart list, and clearing one takes its key
away; every change goes through the plugin's check like any edit. Tap Tap Revenge has one of each,
`theme`: the song's names the theme every level plays in, such as `DeltaHeavyTheme`, and a chart's
names the theme of that level alone. A new song, and each level it starts with, plays in
`TTRDJ`, the game's own theme; a song read without a theme keeps none.

A plugin offers changes for the selected notes with `note_changes`, each a name, what it does and
the function that makes it, which turns one note of a chart so many lanes wide into another, or
leaves it alone. There is no id to match: the plugin hands over the behaviour itself, so a change
the plugin does not offer cannot be asked for. The editor lists them in the notefield's right-click
menu and applies one to the whole selection as one step, checking the result as it checks any edit;
notes a change makes the same merge into one. Tap Tap Revenge offers four:

- Arrow, a shake: left on the first lane, right on the last, down between.
- Slice: left on the left half of the lanes, right on the right.
- Big tap: a mega tap across every lane.
- Tap: back to a tap on the note's first lane.

A plugin can also read another program's timing: `timing_format` names the format, and
`import_timing` turns text in it into a `Timing` for a given song, with a list of what it left
out. Any song can take timing from any plugin's format, so a plugin keeps its own timing events,
such as osu!mania's scroll speeds, only for songs of its game. osu!mania reads osu!'s
`[TimingPoints]`, pasted alone or in a whole `.osu` file.

A plugin can read and write whole songs in other programs' formats: `song_formats` lists them,
each a name and its extensions, and `import_song` and `export_song` convert bytes to a song of its
game and back, each with a list of known conversion losses. The bytes are untrusted. Same-format
fidelity and complete loss reports are requirements, not guarantees of every current adapter.
A format may name `companions`, files it keeps beside the song's, which the host reads and the
import is handed by name. An export may name its main file, make files beside it and have the
song's audio and artwork copied in under names of its own, as a format that keeps a song as a
folder does; the editor passes it how long the audio lasts, when the audio has loaded. These
names use `FileName` in `SongFormat::companions`, `ExportedSong::file_name`, `beside` and `copies`,
and as keys handed to `import_song`. It refuses empty names, `.` or `..`, separators, NUL and `:`,
so the host joins only one component onto the chosen folder. Formats may share an extension, as
one file read the ways different games play it, and the editor asks which game a file is for.

Tap Tap Revenge reads and writes the game's [`.ttr2_track`](research/formats/ttr2_track.md) and
the Editor on Fire MIDI files MidiToTTR converts, both through one model of the game's files: each
level's notes switched on and off at moments of the audio. What a key means is the theme's choice,
so each format is offered once for each game:

| Format | 61 and 63 | 65 |
|---|---|---|
| Tap Tap Revenge track | Left and right shakes | Down shake |
| Tap Tap Revenge: Tour track | Left and right slices | Mega tap, the big note |
| Tap Tap Revenge track, custom tap theme | Taps in the left and right columns | Tap in the middle column |
| Editor on Fire MIDI | Left and right shakes | Down shake |
| Tap Tap Reloaded MIDI | None | Mega tap, on the level's key 4 |

A track export writes `taptrack.ttr2_track` directly in the selected file's parent folder, ignoring
its selected filename, and an `info.plist` beside it with the title, artist, genre, levels, their
columns and themes and how long the audio lasts. The audio goes in as `audio.m4a`, AAC in an M4A file as the
game's own tracks have it, with PNG artwork as `artwork.png` and `artwork@2x.png`. Audio already in
AAC is copied; any other is converted with ffmpeg, which must then be installed and on the `PATH`,
and the export says so when it is not. The host does not create a named `.track` folder and can
replace files under the fixed export names, so the user must choose a file inside a separate,
empty track folder. Imported back with that `info.plist`, the supported game data returns,
subject to conversion losses. A track's import takes the title, artist, genre and themes from
its `info.plist` too, as the game shows them. A theme dependency uses the fixed `Themes/` folder;
export refuses a theme whose `{theme}.ttrTheme` file name has a separator.

A level becomes a chart of its difficulty with the columns `info.plist` gives it, three without
one. Three lanes take keys 60, 62 and 64 as taps, or holds when held over 0.15 seconds, and 61, 63
and 65 as the table says; four or five take keys from 60 up, left to right. Notes landing on a
lane and tick a note of the same kind already takes are dropped, as are levels other than the four
and keys a level has no place for, and tempo changes after the last note. Export writes a kind the
chosen game plays as another, such as a slice in a Tap Tap Revenge track, as that one. It leaves
out, saying so, charts of no level, shakes and the like in four or five lanes, markers, the
preview and artwork that is not PNG. In MIDI it also leaves out the artist, genre, artwork and
themes and levels of four or five lanes, which this adapter's MIDI layout does not represent.

Layers are merged, and their names, colours, visibility and locks are dropped without a loss
entry. This reporting gap is described in [the layers design](layers.md#in-the-file); a game
export is not a complete backup of the charter document.

`plist` reads and writes the property list and `midly` the MIDI file; the plugin walks the
archive's fixed shape itself, since the keyed-archive crates decode by unbounded recursion and
none writes custom classes. A MIDI file has one tempo map, the first
level's, and starts at the audio's start, so export leads in with a tempo that puts the song's
first beat on a whole MIDI beat; imported back, the song's grid starts at the audio's start.

| Ruleset | Namespace | Kinds | Timing | Chart settings |
|---|---|---|---|---|
| StepMania | `sm` | `tap`, `hold`, `roll`, `mine`, `lift`, `fake`, `sm.auto_keysound` | Anchors, tempo, every core event kind, `sm.tickcount`, `sm.combo`, `sm.speed`, `sm.scroll`, `sm.fake` | `style`, `difficulty`, `meter`, `description` |
| Tap Tap Revenge | `ttr` | `tap`, `hold`, `ttr.shake`, `ttr.slice`, `ttr.mega_tap` | Anchors, tempo, `time_signature` | `difficulty` and `theme`, optional |
| osu!mania | `osu_mania` | `tap`, `hold` | Anchors, tempo, `time_signature` over 4, `osu_mania.scroll_speed` | `hp`, `od` |

Each ruleset fixes lanes its own way:

- StepMania takes the style's column count from its own steps-type table. Routine styles, where
  two players share the panels, get a full set per player, player 1 first.
- A Tap Tap Revenge chart has 3, 4 or 5 lanes, and the charts of one song may differ, as the
  game's `columnCountPerLevel` lets each level have its own. A shake or slice sits in the lane
  of its direction: left is lane 0 and right the last lane, and a down shake takes the middle
  lane, or both middle lanes with four. Slices go left or right only. A mega tap covers every
  lane. Shakes, slices and mega taps are gestures that take no column, so each may share its
  lane with a tap or hold at one instant, as the format leaves a ruleset's own kinds to it; two of
  them on one lane at once are refused. Notes scroll at a constant speed, so a tempo change moves
  only the beat grid.
- osu!mania takes the key count: 1 to 10, or an even count up to 20, which osu!lazer splits
  across two stages.

Each plugin's vocabulary is an enum in Rust and text only in the document: StepMania's styles and
difficulties, Tap Tap Revenge's lane counts, levels and note kinds, and osu!mania's key counts.
