---
type: Guide
title: Settings
description: Every page and setting of the settings window, and how settings are kept between runs.
tags: [guide, settings]
status: draft
---

# Settings

Edit > Settings…, or Ctrl+Comma, opens the settings window. Changes take effect at once. Whatever
you pick is remembered between runs, and Restore defaults, beside the close button, puts
everything back except the latency calibration and the list of recent songs.

The window is modeless. Opening it does not disable chart editing; lane keys can still change
notes behind it when no text field has the keyboard.

## Editing

| Setting | What it does |
|---|---|
| Move on a snap after placing a note | Once the lane keys that placed a note come up, the cursor moves one snap on. |
| Keep notes where they sound when a BPM changes | Off, the notes a changed BPM times stay on their beats and move with the grid. See [Timing](timing.md#offset-and-timing-points). |
| Default snap | The snap a song opens on, a 16th to start. |
| Default zoom | How close the notefield opens. |

## Display

| Setting | What it does |
|---|---|
| Interface size | How large the whole window draws. |
| Notefield | Which way notes travel past the cursor: up, down, left or right. |
| Waveform | The song's audio in a strip beside the lanes. |
| Frequencies | The strip shows how loud each pitch is instead of the waveform. |
| Minimap | The whole chart in a strip beside the notefield. |
| Grid lines | Lines between the beats, down to an 8th, 12th, 16th, 24th, 32nd or 48th note. Where the snap has lines the grid leaves out, the snap's lines draw instead. |

## Sounds

| Setting | What it does |
|---|---|
| Note ticks, Note tick sound | A sound on every note as it passes, and which: a click, a beep, a hit, a clap, a soft clap or a finish. |
| Tuned lanes | Each lane's tick at a pitch of its own. |
| Metronome, Metronome sound | A click on every beat, accented on each measure's first, in one of nine sounds. |
| Metronome follows the grid | In the timing window, a click on every grid line, the ones between beats quieter. |
| Keep pitch at other speeds | Audio played from 10% to 200% of its speed keeps its pitch. |
| Latency | Calibrate… measures the audio and input latency. See [Listening](listening.md#latency). |
| Alternate speed | The speed R switches to in the timing window, 60% to start. |
| Snippet length | How much Q and W play, 200 ms to start. |
| Volumes | The music, the note ticks, the metronome, and the clicks between beats as a share of the metronome's. |

## Autosave

| Setting | What it does |
|---|---|
| Autosave unsaved changes, Wait before saving | Keeps unsaved changes for the next start, 30 seconds after a change to start. |
| Snapshots in a folder, Snapshot folder | Dated copies of a changed song, each song in a folder of its own. |
| Snapshots beside the song | The same copies in an `autosaves` folder beside the song's file. |
| Snapshot every, Snapshots kept | How often a changed song is copied, 5 minutes to start, and how many copies each folder keeps, 20 to start. |

The page also names charter's data folder. [Files and recovery](files.md) explains each of these.

Only what you changed is written, to `app.ron` in that folder. Anything left alone keeps following
its default, so a later version that revises that default will quietly pick the new value up.
