---
type: Design Note
title: Packaging
description: What `just dist` puts in a release package, why the binary needs no files beside it, and what is fetched at run time instead.
tags: [build, packaging, release, licensing]
status: draft
---

# Packaging

`just dist` builds the editor with the `dist` profile, a release build with its symbols stripped.
It writes two packages to `dist/`: `charter-<version>-linux-x86_64.tar.gz` and
`charter-<version>-windows-x86_64.zip`. `just dist-linux` and `just dist-windows` build one each;
the Windows build cross-compiles with cargo-xwin, or builds with MSVC when run on Windows as CI
does, and links the C runtime in, so it needs no Visual C++ redistributable. Cross-compiling, LLVM's
`llvm-rc` stands in for the `rc.exe` that turso's build script, under burn's kernel cache, runs.
CI tests every push and pull request, and publishes each push to `main` as a prerelease of both
packages, its notes listing the commits since the one before.

## What a package holds

| File | What it is |
|---|---|
| `charter`, `charter.exe` | the editor, with everything it draws and plays built in |
| `LICENSE-MIT`, `LICENSE-APACHE` | charter's own licences |
| `licenses/Inter-OFL.txt` | the Inter font's licence |
| `licenses/metronome-sounds-*` | the click sounds' licence, CC0 text and provenance |
| `licenses/hit-sounds-*` | the note-tick hit sounds' licence, CC0 text and provenance |
| `licenses/mapperatorinator-*` | Mapperatorinator's MIT licence and what the port takes from where |
| `licenses/THIRD-PARTY.md` | every crate linked into the binary, with its version and licence |
| `BUILD.txt` | the commit and the `rustc` and `cargo` versions it was built with |

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

## Reproducible packages

Building the same commit with the same toolchain gives the same package byte for byte, wherever the
checkout is and whenever it is built. Anyone can check a release by rebuilding it:

```bash
git checkout <commit from BUILD.txt>
rustup toolchain install <version from BUILD.txt> && rustup override set <version>
just dist-linux    # or dist-windows
sha256sum dist/*.tar.gz
```

What keeps the build from depending on the machine:

- `--remap-path-prefix` replaces the checkout's folder with `.` and cargo's with `/cargo` in what
  rustc writes into the binary, panic locations included. `-ffile-prefix-map` does the same for
  the C that `ring` and `zstd-sys` compile. The toolchain's own paths are remapped by rustc already.
- `--locked` builds the crates `Cargo.lock` names and refuses to resolve others.
- The `dist` profile's `codegen-units = 1` and thin LTO leave the code's order to the compiler
  alone, and `strip` removes the debug information.
- On Windows `/Brepro` keeps the link time out of the executable.
- `packaging/pack.py` writes the archive itself, entries sorted, every file stamped with the
  commit's time, owners and permissions fixed, where `tar` and `zip` would record the machine's.
- Nothing reads the clock or the machine at build time. The commit the About window names comes in
  as `CHARTER_COMMIT`, set by the `justfile` from git.

The recipes set these in `_reproducible`. A plain `cargo build --release` has none of them, and its
binary names the folder it was built in.

## What is not in a package

A song's audio, artwork and chart files are the user's, beside the song, and never packaged.

ffmpeg is not packaged either. Exporting a Tap Tap Revenge track converts audio that is not AAC
with the binary chosen under Settings > Sounds, falling back to `ffmpeg` on the `PATH`. Without
ffmpeg everything else still exports, and the export says the audio was not written.

The Beat This! weights, `final0.ckpt` at 81 MB, and Mapperatorinator's, 416 MB, are left out on
purpose: they are needed only by [automatic timing](./autotiming.md). The first time Timing ›
"Find timing with Beat This!…" or "Find timing with Mapperatorinator…" runs, charter asks before
downloading its model's weights into its data folder: Beat This!'s from the authors' server as
`beat_this-final0.ckpt`, Mapperatorinator's from Hugging Face as `timingator-v32-f16.gguf`. The folder is eframe's storage directory for charter, the same one that
holds the settings and the crash-recovery file; on Linux that is `~/.local/share/charter`
(or under `$XDG_DATA_HOME`), and on Windows `%APPDATA%\charter\data`.
