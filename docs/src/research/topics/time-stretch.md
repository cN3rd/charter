---
type: Open Decision
title: Time stretch
description: Slowing audio for difficult passages without changing pitch, where no pure-Rust option was found.
tags: [topic, stack, dsp, audio, open]
status: draft
---

# Time stretch

## Observed

Playback-rate control is universal; pitch-preserving stretch is not.

- **ArrowVortex** clamps playback speed to 10%-400%.
- **osu!stable** offers 25%, 50%, 75%, 100%.
- **Editor on Fire** steps by 10% or 25% with Ctrl.
- **`kira`** supports playback-rate changes directly.

Whether those preserve pitch is not established by the sources.

## Candidates

| Option | Licence | Cost |
|---|---|---|
| `signalsmith-stretch` `0.1.3` | MIT crate and library | FFI plus a C++ build step via `cc` |
| `soundtouch` / `soundtouch-sys` | bindings MIT-ish, core LGPL-2.1 | LGPL makes linking it as a shared library the cleaner route |
| `rubato` | MIT | Resampling only, changes pitch with rate |
| Render offline | n/a | Latency at the point of use instead of CPU during playback |

**No pure-Rust time-stretch or pitch-shift option was found.**

## Open decision

Which route, or whether to ship rate-only playback first and treat pitch preservation as later.

## Evidence

Earlier research called `signalsmith-stretch` the "best route" and preferred it over SoundTouch.
Licensing is **observed**; algorithm quality is **asserted**.

The spike was never run: verify the `cc` build across Windows, macOS and Linux. That matters more
than usual here, because it is the only FFI dependency in the proposed stack and a broken build on
one platform blocks a release.

Related constraint: [toolchain and packaging](../constraints/toolchain-and-packaging.md).
