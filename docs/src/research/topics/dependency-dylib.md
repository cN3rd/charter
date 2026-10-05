---
type: Open Decision
title: Dependency dylib
description: The build-time dylib trick for link speed, already adopted here ahead of the research that recommended waiting.
tags: [topic, stack, build, compile-times, open, acted-on]
status: draft
sources:
  - id: bevy-dylib
    resource: https://docs.rs/bevy_dylib/latest/bevy_dylib/
    title: bevy_dylib
  - id: rust-34909
    resource: https://github.com/rust-lang/rust/issues/34909
    title: rust-lang/rust#34909
---

# Dependency dylib

A **build-time** trick: heavy third-party dependencies are compiled into one shared object so
incremental rebuilds link against it instead of relinking everything. It exists to make rebuilds
fast during development.

This is **not** a plugin mechanism and has nothing to do with loading code at runtime. The wrapper
is a Rust `dylib`, not a `cdylib`; call sites keep ordinary imports, and the artifact is
toolchain-locked and unshippable. For runtime extension, see [plugins](plugins.md).

The facade is used by `charter-audio`. The measurements below come from an earlier synthetic-stack
experiment, not a benchmark of the current editor.

## Current repository state

- `third-party/symphonia-wrap` exists as a `dylib` + `rlib` crate containing `pub use symphonia::*;`
- The dev profile sets `opt-level = 3` for every `symphonia-*` package and for the wrapper
- `charter-audio` depends on the wrapper and decodes through it
- No `dylink` feature gate exists; declaring both artifacts does not require a dynamically
  linked release. The Linux release links the `rlib` and needs neither the wrapper's `.so` nor a
  shared Rust standard library

## What the research recommended

"Adopt it, but not yet: when the first real editor build exists and a rebuild first feels slow."
Specifically: an **off-by-default `dylink` feature**, **one wrapper for all heavy dependencies**,
and **static release and CI builds**.

The single-wrapper constraint still applies; the static-release requirement is already met:

- **The wrapper is named for symphonia.** Only one dependency dylib can exist, because Rust's
  diamond-dependency limitation makes multiple wrappers unsatisfiable (a two-wrapper experiment
  produced 22 errors). If the GUI or audio stack later wants the same treatment, it must join this
  crate rather than get its own. The name will then be wrong.
- **A `dylink` feature is optional.** It could select development linkage explicitly, but its
  absence does not prevent a static release. Check the resulting binary's dependencies rather
  than infer its linkage from the wrapper's `crate-type` declaration.

## Measurements

Linux only, `eframe` with its default `glow` renderer, against a synthetic stack rather than a real
editor crate.

| Scenario | Static | Dylib |
|---|---|---|
| One-line rebuild, rust-lld | 636-661 ms | 313-331 ms |
| One-line rebuild, GNU bfd | 3.566-3.853 s | 515-535 ms |
| 7,080-line first-party addition | 1,082 ms | 808 ms |
| `cargo check` | 172 ms | 175 ms |
| Artifact size | 205 MiB binary | 0.95 MiB binary + 264 MiB `.so` |

The saving is **link-time only**. The Symphonia family costs 3.0 s of compiler work once either
way; dynamic linking moves heavy dependency objects into a `.so` so later rebuilds link against a
recorded reference. It does nothing for `cargo check`, rust-analyzer or cold builds.

## Open decision

- Whether measured link savings on the current editor and each platform justify the wrapper
- Whether to rename it for its actual role, since it can only ever be the single shared wrapper
- Whether an explicit development-linkage feature would be useful; release builds are already static
- Whether the dev-profile `opt-level = 3` block stands on its own merits, independent of the dylib
  question, since it addresses decode speed in debug builds, a separate concern

## Evidence

Grade: **measured**, but narrowly: one platform, one linker pair, one renderer, no real editor
crate. The author characterised it as "worth ~300 ms" and a second-order optimization.

The experiment did not measure Windows MSVC, macOS linker behaviour, `cargo test` or the later
workspace layout. Cranelift was not measured because the nightly component was not installed.

Full caveat list: [toolchain and packaging](../constraints/toolchain-and-packaging.md).

The same research is explicit that this mechanism "is build-time first-party development linkage,
not a stable-ABI plugin system", and that adopting it does not reopen the separately decided
question in [plugins](plugins.md).
