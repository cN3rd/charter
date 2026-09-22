---
type: Editor Profile
title: StepMania built-in editor
description: The in-game editor that established the 48 rows/beat model and the digit-key column mapping.
resource: https://github.com/stepmania/stepmania
tags: [editor, stepmania, prior-art]
status: draft
sources:
  - id: screenedit
    resource: https://raw.githubusercontent.com/stepmania/stepmania/5_1-new/src/ScreenEdit.cpp
    title: ScreenEdit.cpp
  - id: keymap
    resource: https://raw.githubusercontent.com/stepmania/stepmania/5_1-new/Docs/Mapping_keys_for_edit_mode.txt
    title: Mapping keys for edit mode
  - id: notetypes
    resource: https://raw.githubusercontent.com/stepmania/stepmania/5_1-new/src/NoteTypes.h
    title: NoteTypes.h
---

# StepMania built-in editor

Part of the game (`ScreenEdit.cpp`) rather than a separate authoring product. Windows, Linux and
macOS.

## Behaviour

Edit, play, record and record-paused modes. Autosaves every five minutes. Places notes by column;
edits BPM, stops, delays, offset and sample timing; switches charts; supports undo; plays or
records from cursor or selection. Custom key maps through `EditMode_Keymaps.ini`.

## Data model

Per-track rows with `ROWS_PER_BEAT = 48`. Canonical note symbols: `0` empty, `1` tap, `2` hold
head, `4` roll head, `M` mine, `L` lift, `A` attack, `K` auto-keysound, `F` fake.

## Documented limitations

The mapping documentation states that key mapping is complicated, that the default digit mapping
exposes ten columns and becomes awkward beyond ten, that rolls require Shift, and that Alt and
Shift are heavily overloaded. This is the tool's own documentation, not an outside complaint.

See [wide layouts](../topics/wide-layouts.md).
