---
type: Open Decision
title: Guitar format support
description: Whether .chart and .mid are real targets or only a source of UX ideas.
tags: [topic, scope, formats, open]
status: draft
---

# Guitar format support

## Observed

`.chart` and `.mid` come with a different note vocabulary: HOPO, tap, open notes, sustains,
sliders, disjointed notes, drum cymbal and tom distinctions, plus phrases, vocals and lyric lines.
Editor on Fire needs three separate track domains to hold it.

`.chart` also encodes modifiers as pseudo-frets rather than flags, and uses 192 ticks/beat rather
than 48 rows.

Moonscraper's GHLive six-button two-row logic was explicitly noted as outside a 4-panel MVP scope.

## Open decision

- Real import and export targets
- Import only
- Neither; take only the event and section UX ideas

Supporting them pulls directly on [note type model](note-type-model.md),
[row resolution](row-resolution.md) and [target audience](target-audience.md). None of those can be
settled without knowing this one.

## Evidence

Explicitly unresolved.
