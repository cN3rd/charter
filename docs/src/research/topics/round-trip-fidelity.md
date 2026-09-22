---
type: Open Decision
title: Round-trip fidelity
description: Whether conversion must preserve everything, or may lose data with a report.
tags: [topic, scope, formats, interop, open]
status: draft
---

# Round-trip fidelity

## Observed

Two shipped tools lose data silently:

- **ArrowVortex** ignores keysounds on SM load while its saver writes `#KEYSOUNDS`. Opening and
  saving a keysounded file through it drops them, with a warning in source but not to the user.
- **Tempora** rewrites timing on osu export: removes no-op points, adds downbeat points, resets
  unsupported signatures, adjusts BPMs for millisecond rounding.

One tool does not:

- **Editor on Fire** retains unknown MIDI tracks in a raw linked list and writes them back on
  export.

## Open decision

- Guarantee preservation of unsupported fields, refusing conversions that cannot honour it
- Permit lossy conversion with warnings
- Structured loss reports, with an empty report required for a same-format round trip

The third is testable, which the first two are not: a same-format round trip producing a non-empty
report is a failing test.

## Evidence

Earlier research leaned to structured loss reports, reasoning from ArrowVortex's silent keysound
drop. The failure mode is **observed**; the proposed remedy is **inferred**.

## Unresolved

Whether EOF's unknown-track passthrough generalizes beyond `.mid`. Complete conversion semantics
among `.ssc`, `.sm`, `.osu`, `.qua`, `.chart`, `.mid` and `.mc` are not established.

## Draft position

Provisionally addressed: verbatim extension passthrough makes an empty-loss-report test possible. See [extensions](../../format.md#extensions).
