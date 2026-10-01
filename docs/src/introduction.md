---
type: Overview
title: Introduction
description: What charter is, the games it charts, and where to start reading.
tags: [guide, overview]
status: draft
---

# Introduction

Charter is an editor for rhythm-game charts: the notes a player hits in time with a song. One
editor serves several games, each through a ruleset plugin that knows its notes and rules. It is
in the same family as ArrowVortex, Moonscraper, Editor on Fire and the editors built into
StepMania and osu!.

It ships with these games:

| Game | Charts | Notes |
|---|---|---|
| StepMania | its styles, such as dance-single and pump-double | taps, holds, rolls, mines, lifts, fakes |
| Tap Tap Revenge | 3, 4 or 5 lanes, in levels from Easy to Extreme | taps, holds, shakes, slices, mega taps |
| osu!mania | 1 to 10 keys, or an even count up to 20 | taps, holds |

A song keeps every chart for its game in one JSON file beside its audio, in charter's own
[format](format.md). Tap Tap Revenge tracks and Editor on Fire MIDI files import and export, and
osu! timing imports into any song.

Charter does some things the editors it follows do not:

- Every edit is checked against the game's rules as it is made, so a chart is always one the game
  can play. An edit that breaks a rule is refused with the reason.
- Every timing point stays at its moment of the audio. Changing a BPM moves the grid between
  points and leaves the later points where they sound.
- Two models find a song's timing from its audio: Beat This!, which runs on any computer, and
  Mapperatorinator, which wants a graphics card.
- The timing window lays the audio out in rows and measures the grid against the onsets it hears.
- Notes can be split into layers, such as one per instrument, that show, hide and lock on their
  own.
- Playback slows down to a tenth of the song's speed and keeps the pitch down to that, while the
  metronome and note ticks stay crisp.
- Unsaved changes are autosaved, a crash leaves a report, and dated snapshots of a song can be
  kept in a folder of their own.

The [user guide](guide/index.md) explains how to use the editor. The chapters after it record the
design: the chart format, the ruleset plugins, the window's layout and the research behind them.
