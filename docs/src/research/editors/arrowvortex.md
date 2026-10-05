---
type: Editor Profile
title: ArrowVortex
description: Windows StepMania/ITG simfile editor with the densest documented feature set among the surveyed tools.
resource: https://arrowvortex.ddrnl.com/overview.html
tags: [editor, stepmania, prior-art]
status: draft
sources:
  - id: av-overview
    resource: https://arrowvortex.ddrnl.com/overview.html
    title: ArrowVortex overview
  - id: av-readme
    resource: https://raw.githubusercontent.com/uvcat7/ArrowVortex/beta/README.md
    title: ArrowVortex README
  - id: av-src
    resource: https://github.com/uvcat7/ArrowVortex
    title: ArrowVortex source
---

# ArrowVortex

C++ on OpenGL and SDL3, vcpkg-managed, depending on freetype, iir1, aubio, ffmpeg, libmad, libogg
and libvorbis. Windows only. The 2025 continuation is open source.

## Formats

Loads and saves `.sm`, `.ssc`, `.osu`, `.osz`. Loads `.dwi` with no saver found in source. Audio:
Ogg Vorbis, MP3, WAV. Ogg conversion now shells out to an external `oggenc2.exe`.

Styles: dance single, double, couple, routine, solo; pump single, halfdouble, double, couple.

The SM loader warns that keysounds are unsupported and ignored, while the saver can still write
`#KEYSOUNDS`. A same-format round trip through ArrowVortex is therefore lossy.

## Note model

`NOTE_STEP_OR_HOLD`, `NOTE_ROLL`, mines, fakes, lifts. A `NOTE_STEP_OR_HOLD` with
`endrow == row` is a tap; `endrow > row` is a hold.

## Entry

Digits `1`-`0` map to columns 0-9; Alt offsets into the second side for doubles. Holding a column
key while moving the cursor grows a ghost note, committed on release. Shift plus a digit places a
mine. With auto-jump enabled, release advances the cursor to the next snap.

Snaps: 4th, 8th, 12th, 16th, 24th, 32nd, 48th, 64th, 96th, 192nd, none, custom. Custom snaps take
4 through 192 and regularize to built-ins where possible.

## Selection and clipboard

Select by quantization, by note type, by density, before or after cursor, cursor-to-cursor region,
and box marquee. Shift adds to a marquee, Alt subtracts, right-click clears.

Cut, copy, paste, delete on `Ctrl+X/C/V`. `Shift+Ctrl+V` pastes non-destructively. Both row-based
and time-based copy exist: "Enable time-based copy" encodes notes against timing data and decodes
at cursor time. With nothing selected, copy captures the cursor timestamp and paste jumps to it.

## Conversions and transforms

Notes to and from mines, fakes and lifts; fakes and lifts to notes; holds to and from rolls; holds
to steps or mines; switch player; switch sides.

Transforms: horizontal mirror, vertical mirror, full mirror, expand `2:1` `3:2` `4:3`, compress
`1:2` `2:3` `3:4`, and `Requantize`. Mirrors use style-specific lane tables.

No turn, shuffle, rotate or generic transpose action appears in the cited menu source.

## Sync and tempo

`Adjust Sync`: music offset from -100 to 100 s at 0.001 s resolution, initial BPM, tweak buttons,
and four controls moving the first beat by half or one beat. `Adjust Tempo`: BPM changes, halve and
double BPM, stops, and region-to-stop conversion. SM5 controls cover delays, warps, time
signatures, tick counts, combo multipliers, speeds, scrolls, labels and attacks.

Visual sync anchors on the cursor or the receptors and shifts a row to a target time by changing
offset or BPM boundaries.

## Automatic detection

Analyses a selected region or the first 600 seconds, lists candidates with normalized fitness, and
requires the user to apply one. Onsets come from aubio's `complex` detector at 256 window and 1024
buffer. BPM candidates span 89 to 205. Source comments report the correct BPM within the top three
for 300 cases.

Stream generation takes a region, spacing, maximum single and paired column repetition, difficulty
and starting foot.

## View and history

Waveform with color schemes, luminance, shape, anti-aliasing, high and low-pass filtering, and a
filter overlay. Playback speed clamps to 10%-400%; Space toggles. Minimap has Notes and Density
modes. Navigation covers zoom, snap and continuous scrolling, reverse scroll, C-mod (time-based)
and X-mod (row-based) view modes, and jumps to beat, measure, stream start and end, selection start
and end, and chart start and end.

Undo and redo use a `History` system with chained multi-object operations and no cited depth limit.
Delete removes selected notes and tempo boxes atomically.
