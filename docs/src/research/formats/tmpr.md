---
type: Format
title: .tmpr
description: Tempora's sync project file, storing anchors as paired audio time and musical position.
tags: [format, sync, timing]
status: draft
---

# .tmpr

Contains an audio path, time-signature points, and timing-point lines:

```
Time;MeasurePosition;TimeSignatureUpper;TimeSignatureLower
```

The final point additionally stores `MeasuresPerSecond`.

Structurally interesting because it is a pure sync artifact: it carries the audio-to-music mapping
with no note data at all. See [sync anchors](../topics/sync-anchors.md).
