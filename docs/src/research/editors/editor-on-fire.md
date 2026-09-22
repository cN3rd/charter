---
type: Editor Profile
title: Editor on Fire
description: Guitar, drum and vocal chart editor with the widest note domain of the surveyed tools, and real-time strum recording.
resource: https://github.com/raynebc/editor-on-fire
tags: [editor, guitar-hero, rocksmith, prior-art]
status: draft
---

# Editor on Fire

## Entry modes

Real-time "Guitar Tap/Strum" recording during playback, alongside Classic, Piano Roll and Feedback
keyboard-driven step sequencing.

## Note domains

Models GH/RB legacy, vocal and Rocksmith Pro Guitar domains **separately**, rather than through one
note type. Supports HOPO, tap, open notes, sustains, sliders and disjointed notes, drum cymbal and
tom distinctions, phrases, vocals and lyric lines.

Domain constants: `EOF_LEGACY_TRACK`, `EOF_VOCAL_TRACK`, `EOF_PRO_GUITAR_TRACK`. Flags include
`EOF_NOTE_FLAG_F_HOPO`, `EOF_PRO_GUITAR_NOTE_FLAG_TAP`, `EOF_NOTE_EFLAG_DISJOINTED`. Recorded
vocal pitch range: MIDI 36-84.

## Formats

Parses and writes `.chart`, `.mid` and `.sm`. **Unknown MIDI tracks can be retained in a raw linked
list and written back on export** — a concrete precedent for lossless passthrough of data the
editor does not understand.

## Timing and playback

Beat markers, literal float beat times, anchors for denominator-changing time signatures, and
external Sonic Visualiser / Vamp beat annotations.

Multi-stem playback with mute and solo, multi-pitch metronome and claps, and playback-speed
increments of 10% or 25% with Ctrl.

## Recorded caution

EOF mixes highlight and sort flags into note state. Earlier research treated this as an argument
for keeping editor state out of the domain model; see
[domain model purity](../topics/domain-model-purity.md).
