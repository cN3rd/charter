---
type: Format
title: .ttr2_track
description: Tap Tap Revenge's track file, a keyed archive of per-level MIDI-like note events timed in seconds, and the EOF MIDI layout MidiToTTR converts from.
tags: [format, ttr, midi, eof]
status: draft
sources:
  - id: midi-to-taptrack
    resource: https://github.com/Arktisfox/MidiToTaptrack
    title: MidiToTaptrack (MidiToTTR and TTRToMidi)
---

# .ttr2_track

The chart of a Tap Tap Revenge track. It sits in a `.track` folder beside `info.plist`, which
holds the title, levels, theme and column counts, and the song's `audio.m4a` and `artwork.png`.

## Shape

A property list, binary in the game's own tracks and XML in many custom ones, holding an
`NSKeyedArchiver` archive. XML writes each reference as a dictionary holding only `CF$UID`, and a
binary list as a UID. The root is an `NSMutableDictionary` from each level's number to a
`KBMidiFile`:

| Level | Name |
|---|---|
| 0 | Kids, not seen in any file examined |
| 1 to 4 | Easy, Medium, Hard, Extreme |

A `KBMidiFile` has `title`, `copyrightInfo` (the artist), `timeDivision`, `exactTempo`, `trackID`,
`rawData` (the source MIDI file in the game's own tracks, empty in custom ones), `tempoChanges`
(`KBMidiTempoMap`s of `startTime` in seconds and `tempo` in BPM) and `tracks`. The first track
holds text events (`TITLE:`, `ARTIST:` and `ID:`, with or without a space after the colon), the
second the notes. A `KBMidiEvent` has `type` (0 note-on, 1 note-off, 2 text), `note`, `velocity`
(0 on a note-on also switches it off), `channel`, `time` in seconds and `timeInQuarterNotes`.

The game plays a note at its `time`, so the tempo only draws the beat grid.

## Notes

Every level examined, the game's and custom ones, has three columns (`columnCountPerLevel` in
`info.plist`), and uses six keys:

| Key | Note |
|---|---|
| 60, 62, 64 | A tap in the left, middle or right column |
| 61, 63 | A shake left or right |
| 65 | A shake down |

A note held longer than the game's `tapAndHoldThresholdDuration` is a hold. No shipped file or
theme states that value; MidiToTTR's TTRToMidi uses 0.3 seconds, and so does charter. How four or
five columns, slices and mega taps are written was not found: the game maps notes through
`-[TTRGame mapNoteToFunction:]` using objects and Lua theme scripts, which a static read does not
settle. Charter leaves them out of exports and says so.

Some shipped charts hold the same key twice at one moment; charter keeps one.

## EOF MIDI

MidiToTTR converts an Editor on Fire MIDI file: `PART GUITAR`'s Easy keys 60 to 65 are the six keys
above, and each level up is twelve keys higher, as Rock Band's difficulties are. EOF's lanes 1 to 6
are therefore the left tap, left shake, middle tap, right shake, right tap and down shake. A tick's
time counts from the start of the audio; EOF's MIDI delay is not applied.

## Observed tool behaviour

- MidiToTTR's keyed archiver identifies objects by `RuntimeHelpers.GetHashCode`, which is not
  unique, so two events can be written as one: converting a 1,480-note level once lost a note-on
  and repeated a note-off in its place.
- MidiToTTR reads a title only after `TITLE: ` with a space, so tracks writing `TITLE:sudo` read
  without one.
