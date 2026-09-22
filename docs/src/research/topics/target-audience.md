---
type: Open Decision
title: Target audience
description: Which charting community the editor serves first, given that multi-game support is the stated goal.
tags: [topic, scope, product, open]
status: draft
---

# Target audience

## Observed

The surveyed communities have materially different needs:

| Community | Distinguishing need |
|---|---|
| DDR/ITG pad | Panel layouts, foot-pattern transforms, stream generation |
| Etterna keyboard | Dense keyboard charts, difficulty calibration |
| osu!mania | Keysounds, hitsounds, per-object samples |
| Quaver | Scroll velocity, timing groups, layers |
| Clone Hero / RB | HOPO, taps, sustains, separate instrument domains |
| Pack authors | Batch operations across many files, format conversion |

Every surveyed tool serves one of these well. None serves all of them.

## Open decision

Which comes first. The answer drives [primary format](primary-format.md),
[note type model](note-type-model.md) and [wide layouts](wide-layouts.md), so it is arguably the
upstream decision for most of this section.

## Evidence

Explicitly unresolved. Earlier research called Etterna a target community "until charters validate
workflow", which is **inferred** from repository scope rather than from contact with charters.

Serving several games is the project's stated purpose, so "all of them eventually" is a given. The
open question is ordering, not ambition.
