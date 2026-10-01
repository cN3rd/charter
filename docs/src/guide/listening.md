---
type: Guide
title: Listening
description: Playing the song, short snippets around the cursor, playback speed, the metronome and note ticks, the waveform, and latency calibration.
tags: [guide, playback, audio]
status: draft
---

# Listening

## Playing

Space plays the song from the cursor and pauses it. The cursor follows the playback, and moving the
cursor while playing jumps the playback there.

Q plays a moment just before the cursor and W a moment just after it, then stops with the cursor
where it was, so one spot can be heard again and again while placing a note on it. The moment is
200 ms long unless the Sounds page of the settings says otherwise, from 50 ms to a second.

The status bar says whether the audio is ready, still loading, missing or unreadable. A song's
audio can be an Ogg, MP3, WAV or FLAC file.

## Speed

Shift+Left and Shift+Right play 10% slower or faster, Ctrl+Shift+Left and Ctrl+Shift+Right 1%,
anywhere from 10% to 400%; the Playback menu and the toolbar change it too. The chart is not
touched. From 10% to 200% the audio keeps its pitch, unless Keep pitch is off on the Sounds page;
outside that range its pitch follows its speed.

The metronome and note ticks keep their own speed and pitch, so a tick at 25% sounds as sharp as at
100%.

## Metronome and note ticks

- The **metronome**, F3, clicks on every beat and accents the first of each measure. The Sounds page
  has nine click sounds, each with a preview.
- **Note ticks**, F4, sound on every note as it passes: a click, a beep, or one of four hit sounds.
- **Tuned lanes**, Shift+F4 or the piano-keys button beside the note ticks, give each lane's tick a
  pitch of its own, lowest on the left, so a pattern can be heard as a tune.

The music, the note ticks and the metronome each have a volume, behind the toolbar's speaker
button and on the Sounds page. Ticks are mixed into the music before it plays, so they never drift
from it; with any on, a volume changed while playing takes effect once the slider is let go.

Hidden and faded [layers](../layers.md) make no ticks.

## Seeing the audio

Once the audio loads, its waveform draws in a strip beside the lanes, lined up with the notes.
With Frequencies on in the Display settings, the strip shows the audio's spectrum instead, low
pitches on the left and high on the right, brighter the louder, so a kick and a hi-hat that sound
together show apart.

The minimap beside the notefield shows the whole chart, its notes by lane and its sections as green
lines, with a box over what the notefield shows. A click or drag on it moves the cursor there.

## Latency

Sound reaches your ears a little after the computer plays it, and a key press reaches the computer
a little after you make it. Calibrating both keeps the cursor on what you hear and notes recorded
while playing on the beat. Open Playback > Calibrate latency, or the Sounds page's Calibrate…:

1. A click plays over and over. Move the audio latency until the flash lands on each click you hear.
   The cursor then shows what is heard rather than what plays.
2. Tap along to the click sixteen times. The input latency is how late the taps come, and notes
   placed while playing go that much earlier.
