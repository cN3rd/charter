---
type: Format
title: .mid
description: MIDI as used by guitar-family editors, with a demonstrated precedent for lossless passthrough of unknown tracks.
tags: [format, midi, guitar-hero]
status: draft
---

# .mid

Parsed and written by Editor on Fire alongside `.chart` and `.sm`.

The notable precedent: EOF retains **unknown MIDI tracks in a raw linked list and writes them back
on export**. Data the editor does not understand survives a round trip untouched.

Whether that approach generalizes to the other target formats is unestablished; see
[round-trip fidelity](../topics/round-trip-fidelity.md).
