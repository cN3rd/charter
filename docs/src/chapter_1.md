---
type: Concept
title: Project overview
description: Current shape of Charter, its crate boundaries, and where each part of the editor lives.
status: stable
---

# Project overview

Charter is a rhythm-game chart editor with a game-neutral core. The shared model lives in
`charter-core`; each game's notes and rules live in a ruleset plugin under `plugins/`, for
StepMania, Tap Tap Revenge and osu!mania. A plugin also reads and writes its game's files, as Tap
Tap Revenge does `.ttr2_track` and Editor on Fire MIDI.

The editor state is split from drawing. `charter-editor` owns songs, cursor movement, selection,
undo and redo, markers, layer edits and timing changes. The egui window, panels and notefield are
drawn by `charter-ui`, which asks its host for files and dialogs. The host is `charter-app`: it
registers the plugins, does the file I/O, plays audio with kira, converts export audio with ffmpeg,
and reports crashes.

Audio decoding is handled by `charter-audio` through `symphonia-wrap`. Playback rate changes the
audio's speed and keeps or follows its pitch without changing the chart.

Automatic timing has two trackers. `charter-beatthis-burn` runs the Beat This! model on the CPU,
and `charter-autotiming` turns its beats and downbeats into a tempo map.
`charter-mapperatorinator-burn` is a port of Mapperatorinator's super timing, which finds osu!
timing points directly on the graphics card, behind an API of its own, so the rest of the
workspace does not depend on burn.

The timing window lives in `charter-timing`: rows and anchors map beats to the audio, and edits are
returned as editor commands instead of bypassing the song model.
