---
type: Open Decision
title: Note type model
description: Whether one enum can carry every note kind across the target games, or whether extensible flags are required.
tags: [topic, data-model, multi-game, open]
status: draft
---

# Note type model

## Observed

- StepMania's vocabulary: tap, hold head, roll head, mine, lift, attack, auto-keysound, fake.
- ArrowVortex adds nothing beyond that set, and reaches the variants through conversion commands
  rather than dedicated entry keys.
- `.chart` encodes modifiers as pseudo-frets: fret `5` is Forced, fret `6` is Tap, arriving as
  separate note records at the same tick.
- Editor on Fire models GH/RB legacy, vocal and Rocksmith Pro Guitar as **separate domains**, with
  flags such as `EOF_NOTE_FLAG_F_HOPO`, `EOF_PRO_GUITAR_NOTE_FLAG_TAP` and
  `EOF_NOTE_EFLAG_DISJOINTED`, plus HOPO, tap, open notes, sliders, disjointed notes, drum cymbal
  and tom distinctions, phrases, vocals and lyric lines.

## Open decision

- One flat enum covering every kind across all games
- A core enum plus extensible per-ruleset flags
- Separate per-domain models sharing only a timing core

## Evidence

Earlier research concluded a single `NoteType` enum "will not cover Clone Hero/RB support". The
supporting observation is solid: EOF genuinely needed three domains. Whether that means charter
needs them depends on which games it targets, which is itself open.

Grade: **observed** for the breadth; **inferred** for the conclusion.

## Unresolved

How generalized flags map across StepMania, Clone Hero/GH/RB, Rocksmith, vocals and future
rulesets. See [target audience](target-audience.md).

## Draft position

Provisionally addressed: `Kind` is a namespaced identifier, not an enum, because the core never branches on it. Closed vocabularies are re-typed at the plugin boundary. See [kinds](../../format.md#kinds).