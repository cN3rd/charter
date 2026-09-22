---
type: Format
title: .osu
description: Human-readable sectioned osu! format using millisecond timing and inherited/uninherited control points.
resource: https://osu.ppy.sh/wiki/en/Client/File_formats/osu_%28file_format%29
tags: [format, osu]
status: draft
sources:
  - id: osu-format
    resource: https://osu.ppy.sh/wiki/en/Client/File_formats/osu_%28file_format%29
    title: osu! file format
---

# .osu

Sections: `[General]`, `[Editor]`, `[Metadata]`, `[Difficulty]`, `[Events]`, `[TimingPoints]`,
`[Colours]`, `[HitObjects]`.

`Mode=3` denotes osu!mania. **Timing coordinates are milliseconds**, not rows or beats.

Timing line syntax:

```
time,beatLength,meter,sampleSet,sampleIndex,volume,uninherited,effects
```

A negative beat length denotes inherited control data. Uninherited points set BPM and meter;
inherited points control slider velocity, hitsounds, volume and effects.

In mania, `x` selects the column and hit-object type bit `7` marks a hold.

The format natively represents notes, holds and its control-point model. It does not natively
represent StepMania mines, rolls, lifts, fakes or warps.
