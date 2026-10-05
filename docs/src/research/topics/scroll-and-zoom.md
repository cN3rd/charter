---
type: Open Decision
title: Scroll and zoom
description: Navigation feel, including whether momentum helps or harms precision.
tags: [topic, ux, navigation, open]
status: draft
---

# Scroll and zoom

## Observed

- ArrowVortex offers both snap scrolling and continuous scrolling, reverse scroll, and two view
  modes: C-mod (time-based) and X-mod (row-based). It also has a minimap with Notes and Density
  modes, and jumps to beat, measure, stream start and end, selection start and end, and chart start
  and end.
- osu!lazer provides zoom and drag seeking on a waveform timeline.
- Moonscraper queries only the tick range between camera bounds, by binary search over sorted
  lists.

C-mod versus X-mod is the substantive one: it decides whether vertical distance means time or
beats, which changes what the chart looks like through a BPM change.

## Open decision

- Whether both view modes exist from the start
- Cursor-anchored zoom versus centre-anchored
- Whether scroll momentum and easing exist at all, and whether they are user-controllable

## Evidence

View modes and navigation commands are **observed**. Earlier research proposed cursor-anchored
zoom, continuous scrolling and optional momentum as requirements, grade **asserted**, and
explicitly left open whether momentum helps or harms precision and what per-user controls are
needed.
