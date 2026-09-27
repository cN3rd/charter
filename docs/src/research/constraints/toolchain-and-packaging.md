---
type: Constraint
title: Toolchain and packaging
description: Hard limits on Rust shared libraries, FFI dependencies and platform coverage.
tags: [constraints, build, packaging, platform]
status: draft
sources:
  - id: bevy-dylib
    resource: https://docs.rs/bevy_dylib/latest/bevy_dylib/
    title: bevy_dylib documentation
  - id: rust-34909
    resource: https://github.com/rust-lang/rust/issues/34909
    title: rust-lang/rust#34909, diamond dependency limitation
---

# Toolchain and packaging

## Rust compiler

The workspace requires Rust 1.95 or newer: egui and eframe 0.36.2 declare that minimum. Rust
1.85 introduced edition 2024, but it cannot build these dependencies or the source's let chains.

## Rust shared libraries

- Rust `dylib` artifacts have an **unstable ABI and are toolchain-locked**. Wrapper and binary must
  share rustc version and flags. A rustup update invalidates the artifact.
- **It cannot ship.** The binary requires the `.so` and takes `libstd.so` dynamically. Direct
  execution, debugger and IDE launches, and CI artifacts all fail without loader paths.
  `RUSTFLAGS="-C rpath"` works but embeds machine-local rustup paths. Bevy documents the same
  warning for its own dylib crate.
- **Only one dependency dylib is viable.** Rust's diamond-dependency limitation makes multiple
  dependency-specific wrappers unsatisfiable; a two-wrapper experiment produced 22 errors.
- It gives **no benefit** to `cargo check`, rust-analyzer, cold builds, or dependency compilation.
  It changes link behaviour only.
- `cargo-add-dynamic`, the cited automation, is unmaintained since 2022 at version `0.1.2`.

The unstable-ABI point bounds two separate things: the wrapper crate already present in this
repository, see [dependency dylib](../topics/dependency-dylib.md), and any future native plugin
loaded at runtime, see [plugins](../topics/plugins.md). The second is the harder case, because a
plugin author does not share the host's toolchain.

## FFI and runtime dependencies

- `signalsmith-stretch` is not pure Rust: FFI plus a C++ build step via `cc`.
- `ort` brings ONNX Runtime and its cross-platform runtime requirements.

## Platform

- Windows, macOS and Linux are all required.
- GPUI has platform-support and documentation gaps. Freya is experimental.
- The only build measurements taken were on Linux. Windows MSVC and macOS linker behaviour are
  unmeasured.
- Surveyed tools carry external binary dependencies of their own: ArrowVortex needs `oggenc2.exe`
  for Ogg conversion, and the wider set uses BASS, ffmpeg, aubio, SDL/OpenGL, MonoGame and
  osu-framework.
