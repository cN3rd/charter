---
type: Open Decision
title: GUI framework
description: Which Rust UI framework renders a scrolling notefield and waveform, with no benchmark yet run.
tags: [topic, stack, gui, open]
status: draft
---

# GUI framework

## Candidates

| Framework | Model | Custom drawing | Noted drawbacks |
|---|---|---|---|
| egui | Immediate, rebuilt every frame, `wgpu` or `glow` | `Painter` for lines, shapes, meshes; `PaintCallback` for raw `wgpu` | High CPU/GPU during continuous animation; fewer styling controls |
| iced | Elm-style, `wgpu` or `tiny-skia` | Canvas API, caches geometry until state changes | Needs explicit timer ticks, redraw management, caching, state wiring |
| Slint | Declarative `.slint` compiled to Rust; Skia, OpenGL or software | `DrawingArea` with `RenderCallback` or raw buffers | Rust/markup boundary adds work for dense custom rendering |
| GPUI | Retained, GPU-first, used by Zed | Direct GPU buffer output | macOS-first; Linux and Windows catching up; documentation gaps |
| Freya | Dioxus-style over Skia via `rust-skia` | Skia | Experimental |
| Dioxus / Tauri | WebView, HTML5 canvas or WebGL | Canvas or WebGL | WebView and bridge latency |
| `winit` + `wgpu` | Roll your own | Everything | Text, input routing, file dialogs and layout become project code |

## Open decision

Which to build on. The requirement shape is unusual for a GUI toolkit: a scrolling note timeline
and waveform redrawn continuously at 60 fps, keyboard-first input, and a dense custom canvas rather
than forms.

An immediate-mode framework rebuilding every frame is arguably a natural fit for a view that
changes every frame anyway; that is the case for egui, and it is reasoning rather than measurement.

## Evidence

Grade: **asserted**. The source document states plainly that its assessments came from reading
project documentation and API surfaces, **not from benchmarking**, and that its rating matrix is
author assessment.

Earlier research ranked egui top, iced runner-up, GPUI wildcard, and called Dioxus/Tauri and
`winit` + `wgpu` disqualified.

The decisive spike was never run: waveform and spectrogram rendering at 60 fps with tens of
thousands of line segments and texture strips, to determine whether `Painter` suffices or a `wgpu`
callback is needed. Until that exists, this decision rests on reading documentation.

Related: [waveform rendering](waveform-rendering.md),
[performance targets](performance-targets.md).

## Draft position

egui, through eframe 0.36 and its default `wgpu` renderer, draws `crates/charter-ui`: a
read-only notefield over real documents, and a static mock of the [UI draft](../../ui.md). Each
draws about a thousand shapes where the spike asks for tens of thousands, and neither animates,
so the spike is still unrun.
