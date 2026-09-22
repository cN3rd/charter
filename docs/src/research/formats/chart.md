---
type: Format
title: .chart
description: Clone Hero and Guitar Hero tick-based format with modifier notes encoded as pseudo-frets.
resource: https://github.com/FireFox2000000/Moonscraper-Chart-Editor
tags: [format, clone-hero]
status: draft
---

# .chart

Sections `[SyncTrack]`, `[Events]`, and instrument sections such as `[ExpertSingle]`, with `ts`,
`b`, `e` and `a` events.

Notes are `N <fret> <length>`. Fret `5` is Forced and fret `6` is Tap, so modifiers arrive as
pseudo-notes at the same tick rather than as flags on a note.

Resolution is configurable and conventionally **192 ticks/beat**.

Moonscraper's parser batches notes and applies flags after parsing, because modifier order within
a tick is not guaranteed. Any parser for this format needs the same two-pass shape.
