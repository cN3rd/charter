---
type: Open Decision
title: Shortcuts
description: The keybinding system, its scoping, and how far configurability goes.
tags: [topic, ux, input, open]
status: draft
---

# Shortcuts

## Observed

**All four major tools ship customizable shortcuts**: ArrowVortex, StepMania
(`EditMode_Keymaps.ini`), Quaver and osu!. This is the strongest consensus in the surveyed set.

StepMania's documentation warns its own mapping is complicated, that Alt and Shift are heavily
overloaded, and that digits break down above ten columns.

Tempora concentrates meaning into modifiers instead: plain drag, Ctrl-drag, Shift-drag and Alt-drag
each do something different to the same object, as do Ctrl-wheel, Shift-Ctrl-wheel and Alt-wheel.
Powerful once learned, invisible until then.

## Default bindings

Read from each tool's shipped default table in source, which is the strongest evidence for a
default: ArrowVortex `bin/settings/shortcuts.txt` (uvcat7 fork), StepMania 5 `src/ScreenEdit.cpp`,
Moonscraper `Assets/Database/DefaultControls.json`, Editor on Fire `bin/keys.txt`, osu!lazer
`GlobalActionContainer.cs` and osu-framework `GameHost.cs`, and Quaver `EditorInputConfig.cs`. All
**observed** unless marked.

| Action | Consensus | Outliers |
|---|---|---|
| Play and pause | Space, in all six | StepMania also Ctrl+P; osu! C |
| Undo | Ctrl+Z, five of six | StepMania Ctrl+U |
| Redo | Ctrl+Y and Ctrl+Shift+Z, both bound in osu!lazer | EOF Ctrl+R; StepMania none |
| Select all, delete | Ctrl+A; Delete | StepMania has no select all; Quaver adds Backspace |
| Cut, copy, paste | Ctrl+X, C, V, five of six | ArrowVortex Ctrl+Shift+V pastes without replacing |
| Lane keys | 1 to 9 and 0: ArrowVortex, StepMania, Quaver | osu!lazer places with the mouse only |
| Mine | Shift+digit in ArrowVortex | Quaver Alt+digit swaps type |
| Cursor by snap, by measure | Up and Down; Page Up and Page Down | osu! seeks with Left and Right |
| Snap | Left and Right in ArrowVortex | osu! Ctrl+wheel; EOF comma and period |
| Zoom | Ctrl+wheel and numpad plus and minus in ArrowVortex | Quaver Page Up and Down |
| Start and end | Home and End: ArrowVortex, StepMania, Quaver | EOF Ctrl+Home; osu! Z and V |
| Next and previous section | Alt+Up and Alt+Down in Moonscraper | Quaver Ctrl+Left and Right; osu! Alt+Left and Right for bookmarks |
| Bookmark | Ctrl+B in osu! and Quaver | EOF Ctrl+numpad |
| BPM at the cursor | F7 and F8 lower and raise it in StepMania | ArrowVortex Ctrl+Alt+Q opens it |
| Playback rate | Shift+Left and Right in ArrowVortex | osu!stable Ctrl+Up and Down; Quaver plus and minus |
| Metronome, note ticks | F3 and F4 in ArrowVortex | Moonscraper M and N; Quaver Ctrl+9 and Ctrl+8 |
| New, open, save, save as | Ctrl+N, O, S, Shift+S, all four only in Moonscraper | Ctrl+S is in all six |
| Shortcut list | F1: ArrowVortex, StepMania, EOF | |

No tool binds Ctrl+Comma to settings; charter takes it from desktop convention, **inferred**.
Ctrl+Up and Down mean something different in every tool that binds them, so charter leaves them
free.

## Draft position

charter binds the consensus where one exists and ArrowVortex's choice where the VSRG editors split,
since its charts are lane-based like ArrowVortex's. Bound today: lane keys and Shift for mines, Up
and Down, Page Up and Down, Left and Right for the snap, Home and End, Alt+Up and Alt+Down between
sections, Ctrl+B for a bookmark, F7 and F8 for the tempo at the cursor, Ctrl+Z, Ctrl+Shift+Z and
Ctrl+Y, Ctrl+A, Escape, Delete and Backspace, Ctrl+N, O, S and Shift+S, Ctrl+Comma and F1, and for
playback Space, Shift+Left and Right with Ctrl+Shift for finer steps, and F3 and F4 for the
metronome and note ticks. Cut, copy and paste wait for a clipboard. Bindings are fixed for now; the
open questions below still stand.

## Open decision

- **Scoping.** Whether bindings resolve per context (global, notefield, sync, record, playback,
  dialog) or globally. Context scoping allows reuse of scarce keys; it also makes "what does this
  key do" context-dependent.
- **Lane bindings.** Generated per ruleset, or configured per layout. See
  [wide layouts](wide-layouts.md).
- **Conflict handling.** Whether modifier-shadow conflicts are detected and rejected.
- **Discoverability.** Whether a command palette or equivalent exposes actions that have no
  visible affordance, which is Tempora's main weakness.
- **Presets, import and export.**

## Evidence

Customizability across the surveyed tools is **observed**. Earlier research proposed a full system
of stable named actions with scoped resolution, conflict detection, generated lane bindings,
required presets and above-ten-lane validation, grade **asserted** for the complete system.

Its claim that "this audience expects customizable shortcuts" is **inferred** from the four tools,
which is reasonable but is evidence about tools rather than about users.
