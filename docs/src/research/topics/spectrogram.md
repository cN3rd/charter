---
type: Decision
title: Spectrogram
description: Whether frequency-domain display is core or later, and how it is produced.
tags: [topic, stack, rendering, audio]
status: draft
---

# Spectrogram

## Observed

Two surveyed tools ship it: **Quaver** and **Tempora**. Tempora added it in `v1.5.0-beta` and
renders it per timing segment, like its waveform.

ArrowVortex does not have one, and instead offers filtered waveform views (high and low-pass with
an overlay), which serve a related purpose: seeing which frequency band an onset lives in.

Available crates: `rustfft` for FFT primitives, `realfft` for real-to-complex wrappers described as
roughly 2x faster for real audio, and `sonogram` `0.7.3` for ready-made STFT spectrograms to PNG,
grayscale or buffers. All MIT/Apache-2.0.

## Decided

The notefield's audio strip shows either the waveform or the spectrum, a Display setting
chooses. `charter-audio`'s `Spectrum` builds the STFT on `rustfft`, already a dependency, with a
2048-sample Hann window every 5 ms, summed into 48 bands spaced evenly in pitch from 30 Hz to
16 kHz and scaled over 80 dB below the track's loudest. It replaces the waveform in the strip
rather than sitting beside it, and filtered waveform views were not added.

## Was open

- Core feature or later
- `sonogram` off the shelf, or STFT built directly on `realfft` for control over windowing and
  scaling
- Whether it replaces or complements filtered waveform views

A spectrogram is more useful than a waveform for finding onsets in dense mixes, which is exactly
when sync is hardest. That argues for core; its rendering cost argues the other way.

## Evidence

Presence in two tools is **observed**. Whether it belongs in a first release is **unresolved** —
earlier research called it a differentiator, grade **asserted**, and separately listed spectrogram
throughput among the untested spikes.
