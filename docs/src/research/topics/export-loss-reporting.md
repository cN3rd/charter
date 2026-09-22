---
type: Open Decision
title: Export loss reporting
description: How the editor tells the user what a conversion changed or dropped.
tags: [topic, ux, interop, open]
status: draft
---

# Export loss reporting

## Observed

Tempora's osu export performs at least four silent transformations: removing no-op points, adding
downbeat or quarter-note points, resetting unsupported signatures, and adjusting BPMs for
millisecond rounding and barline behaviour. Its Beat Saber export inserts a point at beat 0 because
Beat Saber has no equivalent offset.

ArrowVortex drops keysounds on SM load with a source-level warning only.

Neither surfaces any of this to the user at the moment it happens.

## Open decision

- Warnings at export time
- A structured loss report, with an empty report required for same-format round trips
- A dedicated transformations panel showing what an adapter will do before it does it

The second is the only one that is mechanically testable.

## Evidence

The silent transformations are **observed** in shipped tools. The reporting requirement is
**inferred** — earlier research proposed that adapters report explicit transformations rather than
silently mutate, based on Tempora's export behaviour.

Related: [round-trip fidelity](round-trip-fidelity.md) is the policy;
[interoperability](../formats/interoperability.md) is why loss is unavoidable in the first place.
