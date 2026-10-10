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
everything back except the latency calibration, the list of recent songs and whether ffmpeg
installation help was already offered.

The window is modeless. Opening it does not disable chart editing; lane keys can still change
notes behind it when no text field has the keyboard.

## Editing

| Setting | What it does |
|---|---|
| Move on a snap after placing a note | Once the lane keys that placed a note come up, the cursor moves one snap on. |
| Keep notes where they sound when a BPM changes | Off, the notes a changed BPM times stay on their beats and move with the grid. See [Timing](timing.md#offset-and-timing-points). |
| Default snap | The snap a song opens on, a 16th to start. |
| Default zoom | How close the notefield opens, from 12.5% to 1250%. |

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
| Latency | Calibrate… measures the audio and input latency, from -500 to +500 ms. See [Listening](listening.md#latency). |
| Alternate speed | The speed R switches to in the timing window, from 10% to 400%, 60% to start. |
| Snippet length | How much Q and W play, from 50 to 1000 ms, 200 ms to start. |
| Volumes | The music, the note ticks, the metronome, and the clicks between beats as a share of the metronome's, each from 0% to 100%. |
| FFmpeg binary | Path to the ffmpeg executable used when exporting Tap Tap Revenge audio. Leave empty to use the `PATH`, or use Choose FFmpeg to select it. Charter shows the binary it found, or that none runs. Install help appears if none runs. |

When no ffmpeg runs at first start, charter offers installation help once. The Install help
button beside the path remains available whenever ffmpeg is missing. On Linux the dialog shows
a command for an installed package manager and can copy it; otherwise it asks you to install
ffmpeg with your system's package manager. If you install ffmpeg outside the `PATH`, enter its
binary's path here or choose it.

## Autosave

| Setting | What it does |
|---|---|
| Autosave unsaved changes, Wait before saving | Keeps unsaved changes for the next start, after 5 to 600 seconds, 30 to start. |
| Snapshots in a folder, Snapshot folder | Dated copies of a changed song, each song in a folder of its own. |
| Snapshots beside the song | The same copies in an `autosaves` folder beside the song's file. |
| Snapshot every, Snapshots kept | How often a changed song is copied, from 1 to 120 minutes, 5 to start; how many copies each folder keeps, from 1 to 500, 20 to start. |

The page also names charter's data folder. [Files and recovery](files.md) explains each of these.

Only what you changed is written, to `app.ron` in that folder. Anything left alone keeps following
its default, so a later version that revises that default will quietly pick the new value up.
Saved numbers outside a setting's range are brought to the nearest end when read. A setting that
cannot be read takes its default without losing the other choices.
