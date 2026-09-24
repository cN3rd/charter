---
type: Design Note
title: Ruleset plugins
description: How the built-in rulesets read and write documents, and the vocabulary each one owns.
tags: [plugins, rulesets, format, draft]
status: draft
---

# Ruleset plugins

A ruleset plugin is a crate under `plugins/` that turns a `ValidDocument` into the game's typed
model and back. It depends on `charter-core` alone, the surface a third-party plugin would use;
how plugins are loaded is still [open](research/topics/plugins.md).

- Reading fails on anything the game cannot represent, such as a stop in osu!mania, a keysound
  in Tap Tap Revenge, or a key in the ruleset's own namespace that it does not define.
- Data other plugins left behind is skipped, since their keys and `other` events are theirs to
  read.
- Writing produces a validated document holding the ruleset's own data. The typed model is a
  view, so reading and writing back drops other plugins' data.
- Every ruleset keeps the song's markers of the shared kinds, `section` and `bookmark`, and none
  defines a kind of its own. StepMania's `#LABELS` are section markers.
- A song is checked when built, by reading or by `Song::new`, against the ruleset and every
  document rule, and cannot change after. Writing one therefore cannot fail.

| Ruleset | Namespace | Kinds | Timing | Chart settings |
|---|---|---|---|---|
| StepMania | `sm` | `tap`, `hold`, `roll`, `mine`, `lift`, `fake`, `sm.auto_keysound` | Every core kind, `sm.tickcount`, `sm.combo`, `sm.speed`, `sm.scroll`, `sm.fake` | `style`, `difficulty`, `meter`, `description` |
| Tap Tap Revenge | `ttr` | `tap`, `hold`, `ttr.shake`, `ttr.slice`, `ttr.mega_tap` | `bpm`, `time_signature` | None |
| osu!mania | `osu_mania` | `tap`, `hold` | `bpm`, `time_signature` over 4, `osu_mania.scroll_speed` | `hp`, `od` |

Each ruleset fixes lanes its own way:

- StepMania takes the style's column count from its own steps-type table. Routine styles, where
  two players share the panels, get a full set per player, player 1 first.
- Tap Tap Revenge songs have 3, 4 or 5 lanes in every chart. A shake or slice sits in the lane
  of its direction: left is lane 0 and right the last lane, and a down shake takes the middle
  lane, or both middle lanes with four. Slices go left or right only. A mega tap covers every
  lane. Notes scroll at a constant speed, so a BPM change moves only the beat grid.
- osu!mania takes the key count: 1 to 10, or an even count up to 20, which osu!lazer splits
  across two stages.

Each plugin's vocabulary is an enum in Rust and text only in the document: StepMania's styles and
difficulties, Tap Tap Revenge's lane counts and note kinds, and osu!mania's key counts.
