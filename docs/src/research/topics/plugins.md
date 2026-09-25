---
type: Open Decision
title: Plugins
description: How per-game support and third-party extensions are loaded, given that Rust has no stable ABI.
tags: [topic, architecture, plugins, multi-game, open]
status: draft
sources:
  - id: rust-abi
    resource: https://doc.rust-lang.org/reference/abi.html
    title: Rust reference, ABI
---

# Plugins

Charter's crate taxonomy already assumes an extension surface: built-in plugins provide per-game
support and optional features, and are meant to be written against the same interface a
third-party plugin would use. That constraint only pays off if the surface is real, so how
plugins load is an architectural decision, not a packaging detail.

**Evidence warning.** The prior research corpus processed here does not contain the plugin
decision itself. The build-times document refers in passing to a "separately rejected
compiled-plugin decision", so a decision appears to have been reached somewhere outside the
documents available. Everything below is reconstructed from adjacent findings, not from that
document.

## Why this is hard in Rust

**Rust has no stable ABI.** A native plugin compiled by a different rustc version, or with
different flags, cannot be safely loaded. The same constraint that makes the
[dependency dylib](dependency-dylib.md) a development-only trick applies with more force to
plugins, because a plugin author does not share the host's build environment.

The usual escapes, each with a different cost:

| Mechanism | ABI safety | Cost |
|---|---|---|
| Native `cdylib` with a C ABI boundary | Safe if the boundary is `extern "C"` and `#[repr(C)]` | Hand-written FFI surface; every rich Rust type must be flattened |
| Native `dylib` with Rust ABI | Unsafe across toolchains | Plugins must be rebuilt in lockstep with the host |
| WASM component | Safe, sandboxed, portable | Runtime dependency; host-call overhead; awkward for tight loops |
| Embedded scripting (Lua, Rhai) | Safe | Slower; a second language in the project |
| Compiled in, no runtime loading | Trivially safe | Third parties must fork and rebuild the whole editor |

Note that StepMania's own ecosystem solved the analogous problem with scripting rather than
binaries, and that ArrowVortex ships customizable game styles and noteskins as **data** rather
than as code.

## Open decision

- Which mechanism, if any, carries third-party extensions
- Whether "built-in plugins" are literally the same mechanism or merely the same trait surface
  compiled in, which is much cheaper and still enforces the boundary
- What a plugin is allowed to do: define a ruleset and note vocabulary, add a format adapter,
  add an analysis pass, add UI, or all of those
- Whether plugins may add to the chart data model, which interacts with
  [note type model](note-type-model.md) and [internal model richness](internal-model-richness.md)

## A concrete driver

Licensing supplies one independent reason to want a process or plugin boundary: **aubio is
GPL-3.0**, so matching ArrowVortex's beat-detection quality by linking it directly is incompatible
with a permissively licensed binary. Isolating it behind a boundary is one of the recorded options.
See [beat detection](beat-detection.md) and [licensing](../constraints/licensing.md).

That is an argument for *some* isolation mechanism, not specifically for third-party plugins.

## Evidence

Grade: **asserted**. No document available here establishes the requirement, the mechanism, or the
rejection. The ABI constraint itself is **observed** and is a property of Rust rather than of this
project.

Worth recovering the original plugin decision document before treating any of this as settled.

## Draft position

Built-in plugins are the same trait surface compiled in. `charter-core` defines `RulesetPlugin`,
each ruleset crate implements it, and `charter-app` registers the built-in ones. The editor asks it
whether a song, and each edit to it, is valid for the game. Nothing loads a plugin at runtime.
