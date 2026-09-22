---
type: Open Decision
title: Wide layouts
description: How to map and edit layouts with more than ten columns.
tags: [topic, ux, input, open]
status: draft
---

# Wide layouts

## Observed

- StepMania's default mapping exposes columns `1` through `0`, and **its own documentation says
  this becomes awkward above ten columns**. Rolls require Shift; Alt and Shift are heavily
  overloaded.
- ArrowVortex uses the same digits and reaches doubles by offsetting with Alt into the second side.
- Quaver uses home-row bindings instead: 4K `D F J K`, 7K `S D F Space J K L`.

Ten is not a design limit anywhere; it is the number of digit keys.

## Open decision

- Digit keys with a modifier for the second half, as ArrowVortex does
- Home-row style bindings generated per ruleset, as Quaver does
- Chorded entry
- Mouse-driven entry for wide layouts, keyboard for narrow

## Evidence

The problem is **observed** and documented by StepMania itself. Earlier research asserted charter
"must not inherit" it, which is a requirement rather than a finding.

Related: [shortcuts](shortcuts.md), where per-ruleset lane binding generation and
above-ten-lane validation are part of the same question.
