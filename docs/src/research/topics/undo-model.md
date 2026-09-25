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

## Draft position

`charter-editor` keeps each edit with the chart and the cursor it was made at, and undo and redo
return to both: Moonscraper's view restoration, less a camera the editor does not have. One lane
key press is one step; stretching a note into a hold joins the step that placed it, and deleting a
selection is one step. An undone or redone edit is checked like a new one, by the format and the
ruleset plugin, since every document the editor holds has passed both. The
`undo` crate's `Record` does not fit that: it keeps an edit it has
applied whatever the edit returned, so a refused edit would stay on the record.
