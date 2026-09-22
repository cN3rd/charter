---
type: Editor Profile
title: Moonscraper
description: Unity-based Clone Hero chart editor, notable for its tick-indexed data model and view-restoring undo.
resource: https://github.com/FireFox2000000/Moonscraper-Chart-Editor
tags: [editor, clone-hero, prior-art, data-model]
status: draft
sources:
  - id: moonscraper
    resource: https://github.com/FireFox2000000/Moonscraper-Chart-Editor
    title: Moonscraper Chart Editor
---

# Moonscraper

Unity, targeting Guitar Hero, Clone Hero and Rock Band. Source-build Unity version
`2018.4.23f1`; an older bundled manual says `5.5.5f1`.

## Features

Local and global views, waveform, BPM, time-signature, section and event tools, click and drag
notes, sustains, numeric snapping, open notes, preview, controller and keyboard playtest, `.chart`
saving, `.mid` export, resolution changes, and Clone Hero packaging.

## Data model

`SongObject`s hold absolute `uint tick` positions in sorted flat lists. Configurable resolution
defaults to the `.chart` convention, typically **192 ticks/beat**.

BPM values are stored as **BPM x 1000** in a `uint`. Each BPM's assigned time is cached through an
iterative `UpdateBPMTimeValues()` pass.

Notes carry lane, sustain length, and flags including `Forced` and `Tap`. Same-tick notes are
linked into chords.

## Undo

`CommandStack` stores edit commands alongside camera and selection snapshots, so undo restores the
view as well as the data. Copy and paste compute relative tick bounds and shift ticks.

## Rendering

Timeline objects are pooled. Only the tick range between camera bounds is queried, by binary
search. During playback, notes behind the strikeline are hidden visually while remaining in the
data.

See [row resolution](../topics/row-resolution.md) and [undo model](../topics/undo-model.md).
