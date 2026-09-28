---
type: Design Note
title: Packaging
description: What `just dist` puts in a release package, why the binary needs no files beside it, and what is fetched at run time instead.
tags: [build, packaging, release, licensing]
status: draft
---

# Packaging

`just dist` builds the editor with the `dist` profile, release with symbols stripped, and writes two
packages to `dist/`: `charter-<version>-linux-x86_64.tar.gz` and
`charter-<version>-windows-x86_64.zip`. `just dist-linux` and `just dist-windows` build one each;
the Windows build cross-compiles with cargo-xwin and links the C runtime in, so it needs no Visual
C++ redistributable.

## What a package holds

| File | What it is |
|---|---|
| `charter`, `charter.exe` | the editor, with everything it draws and plays built in |
| `LICENSE-MIT`, `LICENSE-APACHE` | charter's own licences |
| `licenses/Inter-OFL.txt` | the Inter font's licence |
| `licenses/metronome-sounds-*` | the click sounds' licence, CC0 text and provenance |
| `licenses/hit-sounds-*` | the note-tick hit sounds' licence, CC0 text and provenance |
| `licenses/THIRD-PARTY.md` | every crate linked into the binary, with its version and licence |

There is no assets folder, and none is missing. The resources the editor ships with are compiled
into the binary with `include_bytes!`, so it runs from anywhere with only its own file:

- the metronome sounds, two WAVs for each kit, in `charter-audio/src/click.rs` from
  `crates/charter-audio/assets/clicks/`
- the note-tick hit sounds, in the same file from `crates/charter-audio/assets/hitsounds/`
- the Inter fonts, in `charter-style/src/lib.rs` from `crates/charter-style/assets/fonts/`
- the Phosphor icon font, which the `egui-phosphor` crate embeds

The audio decoder needs no library beside it either. `symphonia-wrap` is a `dylib` only to speed
up dev builds (see [Dependency dylib](./research/topics/dependency-dylib.md)); the release build
links it in. The Linux binary depends on system libraries alone, and the Windows one on system
DLLs alone.

A new resource that must ship goes in the same way: `include_bytes!` in the crate that uses it,
and its licence copied into `licenses/` by the `_licenses` recipe in the `justfile`.

## What is not in a package

A song's audio, artwork and chart files are the user's, beside the song, and never packaged.

The Beat This! weights, `final0.ckpt` at 81 MB, are left out on purpose: they are needed only by
[automatic timing](./autotiming.md). The first time Timing › "Find timing from the audio…" runs,
charter asks before downloading them from the authors' server into its data folder, as
`beat_this-final0.ckpt`. The folder is eframe's storage directory for charter, the same one that
holds the settings and the crash-recovery file; on Linux that is `~/.local/share/charter`
(or under `$XDG_DATA_HOME`), and on Windows `%APPDATA%\charter\data`.
