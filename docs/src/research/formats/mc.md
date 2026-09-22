---
type: Format
title: .mc
description: Malody's JSON chart format, reachable through Quaver's converter.
resource: https://raw.githubusercontent.com/Quaver/Quaver.API/master/Quaver.API.Tests/Malody/Resources/1574436578.mc
tags: [format, malody]
status: draft
---

# .mc

JSON with top-level `meta`, `time`, `effect` and `note` arrays. Notes use `beat`, an optional
`endbeat`, and `column`.

Quaver converts `.mc` to `.qua` and has tests covering scroll-velocity effect conversion.

The surveyed sources establish the structure but not the current Malody editor workflow.
