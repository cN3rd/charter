---
type: Open Decision
title: Sync workstation
description: Whether manual audio sync gets a dedicated view or lives inside the charting view.
tags: [topic, ux, sync, open]
status: draft
---

# Sync workstation

## Observed

**Tempora is an entire shipped application for this one job.** It renders one measure per waveform
row with adjustable row count, row offset and overlap, and draws waveform and spectrogram per
timing segment. It offers click-to-anchor, drag for music position, Ctrl-drag for audio offset,
Shift-drag for BPM, right-click local playback, and a metronome following the grid division.

That layout is fundamentally different from a charting view: measures stacked as rows rather than a
single scrolling lane.

**ArrowVortex** instead keeps sync inside the editor, as Adjust Sync and Adjust Tempo dialogs plus
a visual-sync command that shifts a row to a target time.

**osu!lazer** offers waveform comparison for timing alignment within its editor.

## Open decision

- Sync as dialogs over the charting view, as ArrowVortex does
- A dedicated sync view with its own layout, as Tempora does
- Tempora-style measure-per-row as an optional lens within one editor

## Evidence

Tempora's existence and scope demonstrate that the workflow is substantial enough to justify a
product. Earlier research concluded from this that manual sync "can be treated as a first-class
workstation" rather than a dialog, grade **inferred**, with no comparative usability data.

Note the direction of the evidence: a tool existing proves the work exists, not that it needs a
separate view inside an editor that also charts.

## Current choice

charter takes the third option: a timing window beside the notefield, opening on rows of audio
time and switching to rows of bars with V. It edits the [anchors](sync-anchors.md) stored in the
chart's timing; segment tempos follow from those anchors, with an explicit tail tempo.
See the [user interface](../../ui.md) chapter.
