---
type: Guide
title: Timing
description: Setting a song's offset and timing points, the timing window, finding timing from the audio, and importing osu! timing.
tags: [guide, timing, sync]
status: draft
---

# Timing

The grid of beats and measures the notes sit on has to line up with the music. Time a song before
charting it, since notes placed on a wrong grid have to move later.

## Offset and timing points

The **offset** is where beat 0 falls in the audio, in milliseconds. A **timing point** is
everything that changes at one beat: the tempo in BPM, the meter such as 3/4 or 7/8, a stop, and
any timing events the game has of its own, such as osu!mania's scroll speed.

Every tempo point is pinned to its moment of the audio, as Editor on Fire's anchors and osu!'s red
lines are. Setting the BPM of one point changes how many beats fit before the next, and the next
point stays where it sounds; everything after it, notes, events and markers, moves along the grid
with it. The notes the changed BPM times stay where they sound too, while the grid moves under them,
unless the Editing page's "Keep notes where they sound when a BPM changes" is off, which keeps them
on their beats.

## The Timing section

The song panel's Timing section is laid out as osu!'s timing panel is:

- **Offset**, stepped by 1 ms with its buttons or Alt+Left and Alt+Right, 10 ms with Shift.
- **Start BPM**, the first point's tempo, and a button that opens the timing window.
- The list of timing points, each by its moment in the audio, such as `0:26.100`. The point in
  effect at the cursor is highlighted, and a click moves the cursor to it.
- The selected point's time, BPM and meter. A time typed for a tempo point moves it to that moment
  of the audio; for the first point it moves the whole grid, as the offset does. The BPM has - and +
  buttons. A button removes the point, except the one that starts the tempo.
- **Add a timing point here** starts one at the cursor, at the tempo in effect there.

F7 and F8 lower and raise the tempo at the cursor by 1 BPM. In the notefield, a tempo point drags
along the timing column to the moment it is dropped on, keeping its beat; a meter or stop point,
and a marker on the ruler, drag to another snap line.

## The timing window

Timing > Timing window, or Ctrl+T, opens a window made for syncing. It lays the audio out in rows,
each one to eight seconds long, with the beat grid drawn over it: downbeats thick, beats solid, the
lines between dashed. A correct grid lines up with the hits down every row. V switches to rows of
bars, which keep the grid still and slide the audio under it instead.

Each BPM change is an **anchor** pinning a beat to a moment of the audio. The window finds the
onsets in the audio, the starts of hits, and measures the grid against them. It shows ticks along
each row, a strip coloured by how early or late each onset falls, a badge on bars more than 5 ms
off, and a list of issues that N and Shift+N step through.
Early onsets have a negative offset and appear in the strip's top half; late ones have a positive
offset and appear below. The inspector's Δ uses the same sign: onset time minus anchor time, in ms.

- A click adds an anchor, snapped to the strongest onset within 25 ms and to the grid. The grid
  runs from 1/1 to 1/32 of a whole note, chosen with the keys 1 to 0.
- Dragging an anchor moves its beat, and Ctrl+drag its moment; Shift makes either finer and
  unsnapped. Dragging elsewhere selects the anchors between the drag's start and end, even after
  the starting row scrolls away.
- Left and Right nudge the focused anchor by 1 ms, 0.1 ms with Shift and 10 ms with Ctrl; Up and
  Down move it a grid step; J and K focus the anchor before or after. Delete removes it. + and -
  change the last section's tempo by 0.1 BPM, 1 BPM with Shift.
- A pins an anchor to the audio under the playhead.
- F fits the anchor or the selection to the onsets; Shift+F rounds to half BPMs. The fit is shown
  first, and Enter applies it. D and Shift+D double and halve the tempo.
- L loops the bar under the cursor with the metronome, and Shift+L the selection; a fix plays again
  at once. R switches to the alternate speed, 60% unless the settings say otherwise. C turns the
  metronome on and off, and Shift+C mutes it while held.
- ? lists the window's keys.

The window takes the keyboard after a click in it and gives it back after a click elsewhere.

## Finding timing automatically

Timing > Find timing with Beat This!… and Find timing with Mapperatorinator… listen to the song and
put a timing in place of the open chart's, as one undo step. Check what they find in the timing
window before charting on it.

| | Beat This! | Mapperatorinator |
|---|---|---|
| Finds | beats and downbeats, which charter fits a tempo map to | osu! timing points directly |
| Runs on | the processor | a graphics card, or the processor if you agree |
| Download | 81 MB, once | 416 MB, once |

The first time each runs it asks before downloading its model into charter's data folder. The song
itself is never sent anywhere. Charter checks each download against its published SHA-256
checksum before keeping it. The status bar shows the download and the search, and editing goes on
meanwhile. A timing found after another song was opened, or after the song's audio changed, is not
used. Without a graphics card Mapperatorinator asks whether to run on the processor, which can
take over 15 minutes a song. [Automatic timing](../autotiming.md) explains how each works and how
well.

## Importing timing

Timing > Import timing… reads osu!'s `[TimingPoints]`, pasted or from an `.osu` file, and shows
what it holds before it replaces the open chart's tempo, meters and offset as one step. Red lines
become BPM changes, with a meter where osu! starts a measure. Green lines become scroll speeds in an
osu!mania song and are left out of any other; the window says what is left out, such as volumes,
hit sounds and kiai.

Timing > Reset timing… puts 120 BPM in 4/4 from 0 ms in place of the chart's timing, after asking.
The notes keep their beats.
