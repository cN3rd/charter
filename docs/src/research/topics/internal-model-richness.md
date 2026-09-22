---
type: Open Decision
title: Internal model richness
description: How much more the internal model must represent than any single target format.
tags: [topic, data-model, multi-game, open]
status: draft
---

# Internal model richness

## Observed

The surveyed formats are mutually non-equivalent. StepMania row/note types, osu!mania millisecond
timing, Quaver scroll velocity and timing groups, guitar events, and Malody beat/effect arrays do
not form one equivalent data model. See [interoperability](../formats/interoperability.md).

## Open decision

- A superset of all target formats
- A superset of the MVP target only, extended later
- Per-ruleset models sharing a common timing core

The cost of the first is carrying concepts no current user needs. The cost of the second is that
widening a core model later is the kind of change that touches everything.

## Evidence

Earlier research asserted "the internal model must be richer than any one format" as an acceptance
criterion. The premise (formats are non-equivalent) is **observed**; the conclusion about *how much*
richer does not follow from it.

Grade: **asserted**.
