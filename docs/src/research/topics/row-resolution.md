---
type: Open Decision
title: Row resolution
description: What time base the chart model uses, given that StepMania quantizes at 48 rows per beat and .chart at 192 ticks.
tags: [topic, data-model, timing, open]
status: draft
---

# Row resolution

## Observed

- StepMania stores per-track rows with `ROWS_PER_BEAT = 48`.
- `.chart` resolution is configurable and conventionally **192 ticks/beat**; Moonscraper holds
  absolute `uint tick` positions in sorted flat lists.
- Tempora avoids integers entirely, using float measure positions.
- ArrowVortex offers snaps up to 192nd and accepts custom snaps from 4 through 192.

## Open decision

| Option | Consequence |
|---|---|
| 48 rows/beat | Matches StepMania exactly; cannot represent `.chart` ticks without loss |
| 192 ticks/beat | Matches `.chart`; 48 divides into it cleanly |
| A higher common multiple | Holds both; larger numbers, more overflow surface |
| Rational or float offsets | Holds anything; invites drift and comparison bugs |
| Integer rows plus an unquantized escape hatch | Holds both; two code paths |

Note that 192 is divisible by 48, which makes one direction of that pair cheap and the other
lossy.

## Evidence

Earlier research assumed 48 throughout, inherited from StepMania rather than chosen. The
Moonscraper study separately flags that 48 may be too coarse for dense or deliberately unquantized
charts and names 192 and float offsets as alternatives, without resolving it.

Grade: **observed** for both constants; the preference for 48 is **asserted**.

## Unresolved

Whether 48 is sufficient for dense or deliberately unquantized charts.
