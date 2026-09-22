---
type: Format
title: .sm
description: StepMania MSD-style tag format with song-level timing and track/row note data.
resource: https://raw.githubusercontent.com/stepmania/stepmania/5_1-new/src/NotesLoaderSM.cpp
tags: [format, stepmania]
status: draft
---

# .sm

MSD-style tag format containing metadata, assets, `#OFFSET`, `#BPMS`, stops and freezes, newer
timing fields, keysounds, attacks, and one or more chart blocks.

Notes are track/row data using the StepMania note symbols: `0` empty, `1` tap, `2` hold head,
`4` roll head, `M` mine, `L` lift, `A` attack, `K` auto-keysound, `F` fake.

Timing is song-level, not per-chart. That distinction is the main reason `.ssc` exists.
