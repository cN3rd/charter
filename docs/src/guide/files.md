---
type: Guide
title: Files and recovery
description: Where a saved song's files go, what charter keeps in its data folder, and how autosave, snapshots and crash reports protect work.
tags: [guide, files, autosave, recovery]
status: draft
---

# Files and recovery

## A song on disk

A song is one `.json` file, with its audio and artwork beside it. Saving a song somewhere new makes
it a folder: choosing `Anthem` writes `Anthem/Anthem.json`, copies the audio in, keeping a
subfolder it was in relative to the song, and writes the artwork. Choosing the song's own file in
its folder saves over it.

A save that would lose something stops instead and says why: different audio already at the
destination, a copy that fails, or another picture under the artwork's name that the song did not
name. A song whose file would pass 64 MiB, a few hundred thousand notes, stops too, since charter
opens no larger file. Saves write beside the song and then move over it, so a failed save leaves
the old file.

An export to another program's format says it is exporting while its files are being written,
and reports success only after the main file, files beside it and copied resources are all written.
If one fails, charter names the file that was not written instead.

Charter keeps what it works out from the audio in a `cache` folder in the song's folder, such as
the waveform, so the next open shows it at once. Deleting the folder only costs the time to work it
out again.

The title bar marks a song with unsaved changes with a star. Closing the window, opening another
song or starting a new one first asks whether to save them.

A song stays in Open recent after a permission or drive error, or if its contents are invalid, so
it can be tried again. Only a file the host reports as missing leaves the list.

## The data folder

Charter keeps its own files in `~/.local/share/charter` on Linux and `%APPDATA%\charter\data` on
Windows; the Autosave page of the settings names it. It holds:

- `app.ron`, the settings, only those changed from their defaults;
- `recovery.json`, the autosave, and `recovery-unreadable.json`, one a later start could not read;
- the timing models' weights, once downloaded;
- `snapshots`, when snapshots in a folder are on;
- `crash-<number>.txt`, a report for each crash.

## Autosave

Half a minute after a change, charter saves the unsaved song to `recovery.json`; the Autosave page
sets the wait from 5 seconds to 10 minutes, or turns it off. Saving the song, undoing back to the
saved version, or throwing the changes away removes the file. If charter closes without any of
those, as after a crash or a power cut, the next start offers the song back, unsaved, for the file
it belongs to. An autosave that start cannot read is not thrown away: charter says so and moves it
to `recovery-unreadable.json` beside it, where later autosaves leave it for repair.

## Snapshots

Snapshots keep a history. Every few minutes, 5 unless the Autosave page says otherwise, a changed
song is copied with the date and time in its name, such as `Anthem 2026-09-29 05.40.12 UTC.json`:

- **in a folder**, `snapshots/<song>` in the data folder or a folder chosen on the Autosave page;
- **beside the song**, in an `autosaves` folder next to the song's file, once it is saved.

Both are off to start, and each switches on by itself. A song unchanged since its last copy is not
copied again, and each folder keeps the newest 20 copies of a song unless the page says otherwise.

A snapshot keeps the chart JSON only, not copies of its audio, artwork or samples. Their paths
remain relative to the JSON file, so opening a snapshot directly in `snapshots` or `autosaves`
looks for those resources in the wrong folder.

To restore a saved song, copy the snapshot into the original song's folder under a different
name, such as `Anthem-restored.json`, and open that copy. The audio, artwork and samples then
resolve beside the original song. Keep the current JSON until you are ready to replace it.
For a song that was never saved, choose its audio and artwork again and put any sample files at
the paths the snapshot names; it does not keep the original chosen paths or unsaved artwork bytes.

## Crashes

If charter crashes it writes `crash-<number>.txt` to the data folder, saying what went wrong and
where, and a dialog names the file before the window closes. Attach that file to a bug report on
the [issue tracker](https://github.com/cN3rd/charter/issues). The last autosave is offered on the
next start.
