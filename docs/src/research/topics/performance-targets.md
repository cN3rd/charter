---
type: Open Decision
title: Performance targets
description: Proposed frame, latency and jitter budgets, none of which have been validated against users.
tags: [topic, ux, performance, open]
status: draft
---

# Performance targets

## Proposed figures

Earlier research states these as requirements:

| Metric | Target |
|---|---|
| Frame rate | 60 fps |
| Frame time p95 | <= 16.6 ms |
| Frame time p99 | <= 24 ms |
| Input to visible highlight, p95 | <= 50 ms |
| Playhead jitter | <= one frame, roughly 16 ms |
| Test chart | 5 minutes, 10,000 notes |
| Acceptance span | 32 bars of playback and editing |
| Hardware | low-end GPU |

## Open decision

Whether these are the right numbers, and whether they are gates or aspirations.

50 ms input-to-highlight is a notably loose budget for a tool whose users are working to
millisecond precision on the audio axis. Whether visual feedback latency and timing precision are
felt as the same thing is exactly the untested question.

## Evidence

Grade: **asserted**. Earlier research is explicit that these came from an internal design document,
and that whether they produce the intended subjective feel requires user testing that was never
done.

No rendering benchmark was run either. The relevant spike, waveform and spectrogram at 60 fps with
tens of thousands of line segments, was identified and left unrun. It is therefore not established
that these targets are achievable on the candidate stack, let alone sufficient. See
[waveform rendering](waveform-rendering.md) and [GUI framework](gui-framework.md).
