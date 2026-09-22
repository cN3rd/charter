---
type: Open Decision
title: Undo model
description: What one undo step covers, and whether it restores view state as well as data.
tags: [topic, architecture, ux, open]
status: draft
---

# Undo model

## Observed

- ArrowVortex uses a `History` system with chained multi-object operations and no cited depth
  limit. Delete removes selected notes and tempo boxes atomically, so one step spans two kinds of
  data.
- Moonscraper's `CommandStack` stores edit commands **plus camera and selection snapshots**, so
  undo returns the view to where the edit happened, not just the data to its prior state.
- StepMania exposes undo as a single binding within edit mode.

## Open decision

- Data-only undo
- Data plus view restoration, as Moonscraper does
- Data plus view, with view restoration suppressible

Separately: what constitutes one step. A recording pass producing hundreds of notes is the hard
case — see [live recording](live-recording.md).

## Evidence

Earlier research proposed transactional undo for every edit, and that a recording pass be one
undoable transaction. Moonscraper's view-restoring undo is **observed** and unusually thoughtful;
the transactional requirement is **asserted**.
