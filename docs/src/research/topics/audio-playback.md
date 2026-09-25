---
type: Open Decision
title: Audio playback
description: Which crate drives transport, given that an editor needs sample-accurate seeking rather than just playback.
tags: [topic, stack, audio, open]
status: draft
---

# Audio playback

## Candidates

| Crate | Role | Licence |
|---|---|---|
| `symphonia` | Pure-Rust demux and decode: FLAC, MP3, AAC, Vorbis, WAV, ISO-MP4, OGG; raw samples and metadata | MPL-2.0 |
| `cpal` | Low-level cross-platform I/O: ASIO, WASAPI, CoreAudio, ALSA | MIT/Apache-2.0 |
| `rodio` | High-level playback over `cpal`, decoding via `symphonia`; sources, mixing, speed control | MIT/Apache-2.0 |
| `kira` | Sample-accurate seeking, tweens, clocks, effect chains, playback-rate changes, spatial | MIT/Apache-2.0 |

`rodio` and `kira` both sit on `cpal` + `symphonia`, so the decode layer is settled by default
regardless of which front end is chosen.

## Draft position

`kira` 0.12, with its own decoding off. `charter-audio` decodes through `symphonia-wrap` into stereo
frames, sans-io and tested, and `charter-app` hands kira those frames and plays them. The window
never sees the device: it asks for play, pause, seek and rate, and the host reports the playhead
each frame. The playback rate is kira's, so the pitch follows the speed, as tape does; keeping the
pitch needs a time-stretch, still [open](time-stretch.md). Scrubbing is not built, so the spike
below still decides whether kira's seeking holds up.

## Open decision

`rodio` or `kira`, or `cpal` directly.

The distinguishing requirement is scrubbing: dragging a playhead through audio issues seeks
continuously, and an editor needs the resulting position to be exact rather than approximately
right. `kira` documents sample-accurate seeking and musical clocks; `rodio` is described as weaker
there.

## Evidence

Grade: **observed** for the API differences, **asserted** for whether they matter in practice.
Earlier research called `kira` a "strong pick" and was "leaning kira over rodio", based on API
reading.

The spike that would settle it — verify `kira` seeking and clocks under rapid scrubbing, falling
back to `rodio` plus custom seeking if too heavy — was never run.

`symphonia`'s selection is the one part of this already acted on in the repository; see
[dependency dylib](dependency-dylib.md) and [codec support](codec-support.md).
