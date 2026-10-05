---
type: Open Decision
title: Beat detection
description: Automatic BPM and offset detection, where licensing rules out the obvious library.
tags: [topic, stack, dsp, audio, open]
status: draft
---

# Beat detection

## The problem

Automatic BPM and offset detection appears in **ArrowVortex and Quaver**, and is the single
highest-leverage assistive feature in a charting tool: sync is the first task on every song and the
one most likely to be wrong.

ArrowVortex's implementation is measurable prior art. It uses **aubio's `complex` onset detector at
256 window and 1024 buffer**, searches candidates from 89 to 205 BPM, and its source comments report
the **correct BPM within the top three for 300 cases**.

## The licensing wall

**aubio is GPL-3.0.** The `aubio-rs` bindings are MIT, which does not change that, and those
bindings have a reported maintenance gap of 3+ years. Of the rest, `bliss-audio` is GPL-3.0 and
targets playlist similarity rather than beat grids, `essentia` is AGPL or commercial, and `madmom`
is Python.

There is no maintained, permissively licensed, pure-Rust beat tracker in the surveyed set. This is
the clearest negative finding in the whole research corpus.

## Open decision

- Build on `rustfft` / `realfft`: spectral flux onset detection plus autocorrelation tempo
- Ship aubio in an optional [plugin](plugins.md) or separate process, keeping GPL out of the binary
- Defer automatic detection entirely and ship manual sync only
- Licence the product compatibly with GPL

## Evidence

The elimination of existing options is **observed** and well grounded. The conclusion to build one
is **inferred** and reasonable given that.

The claim that a project-built detector will be **accurate enough** is **asserted**. The spike that
would test it was never run: implement spectral flux and autocorrelation, then measure against a
labeled corpus, targeting correct BPM modulo octave and offset within roughly +/-10 ms.

ArrowVortex's top-three figure sets the bar, and shows that even a mature implementation does not
reliably produce one right answer. See [suggestion UI](suggestion-ui.md).

Related constraint: [licensing](../constraints/licensing.md).
