---
type: Open Decision
title: egui crates
description: Which published crates around egui 0.36 meet the editor's needs, which needs egui meets itself, and what no crate draws.
tags: [topic, stack, gui, dependencies, open]
status: draft
sources:
  - id: crates-io
    resource: https://crates.io/api/v1/crates
    title: crates.io API, versions, dependencies and downloads read on 2026-09-25
  - id: egui-changelog
    resource: https://github.com/emilk/egui/blob/main/CHANGELOG.md
    title: egui changelog
  - id: egui-wiki-crates
    resource: https://github.com/emilk/egui/wiki/3rd-party-egui-crates
    title: egui wiki, 3rd party egui crates
  - id: egui-mcp
    resource: https://github.com/rerun-io/kittest_inspector/tree/main/crates/egui_mcp
    title: egui_mcp README
  - id: osu-file-format
    resource: https://osu.ppy.sh/wiki/en/Client/File_formats/osu_(file_format)
    title: osu! wiki, .osu file format
---

# egui crates

Crates published around egui 0.36 that meet a need of the [UI draft](../../ui.md) or of another
topic, as crates.io listed them on 2026-09-25. None has been tried in `charter-ui`.

## Observed

### Lockstep with egui

Each egui minor release breaks the API, and a widget built against one minor cannot draw into a
`Ui` of another, so every egui dependency has to release for a minor before charter can move to
it. Recent minors shipped on 2025-10-09 (0.33), 2026-03-26 (0.34), 2026-06-25 (0.35) and
2026-08-05 (0.36). Days from each to a crate's first release for it:

| Crate | 0.34 | 0.35 | 0.36 |
|---|---|---|---|
| egui_plot, egui_table, egui_commonmark | 0 | 1 | 0 |
| egui_tiles | 1 | 1 | 0 |
| egui_dnd, egui_flex, egui_inbox, egui_virtual_list, egui_material_icons | 0 | 1 | 1 |
| egui_dock | 3 | 2 | 0 |
| egui-file-dialog | 1 | 1 | 3 |
| egui-toast | 0 | 13 | 3 |
| egui_json_tree | 1 | 2 | 10 |
| egui-system-fonts | 10 | 22 | 10 |
| egui-probe | 5 | 4 | 21 |
| egui-notify | 1 | skipped | 30 |
| egui-phosphor | 4 | 27 | 36 |
| egui-palette | first release | 30 | none after 51 |

The fastest rows are emilk's egui_plot, Rerun's egui_tiles and egui_table, and the hello_egui
crates of lucasmerlin, who wrote egui's `Modal`, `Popup` and inspection protocol. egui-keybind and
puffin_egui have no release past 0.33, catppuccin-egui and egui-modal none past 0.30, and
egui_hotkey none past 0.19.

### Built into egui and eframe 0.36

| Need | Built in |
|---|---|
| Dialogs: new song, export, detection, preferences | `egui::Modal`, which blocks input to the rest of the window |
| Rebindable keys, shown in menus | `KeyboardShortcut` serializes with the `serde` feature and formats itself per platform; `InputState::consume_shortcut` matches it; `Button::shortcut_text` shows it |
| Undo inside a form or text field | `egui::util::undoer::Undoer`, which snapshots the state once it has held still for a second |
| Long uniform lists: problems, timing events | `ScrollArea::show_rows`, `egui_extras::TableBuilder` |
| Recent songs, window and panel sizes, the keymap | eframe's `persistence` feature and `App::save` |
| Song banners and jackets | `egui_extras` image loaders |
| Fonts past the defaults | `FontDefinitions`, `Context::add_font` |
| Raw GPU drawing in a `Ui`, if `Painter` falls short for the waveform | `egui_wgpu::CallbackTrait` |
| Profiling | egui calls the `profiling` crate, so a Tracy or puffin backend chosen by the app sees egui's own work |
| Rendering in tests | `egui_kittest`'s `wgpu` feature renders a frame to an image in memory; its `snapshot` feature compares against PNG files kept in the crate, and charter's tests never touch files |
| An agent driving the running editor | eframe's `inspection` feature serves the `egui_inspection` protocol on 127.0.0.1:5719 when the app starts with `EGUI_INSPECTION=1`, and is off otherwise |

The default fonts are Ubuntu Light, Hack and two emoji fonts, with no CJK glyphs. osu! files keep
a romanised `Title` beside `TitleUnicode`, so titles outside Latin script are expected input.

### Crates that fit

