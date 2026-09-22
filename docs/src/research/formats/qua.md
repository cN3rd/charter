---
type: Format
title: .qua
description: Quaver's YAML map format, rich in scroll velocity, timing groups and per-object keysounds.
resource: https://raw.githubusercontent.com/Quaver/Quaver.API/master/Quaver.API/Maps/Qua.cs
tags: [format, quaver]
status: draft
---

# .qua

YAML-serialized. Carries metadata and assets, preview time, map IDs, mode, legacy long-note
rendering, normalized scroll velocity, initial scroll velocity, scratch key, layers, bookmarks,
custom audio samples, sound effects, timing points, slider velocities, scroll-speed factors, hit
objects, timing groups, mines, and per-object keysounds.

Quaver's model is `.qua`-first. Its converters to and from other formats do not establish full
round-trip fidelity.
