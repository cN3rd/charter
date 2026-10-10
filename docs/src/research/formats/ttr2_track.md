---
type: Format
title: .ttr2_track
description: Tap Tap Revenge's track file, a keyed archive of per-level MIDI-like note events timed in seconds, the per-theme key meanings, and the EOF MIDI layouts MidiToTTR converts from.
tags: [format, ttr, midi, eof]
status: draft
sources:
  - id: midi-to-taptrack
    resource: https://github.com/Arktisfox/MidiToTaptrack
    title: MidiToTaptrack (MidiToTTR and TTRToMidi)
---

# .ttr2_track

The chart of a Tap Tap Revenge track. It sits in a `.track` folder beside `info.plist`, which
holds the title, levels, theme and column counts, and the song's `audio.m4a` and artwork.
Charter writes both `artwork.png` and `artwork@2x.png` when the song has PNG or JPEG artwork.

## Shape

A property list, binary in the game's own tracks and XML in many custom ones, holding an
`NSKeyedArchiver` archive. XML writes each reference as a dictionary holding only `CF$UID`, and a
binary list as a UID. The root is an `NSMutableDictionary` from each level's number to a
`KBMidiFile`:

| Level | Name |
|---|---|
| 0 | Kids |
| 1 to 4 | Easy, Medium, Hard, Extreme |

A track of one level may number it apart from `info.plist`: Muse's DLC tracks store theirs under 0
or 4 while `availableDifficultyLevels` offers 3, and the game plays it as `info.plist` says.

A `KBMidiFile` has `title`, `copyrightInfo` (the artist), `timeDivision`, `exactTempo`, `trackID`,
`rawData` (the source MIDI file in the game's own tracks, empty in custom ones), `tempoChanges`
(`KBMidiTempoMap`s of `startTime` in seconds and `tempo` in BPM) and `tracks`. The first track
holds text events (`TITLE:`, `ARTIST:` and `ID:`, with or without a space after the colon), the
second the notes. A `KBMidiEvent` has `type` (0 note-on, 1 note-off, 2 text), `note`, `velocity`
(0 on a note-on also switches it off), `channel`, `time` in seconds and `timeInQuarterNotes`.

The game plays a note at its `time`, so the tempo only draws the beat grid.

## Notes

The game does not give keys fixed meanings: each theme's `theme.cfg`, a Lua script, adds the
columns notes fall in, each claiming a key (`claimedNote`) and, for a shake, a direction
(`claimedNoteTapType` 1 left, 2 up and down, 3 right). `numCols`, from `columnCountPerLevel` in
`info.plist`, picks a layout.

| Columns | Keys |
|---|---|
| 4 or 5 | 60 up, left to right, in every theme examined: official Metallica, the `-4ln` custom themes and Muse |
| 3 | 60, 62 and 64 the left, middle and right columns; 61, 63 and 65 as the theme makes them |

The 3-column side and middle keys fall in four families across the 127 themes examined (official
apps, custom DLC and Tap Tap Revenge 4):

| Family | 61, 63, 65 | Themes |
|---|---|---|
| Shakes | Shakes left, right and up-and-down, gestures that take no column | TTRDJ, the official Metallica and its album themes, the official boss themes |
| Tour | Drawn as slices left and right, and the middle note as a bar across every column, its big note | DeltaHeavy, the one theme with `targetEngineVersion` 5000 and `explicitTapSize` |
| Extra taps | Columns of their own ending where 60, 64 and 62 do, with no hold sprite: taps that cannot be held | MCR, Paramore, BFMV, AfterTheBurial, AllThatRemains, AsILayDying, BDM2, BlackDahliaMurder, Blessthefall, Buckethead, ChildrenOfBodom2, Dragonforce, Megadeth, ScarSymmetry, KillswitchEngage, Warmen, ProtestTheHero, Underoath |
| Taps only | Not claimed, so not played | 30stm, BMTH, Slipknot and 16 others |

Boss themes use four columns: NirvanaBoss and NickelbackBoss 60 to 63, and LGGBoss 60, 62, 64 and
66, beside shakes. Key 59, in one LoveGame level, is claimed by no theme. Tap Tap Reloaded's tracks
(`ttmr-*`) have one level, numbered 1 or 4 whatever `info.plist` says, and one plays its columns an
octave set higher, on 96, 98 and 100.

A note held longer than the theme's `tapAndHoldThresholdDuration` is a hold: 0.15 seconds with the
game's short-holds option on, 1 second otherwise, and the game's own default for TTRDJ and Tour
themes, which set none. Charter uses 0.15 seconds.

A tap and a shake of one side often fall on one moment, 4,183 times across 45 tracks, as the
shake takes no column. Some charts hold the same key twice at one moment, 336 times in 64 tracks.

## EOF MIDI

MidiToTTR converts an Editor on Fire MIDI file: `PART GUITAR`'s Easy keys 60 to 71, Medium twelve
keys higher, then Hard and Extreme, as Rock Band's difficulties are. Its standard layout takes a
level's keys as a track's, so EOF's lanes 1 to 6 are the left tap, left side, middle tap, right
side, right tap and middle note. Its Tap Tap Reloaded layout, for the new game of that name rather
than the old Metallica one, puts the left, middle and right taps on a level's keys 0, 1 and 2 and
the middle note, there the big note, on 4. A tick's time counts from the start of the audio; EOF's
MIDI delay is not applied.

## Observed tool behaviour

- MidiToTTR's keyed archiver identifies objects by `RuntimeHelpers.GetHashCode`, which is not
  unique, so two events can be written as one: converting a 1,480-note level once lost a note-on
  and repeated a note-off in its place.
- MidiToTTR reads a title only after `TITLE: ` with a space, so tracks writing `TITLE:sudo` read
  without one.