| Need | Crate | Fit |
|---|---|---|
| Open and save dialogs | rfd 0.17.2 | Native dialogs, through the XDG desktop portal on Linux by default. It has no egui dependency, so no lockstep, and egui_kittest cannot reach it |
| | egui-file-dialog 0.15.0 | Drawn in egui, with keyboard navigation, filters and pinned folders. Its virtual file system would let tests drive it without a disk |
| Icons: transport, snap, metronome, record | egui-phosphor 0.14.0 | Phosphor icons as a fallback font, with a `subset` feature to ship only the icons used |
| | egui_material_icons 0.8.0 | Material icons, from the hello_egui crates |
| Titles in CJK script | egui-system-fonts 0.36.0 | Adds system fonts as fallbacks, chosen by locale; 2,300 downloads in 90 days |
| | fontdb 0.24.0 | No egui dependency: finds installed fonts, whose bytes then go to `Context::add_font` |
| Command palette | egui-palette 0.6.0 | A modal palette with a fuzzy filter from nucleo-matcher and keyboard navigation; still on egui 0.35 |
| Plugin `ext` data, read-only | egui_json_tree 0.17.0 | Collapsible JSON with search and highlighting, over serde_json, which charter already uses |
| Detection candidates and onset curves ([suggestion UI](suggestion-ui.md)) | egui_plot 0.37.0 | Line, bar and point plots with pan and zoom |
| Two charts side by side | egui_tiles 0.17.1 | Rows, columns, grids and tabs, rearranged by dragging |
| | egui_dock 0.21.1 | Tabs and binary splits; tabs drag out into windows and have close buttons |
| Tables beyond `TableBuilder` | egui_table 0.10.0 | Sticky header and columns, millions of rows, rows of varying height |
| Audio loading and analysis on a thread, reporting to the UI | egui_inbox 0.13.0 | A channel that repaints the UI when a message arrives; its README points to `std::sync::mpsc` above thousands of messages a frame |
| Short-lived messages: saved, exported | egui-notify 0.23.0, egui-toast 0.22.0 | Toasts with a timeout |
| Preferences dialog | egui-probe 0.13.0 | Derives an editor for a settings struct |
| Agent checks of the running editor | egui_mcp 0.2.0 | An MCP server over `egui_inspection`: reads the AccessKit tree, clicks, types, presses keys, takes screenshots. Installed with `cargo install`, not a dependency |

### Nothing published

No crate draws a notefield, timeline, transport, waveform or spectrogram. Searching crates.io for
egui with timeline, waveform, spectrogram, piano roll, scrubber or ruler found animation
timelines; audio-visualizer, a debugging aid that renders PNG or SVG or opens its own window; and
egui-scrubber, still on egui 0.29 with 13 downloads in 90 days. These stay charter's own, and
[waveform rendering](waveform-rendering.md) leaves open whether they draw through `Painter` or a
wgpu callback.

[Shortcuts](shortcuts.md) proposes scoped bindings with conflict detection, and no egui crate
provides either. egui-keybind captures one shortcut in a widget and is still on 0.33.

### Outside egui

- `undo` 0.52.0: the command pattern, where each edit knows how to undo itself, with merging,
  checkpoints, a stack in `Record` and a tree in `History`. `Record::edit` applies an edit and
  keeps it whatever the edit returned, and `undo` and `redo` move on the same way, so it suits
  edits that cannot fail. Charter's can: each one builds a new document that the format and the
  ruleset plugin may refuse.
- `nucleo-matcher` 0.3.1: the Helix editor's fuzzy matcher, the one under egui-palette. MPL-2.0,
  the same weak copyleft as symphonia ([licensing](../constraints/licensing.md)).
- `keybinds` 0.2.0: parses bindings such as `Ctrl+x Ctrl+s` from a config file through serde and
  dispatches them to actions. It has integrations for winit and iced, not egui, so egui's key
  events would need converting.
- `gilrs` 0.11.2 and `midir` 0.11.0: game controller and MIDI input, if
  [live recording](live-recording.md) takes more than the keyboard.

## Open decision

- For each need: a crate, egui's built-in, or charter's own code.
- How much lockstep risk to take. One slow crate holds every egui upgrade back: egui-phosphor took
  27 and 36 days over the last two minors, and egui-palette has no 0.36 release 51 days on. The
  options are to prefer crates that release within days, to hold egui back until the slowest
  catches up, or to vendor a small crate under `third-party/` and port it.
- rfd or egui-file-dialog: native dialogs and portal access, against a dialog the tests can drive.

## Evidence

Release dates, egui requirements and downloads are **measured**, read from crates.io's API on
2026-09-25. The built-ins are **observed** in egui 0.36.2's source and changelog. Crate features
are what each README states, untried here, so **asserted**. Their fit to charter's needs is
**inferred**.

Related: [GUI framework](gui-framework.md), [performance targets](performance-targets.md).

## Draft position

- Tooling now: eframe's `inspection` feature with egui_mcp, so an agent can check the running
  editor, and `profiling` with a Tracy or puffin backend for the unrun
  [GUI framework](gui-framework.md) spike.
- In use: rfd for the open and save dialogs, which `charter-app` shows when `charter-ui` asks,
  and eframe's `persistence` feature, which keeps the settings between runs.
- A crate when the feature lands: egui-phosphor for icons, which retires the mock's drawn
  play button; a CJK fallback through egui-system-fonts or fontdb before the first non-Latin title;
  egui_json_tree for `ext`; egui_plot for detection; egui_tiles with a second view.
- Charter's own: the undo history, since its edits can be refused; the notefield and its columns,
  the waveform, spectrogram and overview; and the keymap's scopes and conflict checks over
  `KeyboardShortcut`, with keybinds as the fallback if key sequences or a text config file are
  wanted. The command palette goes on `Modal` with nucleo-matcher unless egui-palette releases for
  0.36 first.
