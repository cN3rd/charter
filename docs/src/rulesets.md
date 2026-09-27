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

Each plugin also exports `Plugin`, which implements `charter-core`'s `RulesetPlugin`: `id` is the
ruleset id its documents carry, and `check` reads a document as a song and keeps only whether
that worked. The editor opens a song for editing only when the plugin for its ruleset accepts it,
and checks every edit, undo and redo through the same plugin, so an edit the game cannot represent
is refused. Without its plugin a song is read-only. `charter-app` compiles the built-in plugins in,
and nothing loads a plugin at runtime yet.

A plugin can also start songs. `name` is the game's name, `layouts` lists the lane layouts a new
song can have, the usual one first, and `layout_kind` says what the game calls one. `new_song`
makes a song with no notes from metadata, a layout and chart names, with every chart on that
layout, at 120 BPM in 4/4. StepMania's layouts are its styles, and a chart whose name spells a
difficulty, such as `Hard`, gets it; others are `edit`, all at meter 1. osu!mania's are its key
counts, 4K first, with HP and OD 5, and Tap Tap Revenge's are its 3, 4 and 5 lanes. A plugin
without layouts cannot start songs.

A game whose charts come in standard levels says so with `difficulties`: the levels, easiest
first, and presets that each pick some of them and name their charts. A new song's chart then
carries its level by index beside its name, so the name can be what the game calls it while the
level stays known. Tap Tap Revenge's levels are Easy, Medium, Hard and Extreme, kept as a chart's
`difficulty` setting (`easy` to `extreme`). Its presets are the two shapes custom charts take,
standard Extreme alone or every level, and the renamings of two of its games: Tap Tap Reloaded
calls Medium, Hard and Extreme `Metallica`, `Downloads` and `Customs` and has no Easy, and Tap
Tap Hardcore calls the four `Extreme`, `Intense`, `Severe` and `Customs`.

A plugin can also read another program's timing: `timing_format` names the format, and
`import_timing` turns text in it into a `Timing` for a given song, with a list of what it left
out. Any song can take timing from any plugin's format, so a plugin keeps its own timing events,
such as osu!mania's scroll speeds, only for songs of its game. osu!mania reads osu!'s
`[TimingPoints]`, pasted alone or in a whole `.osu` file.

| Ruleset | Namespace | Kinds | Timing | Chart settings |
|---|---|---|---|---|
| StepMania | `sm` | `tap`, `hold`, `roll`, `mine`, `lift`, `fake`, `sm.auto_keysound` | Every core kind, `sm.tickcount`, `sm.combo`, `sm.speed`, `sm.scroll`, `sm.fake` | `style`, `difficulty`, `meter`, `description` |
| Tap Tap Revenge | `ttr` | `tap`, `hold`, `ttr.shake`, `ttr.slice`, `ttr.mega_tap` | `bpm`, `time_signature` | `difficulty`, optional |
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
difficulties, Tap Tap Revenge's lane counts, levels and note kinds, and osu!mania's key counts.
