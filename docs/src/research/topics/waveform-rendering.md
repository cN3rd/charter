---
type: Open Decision
title: Waveform rendering
description: How audio amplitude is drawn fast enough to scrub a full song smoothly.
tags: [topic, stack, rendering, audio, open]
status: draft
---

# Waveform rendering

## Observed

**Every surveyed editor displays a waveform.** It is the least optional feature in the set.

- **Tempora** precomputes min/max peaks per 10 samples and reuses render nodes rather than
  recreating them, splitting rendering per timing segment.
- **Moonscraper** queries only the tick range between camera bounds, by binary search over sorted
  lists, and pools timeline objects.
- **ArrowVortex** offers colour schemes, luminance, shape, anti-aliasing, high and low-pass
  filtering, and a filter overlay, so its waveform is an analysis surface, not just a picture.
- **osu!lazer** provides waveform comparison for timing alignment.

The shared technique is precomputed multi-resolution peaks plus drawing only what is on screen.

## Open decision

- Peak pyramid resolution and how many levels
- Whether rendering goes through the GUI framework's painter or a direct `wgpu` callback
- Whether filtering (high/low-pass views) is core or later
- Whether peaks are computed on load, lazily, or cached to disk between sessions

## Evidence

Techniques are **observed** in two shipped tools. Earlier research characterised peak pyramids as
the standard, cheap, scalable approach, grade **inferred**, and uncontroversial.

What is missing is any measurement on the candidate Rust stack. Waveform downsampling performance
and rendering throughput were explicitly marked untested. This is the same unrun spike that blocks
[GUI framework](gui-framework.md) and [performance targets](performance-targets.md).

## Draft position

`charter-audio`'s `Peaks` keeps the lowest and highest sample of each 64-frame block, both
channels together, with coarser levels of twice the length above it, built on the thread that
decodes the audio. The notefield draws the waveform in a strip of its own between the timing
gutter and the lanes. Each screen row asks for the extent of the audio time it covers, read from
the coarsest level that still gives a few blocks, so the drawing lines up with the notes through
every tempo change, costs the same at any zoom, and turns with the notefield. The Display settings
turn it off. Drawing behind the notes, colour by frequency and a GPU path are not built.
