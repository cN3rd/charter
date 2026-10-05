---
type: Editor Profile
title: Tempora
description: A shipped sync-only tool that aligns audio to a metronome and authors no notes; its source is licence-restricted.
resource: https://github.com/teamkongehund/Tempora
tags: [prior-art, sync, timing]
status: draft
sources:
  - id: tempora
    resource: https://raw.githubusercontent.com/teamkongehund/Tempora/master/README.md
    title: Tempora README
  - id: tempora-release
    resource: https://github.com/teamkongehund/Tempora/releases/tag/v1.5.0-beta
    title: Tempora v1.5.0-beta
  - id: cc-by-nc-nd
    resource: https://creativecommons.org/licenses/by-nc-nd/4.0/deed.en
    title: Creative Commons Attribution-NonCommercial-NoDerivatives 4.0 deed
---

# Tempora

Not a chart editor. It aligns recorded audio to a metronome and leaves the audio unaltered; its
output is timing data packaged with audio for continued work elsewhere.

C# on Godot 4 (`Godot.NET.Sdk/4.4.1`) and .NET 8, using `NAudio`, `NAudio.Vorbis` and
`Spectrogram`. Released as `v1.5.0-beta` with Windows, macOS and Linux builds, spectrogram view,
Beat Saber export, autosave and custom osu export settings.

Unknown: who wrote it, whether `v1.5.0-beta` became stable, and whether it is still maintained.

Its source is [CC BY-NC-ND 4.0](https://creativecommons.org/licenses/by-nc-nd/4.0/deed.en):
attributed, noncommercial verbatim sharing is allowed, but distributing adaptations is not.
Adapted source cannot be redistributed as charter's own MIT/Apache-licensed code. Observations
of its behaviour remain usable. See [licensing](../constraints/licensing.md).

## Timing model

`TimingPoint` couples an audio `Offset` in seconds to a musical `MeasurePosition`, with
`MeasuresPerSecond`/`Bpm` describing the segment.

`MeasurePosition` is floating point: the integer part is the measure, the fraction the
intra-measure position, so `0.75` in 4/4 is the fourth quarter-note of measure 0.

Points live in a `List<TimingPoint>` sorted by `Offset`. Insertion recalculates
measures-per-second from neighbours and rejects incompatible offsets. Segment slope is derived:
`MPS = dMeasure / dOffset` where a next point exists.

Conversion is `(time - Offset) * MeasuresPerSecond + MeasurePosition`, and its inverse
`Offset + ((MeasurePosition - point.MeasurePosition) / MeasuresPerSecond)`.

Both music positions and audio offsets are **monotonic**: points cannot cross neighbours in either
coordinate. Invalid edits revert and flash the blocking neighbour.

Time signatures are separate `TimeSignaturePoint` data keyed by measure, defaulting to 4/4. A
meter change can shift downstream musical positions to preserve BPM and beat count.

`MeasuresPerSecond` must be positive, so the model cannot represent warps or negative-BPM timing.

## Interaction

One measure per waveform row, with adjustable `Rows`, row `Offset` and `Overlap`, vertical
division lines, and waveform and spectrogram rendered per timing segment.

| Gesture | Effect |
|---|---|
| Click | Add a snapped anchor |
| Double-click | Delete anchor |
| Drag | Change music position |
| Ctrl-drag | Change audio offset |
| Shift-drag | Change BPM |
| Alt-drag | Multi-select |
| Ctrl-wheel | Nudge nearest point by `0.002 s` |
| Shift-Ctrl-wheel | Nudge by `0.01 s` |
| Alt-wheel | BPM by integer steps; Shift +/-5; Ctrl +/-0.1 |
| Right-click | Play locally |
| Space | Stop |

Selection is contiguous, held as `[first,last]` indices. The metronome follows the grid division.
Waveform peaks are precomputed min/max per 10 samples, and render nodes are reused rather than
recreated.

## Export

osu export writes a skeleton v14 `.osu` with `[TimingPoints]` and no hit objects, packaged as
`.osz`. Each point is uninherited with offset in milliseconds, milliseconds per beat, meter, sample
set `2`, volume `80`, uninherited `1`, effects `8` where barlines are omitted, and a default
`ExportOffsetMs` of `-14`.

Export **actively transforms timing**: it may remove no-op points, add downbeat or quarter-note
points, reset unsupported signatures, and adjust BPMs for millisecond rounding and barline
behaviour. Beat Saber export emits `StartIndex`, `EndIndex`, `StartBeat`, `EndBeat` and `Bpm`, and
may insert a point at beat 0 because Beat Saber has no equivalent offset.

See [export loss reporting](../topics/export-loss-reporting.md).

`.tmpr` project lines are `Time;MeasurePosition;TimeSignatureUpper;TimeSignatureLower`, with the
final point also storing `MeasuresPerSecond`.
