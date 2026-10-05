---
type: Decision
title: Time stretch
description: Slowing audio for difficult passages without changing pitch, through the pure-Rust timestretch crate.
tags: [topic, stack, dsp, audio]
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
| `timestretch` `0.15.0` | MIT | Pure Rust, `forbid(unsafe_code)`, on `rustfft`; keylock from 0.25x to 2x |

The first survey found no pure-Rust option; `timestretch` came later.

## Decided

`timestretch`'s real-time engine in its `WideKeylock` profile, an identity-locked phase vocoder
over the whole spectrum, fed into kira as a streaming sound's decoder, so it stretches just ahead
of playback. On a 4-minute song in a release build it ran 4.5x real time at 2x speed and 35x at
a quarter speed; a seek rebuilds it in about a millisecond and its first chunk follows within 10 ms.
A 440 Hz tone at half speed stays at 440 Hz. It plays 0.6 to 2.3 dB quieter than the source.
The batch `stretch` of the same crate took 29 s for the whole song, too long to wait for on
play. Charter keeps pitch from 0.1x to 2x: an individual real-time keylock stage reaches down
to 0.5x, so slower speeds chain stages. A calibration click measures the chain's lead so the
audio stays aligned with the cursor. Above 2x, or with Keep pitch off, pitch follows speed.

## Was open

Which route, or whether to ship rate-only playback first and treat pitch preservation as later.

## Evidence

Earlier research called `signalsmith-stretch` the "best route" and preferred it over SoundTouch.
Licensing is **observed**; algorithm quality is **asserted**.

The earlier `signalsmith-stretch` proposal left its cross-platform `cc` build spike unrun.
That uncertainty belongs to the FFI candidate; the chosen timestretch implementation is pure Rust.

Related constraint: [toolchain and packaging](../constraints/toolchain-and-packaging.md).
