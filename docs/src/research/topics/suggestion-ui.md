---
type: Open Decision
title: Suggestion UI
description: How automatic analysis results reach the chart, and whether anything is ever applied without consent.
tags: [topic, ux, automation, open]
status: draft
---

# Suggestion UI

## Observed

**ArrowVortex** analyses a selected region or the first 600 seconds, lists BPM candidates with
normalized fitness, and **requires the user to apply one**. Nothing changes until they do. Its
source comments report the correct BPM within the top three for 300 cases — note that "top three"
is an admission that the top result is not always right, which is precisely why the list exists.

**Quaver** also ships automatic BPM and offset detection.

**Tempora** sits at the other extreme: its osu export silently rewrites timing with no report.

## Open decision

- Candidate list requiring explicit apply, as ArrowVortex does
- Apply the best candidate with an easy undo
- Apply silently

Also open: whether the same pattern governs other assistive features (stream generation, difficulty
estimation, snap-to-transient), and whether a suggestion that was applied stays marked as
machine-generated afterwards.

## Evidence

ArrowVortex's behaviour is **observed**, and its top-three accuracy figure is the best evidence in
the whole research set that suggestions need human confirmation. Earlier research concluded
"do not silently apply suggestions", grade **inferred**, but from unusually direct evidence.

Related: [beat detection](beat-detection.md) produces the suggestions;
[export loss reporting](export-loss-reporting.md) is the same consent question on the way out.
