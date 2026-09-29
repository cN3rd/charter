---
type: Concept
title: Project overview
description: Current shape of Charter, its crate boundaries, and the user-visible editor state.
status: stable
---

# Project overview

Charter is a rhythm-game chart editor with a game-neutral core. The shared model lives in
`charter-core`; game-specific note rules live in plugins for StepMania, TTR and osu!mania.

The editor state is split from drawing. `charter-editor` owns songs, cursor movement, selection,
undo/redo, markers, layer edits and timing changes. `charter-ui` draws the egui window, panels and
notefield, while `charter-app` supplies disk dialogs, settings storage and audio playback.

Audio decoding is handled by `charter-audio` through `symphonia-wrap`. Playback is in `charter-app`
with kira, including rate changes that affect audio speed and pitch without changing the chart.
Automatic timing is split between `charter-autotiming`, which turns beat/downbeat observations into
a tempo map, and `charter-beatthis-burn`, which runs the Beat This! model on the CPU.

The timing window lives in `charter-timing`: rows and anchors map beats to the audio, and edits are
returned as editor commands instead of bypassing the song model.
