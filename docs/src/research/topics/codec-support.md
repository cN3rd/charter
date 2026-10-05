---
type: Open Decision
title: Codec support
description: Which audio formats decode by default, given AAC patent encumbrance.
tags: [topic, stack, audio, legal, open]
status: draft
---

# Codec support

## Observed

`symphonia` decodes FLAC, MP3, AAC, Vorbis, WAV, ISO-MP4 and OGG, selected per-codec through
feature flags. It is MPL-2.0, weak copyleft applied per file.

The surveyed editors accept a narrower set: **ArrowVortex takes Ogg Vorbis, MP3 and WAV**, and
converts other input to Ogg through an external `oggenc2.exe`.

Recorded patent position: AAC is described as encumbered in some jurisdictions; MP3 patents are
described as largely expired.

## Open decision

- Which codecs are enabled by default
- Whether AAC ships behind an off-by-default feature flag
- Whether unsupported input is rejected or converted on import, as ArrowVortex does

This repository currently enables a wide set in `third-party/symphonia-wrap`: aac, adpcm, alac,
mpa, flac, pcm, vorbis, aiff, caf, isomp4, ogg, wav, all-meta and opt-simd. **AAC is currently on.**

## Evidence

Patent status is **asserted**: recorded from research, never legally reviewed. The spike to obtain
an actual legal decision on codec defaults and patent posture was identified and never run.

Note also that MP3 carries a technical cost independent of patents: encoder and decoder start delay
can shift the sample origin by tens of milliseconds. See
[audio timing physics](../constraints/audio-timing-physics.md).

Related: [patents](../constraints/patents.md).
