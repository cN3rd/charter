---
type: Editor Profile
title: Quaver
description: Open-source VSRG whose in-game editor treats live mapping and scroll velocity as first-class.
resource: https://github.com/Quaver/Quaver
tags: [editor, quaver, prior-art, live-recording]
status: draft
sources:
  - id: quaver
    resource: https://github.com/Quaver/Quaver
    title: Quaver source
  - id: qua
    resource: https://raw.githubusercontent.com/Quaver/Quaver.API/master/Quaver.API/Maps/Qua.cs
    title: Qua.cs
  - id: input-mgr
    resource: https://raw.githubusercontent.com/Quaver/Quaver/develop/Quaver.Shared/Screens/Edit/Input/EditorInputManager.cs
    title: EditorInputManager.cs
---

# Quaver

Open-source C#/.NET 8 on MonoGame/XNA. Windows, macOS, Linux; distributed on Steam.

## Editing

Places, moves, removes, resizes, resnaps, reverses, flips and swaps lanes. Handles timing points,
scroll velocities, scroll-speed factors, timing groups, layers and bookmarks. Timeline seek and
zoom, waveform, spectrogram, and test play with mod selection via `F4`.

Automatic BPM and offset detection. Mines, long notes, and per-object keysounds.

## Live mapping

First-class, not an afterthought: `EditorLiveMapping`, lane actions `PlaceNoteAtLane1..10`,
`EditorLiveMapOffset`, `EditorLiveMapSnap`, and long-note-on-hold behaviour. While audio plays it
offsets `Track.Time` and snaps to the nearest tick before placing.

Input offset range in this prior art: **-200 to +200 ms**.

Lane bindings in this style are home-row rather than numeric: 4K `D F J K`, 7K `S D F Space J K L`.

See [live recording](../topics/live-recording.md) and
[input latency calibration](../topics/input-latency-calibration.md).
