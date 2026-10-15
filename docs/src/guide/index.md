# User guide

A rhythm game gives you a song and a row of lanes, and asks you to hit each note as it reaches
the line. A **chart** is that pattern: which note, in which lane, at which moment. Charter is
the editor you write one in.

Making a chart is two jobs, and they go in this order:

1. **Time the song.** Tell charter where the first beat falls and how fast the song runs, so its
   grid of beats lines up with the music.
2. **Place the notes.** Put each note on the grid, on the sound you want the player to hit.

The order matters. Notes sit on beats, so a grid corrected after the fact drags every note with
it. Time first.

## Words this guide uses

| Word | What it means |
|---|---|
| Chart | One difficulty of one song: every note, in order. A song usually holds several. |
| Lane | A column notes fall in. One lane is one key, pad or arrow. |
| Tap | A note hit once. |
| Hold | A note pressed and kept down for a while. |
| Beat | One pulse of the music, the thing you tap your foot to. |
| Measure | A group of beats, four in most songs. Charter numbers them down the ruler. |
| BPM | Beats a minute. 135 BPM is a little over two beats a second. |
| Offset | Where beat 0 falls in the audio file, in milliseconds. |
| Timing | Lining charter's grid of beats up with the music. |
| Snap | How finely the cursor moves between beats: a 4th, an 8th, a 16th of a note, and so on. |
| Cursor | The bright line across the editor. Notes go on it, and playback starts from it. |
| Layer | A named, coloured group of notes inside one chart, such as one per instrument. |

## The chapters

- [Getting started](./getting-started.md) — installing charter, opening a song, and what every
  part of the window is for.
- [Your first chart](./first-chart.md) — one song from an audio file to a saved chart. Start
  here if you have never charted.
- [Charting](./charting.md) — the cursor, placing and editing notes, layers, and the song panel.
- [Timing](./timing.md) — the offset, timing points, the timing window, and finding timing from
  the audio.
- [Listening](./listening.md) — playback, speed, the metronome, note ticks and latency.
- [Games, import and export](./games.md) — what each game allows, and moving songs in and out.
- [Files and recovery](./files.md) — where a song's files go, autosave, snapshots and crashes.
- [Settings](./settings.md) — every page of the settings window.
- [Keyboard shortcuts](./shortcuts.md) — every key and mouse gesture, in one table.

The screenshots throughout follow one song: a three-lane Tap Tap Revenge chart of KDrew's *Last
Train To Paradise (Dr. Fresch Remix)*, at 135 BPM. Nothing in the guide depends on having that
file — use whatever song you have, or Help > Open the demo song, which needs no files at all.
Where a screenshot would have shown that release's cover art, the thumbnail is blurred; charter
itself draws it sharp.
