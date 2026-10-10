---
type: Guide
title: Getting started
description: Installing charter, the parts of its window, and starting, opening or importing a song.
tags: [guide, install]
status: draft
---

# Getting started

## Installing

Builds for 64-bit Linux and Windows are on the project's
[releases page](https://github.com/cN3rd/charter/releases), one for every change to the main
branch. Download `charter-<version>-linux-x86_64.tar.gz` or `charter-<version>-windows-x86_64.zip`,
unpack it anywhere and run `charter` or `charter.exe`. There is no installer, and the program needs
no files beside it: its sounds and fonts are built in. The `licenses` folder holds the licences of
everything it includes.

Some features need more:

- [ffmpeg](https://ffmpeg.org), to convert audio when exporting a Tap Tap Revenge track. Charter
  checks the binary set under Settings > Sounds first, then the `PATH`. If neither runs, charter
  offers help at first start: a verified release download on Windows or macOS, or a package
  manager command to copy on Linux. You can open the help again from Settings.
- A graphics card, for finding timing with Mapperatorinator. Without one it runs on the processor,
  much more slowly.

To build it yourself, install Rust, and on Linux `clang`, `mold` and the ALSA headers
(`libasound2-dev` on Debian and Ubuntu), then run `cargo run --release -p charter-app` in a clone
of the repository. [Packaging](../packaging.md) covers release builds.

## The window

- The **menu bar** holds File, Edit, Timing, View, Playback and Help. Every command with a key
  shows it beside its name.
- The **transport toolbar** under it shows the position as `measure:beat:tick` and time, with
  play, the tempo, snap, speed, grid and zoom, and the volumes behind the speaker button. As the
  window narrows it drops the less used controls; the menus still have them.
- The **song panel** on the left holds the song's details, its charts and their settings, its
  layers, its timing points and its markers. Ctrl+\\ folds it away.
- The **notefield** fills the rest. From left to right it has the ruler with measure numbers, the
  timing column, the waveform, the lanes and the minimap. The bright line across it is the cursor,
  where notes go and playback starts.
- The **status bar** at the bottom says whether the audio is ready, counts the selection and names
  the active layer. Messages float over the notefield, each with a Dismiss button.

F1 lists every key, with a filter; [Keyboard shortcuts](shortcuts.md) has the same list.
Publication at <https://cn3rd.github.io/charter/> is currently disabled. The source is in
`docs/src/guide/`; with `just`, mdbook and Python installed, `just docs-serve` previews the book
from a checkout. The book uses `python` on Windows and `python3` elsewhere.
Help > About charter shows the version and the commit it was built from, worth including in a bug
report, with the licence and credits.

## Starting a song

With nothing open, the window offers Open song…, New song and the demo song. The demo is also under
Help, to try things on.

**New song** (Ctrl+N) asks for:

- the game, and the layout every chart shares, such as dance-single or 4K, drawn as lanes;
- the title and artist, which an audio file's tags fill in;
- the charts and their names, or for Tap Tap Revenge the levels, with presets for the usual sets;
- the audio, dropped on the dialog or chosen with its button. It can also come later.

The song starts at 120 BPM in 4/4 with no notes. Next comes its [timing](timing.md), then the
notes.

Until the song is saved it lives in memory and in the autosave. **Save** (Ctrl+S) the first time
asks where, and makes the song a folder: choosing `Anthem` makes `Anthem/Anthem.json` with a copy
of the audio beside it. [Files and recovery](files.md) has the details.

## Opening a song

- **Open song…** (Ctrl+O) opens a charter song, a `.json` file.
- **File > Open recent** lists the last ten.
- `charter path/to/song.json` on the command line opens one at start.
- On Windows and X11 a song file dropped on the window opens. Under Wayland nothing can be dropped
  yet, so the window offers its buttons instead.
- **File > Import song…** opens a Tap Tap Revenge track or an Editor on Fire MIDI file as a new
  song; see [Games, import and export](games.md).

A song opens for editing when charter has its game's plugin. A song of another game opens
read-only, with a banner naming the plugin it needs.
