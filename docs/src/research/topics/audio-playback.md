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
each frame. The host uses the [time stretcher](time-stretch.md) to keep pitch from 10% to 200%
while Keep pitch is on. Outside
that range, or with the setting off, pitch follows speed. Dragging the cursor or panning while
playing issues seeks; the rapid-scrubbing accuracy and clock benchmark below remains unrun.

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

The spike that would settle it was never run: verify `kira` seeking and clocks under rapid
scrubbing, falling back to `rodio` plus custom seeking if too heavy.

The current implementation uses symphonia for decoding, kira for playback and timestretch for
pitch preservation. See [dependency dylib](dependency-dylib.md), [codec support](codec-support.md)
and [time stretch](time-stretch.md).
