---
type: Constraint
title: Codec patents
description: Recorded patent encumbrance affecting which audio codecs can ship enabled by default.
tags: [constraints, legal, audio]
status: draft
---

# Codec patents

Recorded, not legally determined:

- **AAC** is described as patent-encumbered in some jurisdictions.
- **MP3** patents are described as largely expired.

A spike to obtain an actual legal decision on codec defaults and patent posture was identified and
never run.

`symphonia` decodes FLAC, MP3, AAC, Vorbis, WAV, ISO-MP4 and OGG, and its features are selected
per-codec, so this is a feature-flag question rather than a dependency question. See
[codec support](../topics/codec-support.md).
