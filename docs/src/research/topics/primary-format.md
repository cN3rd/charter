---
type: Open Decision
title: Primary format
description: Which chart format the editor is built around first.
tags: [topic, scope, formats, open]
status: draft
---

# Primary format

## Observed

Every surveyed editor is built around one format and converts to the rest. ArrowVortex is
`.sm`/`.ssc`-native. Quaver is `.qua`-first. Moonscraper is `.chart`-first. None of them treats
several formats as equally native.

OpenChart states multi-game support as a goal, but the surveyed evidence does not establish a
shipped feature set behind it.

## Open decision

- `.ssc` — most expressive, so least lossy as an internal pivot
- `.sm` — simpler, wider legacy reach
- `.osu` — largest active community, but millisecond-based and structurally narrower
- `.qua` — richest scroll-velocity model
- An internal format with exporters to all of the above

The choice largely determines how much of the editor is conversion code, and whether conversion
runs on load or on export.

## Evidence

Explicitly unresolved in earlier research. One document names StepMania/ITG 4-panel as the MVP
target while surveying a far wider domain, so the stated scope and the researched scope disagree.

Grade: **unresolved**.

Related: [round-trip fidelity](round-trip-fidelity.md),
[internal model richness](internal-model-richness.md).
