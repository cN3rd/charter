---
type: Editor Profile
title: osu! beatmap editor
description: Stable and lazer beatmap editors, including tap-tempo and waveform comparison for timing.
resource: https://osu.ppy.sh/wiki/en/Client/Beatmap_editor
tags: [editor, osu, prior-art]
status: draft
sources:
  - id: osu-editor
    resource: https://osu.ppy.sh/wiki/en/Client/Beatmap_editor
    title: osu! beatmap editor
  - id: osu-timing
    resource: https://osu.ppy.sh/wiki/en/Client/Beatmap_editor/Timing
    title: osu! editor Timing
  - id: lazer
    resource: https://github.com/ppy/osu
    title: osu!lazer source
---

# osu! beatmap editor

## Stable

Four tabs: Compose, Design, Timing, Song setup.

Compose provides a timeline, beat-snap divisor, object selection and movement, transport,
bookmarks, playback-rate controls and Test. Playback rates listed: 25%, 50%, 75%, 100%.

Timing provides BPM, offset, moving placed notes, slider velocity, tick rate, timing and control
points, a metronome, **tap-tempo via repeated `T` presses** to find initial BPM and offset, and
timing-point copy/paste.

osu!mania Compose has Select, Circle/Note and Hold tools, with holds placed by dragging. The
playfield displays note density, measure, beat and current-time lines, and selection state.

Test mode, statistics, autoplay, AiMod problem checking, bookmarks, low-speed playback, and
per-object and per-timing-point hitsound handling are integrated.

## Lazer

Open-source C#/.NET on osu-framework, with desktop, Android and iOS paths. The mania editor has
Note and Hold tools, beat snapping, and `time|column` selection parsing.

The editor provides cut, copy, paste and clone; test gameplay; an editor clock; a waveform
timeline; beat-divisor controls; zoom and drag seeking; **waveform comparison for timing
alignment**; and undo/redo state tracking.

Lazer is under continuous development, so its behaviour and mania parity are not fixed.
