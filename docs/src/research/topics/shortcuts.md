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
