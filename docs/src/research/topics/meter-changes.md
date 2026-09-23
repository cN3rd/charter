---
type: Open Decision
title: Meter changes
description: What happens to downstream timing when a time signature changes.
tags: [topic, timing, data-model, open]
status: draft
---

# Meter changes

## Observed

Tempora keeps time signatures as separate `TimeSignaturePoint` data keyed by measure, defaulting to
4/4, and looks up the last point at or before the queried measure. **A meter change shifts
downstream timing-point musical positions to preserve BPM and beat count.**

`.ssc` carries `#TIMESIGNATURES` as first-class timing data. Editor on Fire uses anchors
specifically for denominator-changing time signatures.

## Open decision

When the meter changes, one of three things must give:

| Policy | Preserved | Broken |
|---|---|---|
| Preserve measure labels | Measure numbering downstream | Beat count, audio alignment |
| Preserve beat count | Musical duration and BPM | Measure numbering |
| Preserve audio offsets | Everything's wall-clock position | Musical position of every later anchor |

Tempora chose the second. The choice is not neutral and cannot be deferred to the UI: it determines
what the data means.

## Evidence

Tempora's behaviour is **observed**. Which policy charter should expose is **unresolved**, and
earlier research raised it as a design concern without settling it.

## Draft position

The format takes no position; all three policies are representable. See [measures](../../format.md#measures).
