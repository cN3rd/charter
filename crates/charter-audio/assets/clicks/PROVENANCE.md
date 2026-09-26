# Provenance

This file explains where the sounds in this repository come from, so that
anyone auditing the project (a distribution packager, a licence reviewer, a
contributor) can see the whole chain.

## What the sounds are

Nine metronome click pairs (accent + beat), each **synthesised from scratch**
by a short Python script in this folder. Every WAV under `out/` is the output
of `python3 make_all.py`; regenerating them reproduces the files bit-for-bit
(the noise-based presets use fixed random seeds). The scripts are the
"preferred form for modification" of the audio.

## What they are modelled on

Each preset is modelled on the stock metronome click of a well-known
DAW/sampler:

| preset | modelled on |
|---|---|
| `tone_gate` | Ableton Live's "Classic" metronome |
| `soft_buzz` | Cubase's built-in "Steinberg Click" |
| `stick_click` | FL Studio's default "Click" |
| `filter_knock` | Logic Pro's Klopfgeist click at its default settings |
| `pure_sine` | the Maschine metronome |
| `ramp_tick` | the Akai MPC2000XL count-in click |
| `square_decay` | the Pro Tools "Classic Click" (and its "Marimba" preset) |
| `shaker_burst` | the Reason metronome |
| `bright_tick` | the SONAR (Cakewalk) audio metronome |

## How the recipes were made

1. Reference samples of those clicks were **analysed**: length, attack and
   decay times, pitch, harmonic/partial levels, peak level, and 1/3-octave
   spectra measured over successive time windows. The tools in `tools/`
   do this analysis and can re-run it against reference samples that the
   user supplies.
2. A **synthesis recipe** was written for each sound using generic
   building blocks — sines and harmonic series, damped sines, resonant
   filters on impulses, envelopes, spectrally shaped noise — with the
   measured numbers as parameters. Where a reference is a real recording
   (FL Studio, Reason, SONAR, the Cubase capture), the recipe uses freshly
   generated noise coloured by measured 1/3-octave level tables; nothing of
   the recording itself is used.
3. The recipes were compared to the references numerically (spectral and
   envelope distance) and by ear, and adjusted.

The measurements are facts about the reference sounds (a pitch, a decay time,
a level); the audio is generated anew from those facts. No reference audio,
and no fragment of one, is included in or embedded into this repository —
not in the WAVs, not in the scripts.

Two presets are idealised rather than exact: `square_decay` is a textbook
decaying square wave with the measured frequencies, start levels and per-half-
cycle ratio, and `ramp_tick` is a smooth one-millisecond ramp; neither traces
the reference's individual sample values.

## Where the references came from

The reference samples were the DAWs' own shipped metronome sounds, as
collected in a third-party pack ("Metronome Sounds Collection for Ableton
Live", Side Brain, 2016). That pack redistributes the vendors' files and is
**not** part of this repository; the `tools/` scripts expect the user to point
`METRONOME_PACK` at their own copy if they want to re-run the comparison.
The comparison tool writes its A/B reel (which contains reference audio)
only to `tools/plots/`, which is git-ignored.

Deliberately *not* reproduced: recording noise floors, DC offsets and mains
hum present in some reference files.

## Trademarks

Ableton, Live, Cubase, Steinberg, FL Studio, Logic Pro, Klopfgeist, Maschine,
MPC, Akai, Pro Tools, Avid, Reason, SONAR and Cakewalk are trademarks of their
respective owners. They are used here only descriptively, to say what each
preset is modelled on. This project is not affiliated with, endorsed by or
derived from any of those companies or their products. The preset and file
names in this repository are deliberately generic; if you surface the
"modelled on …" descriptions in a user interface, keep them descriptive
("in the style of …") rather than presenting the sounds as the vendors' own.

## Copyright position (not legal advice)

Copyright in a shipped click is a sound-recording right: it protects the
recorded samples from being copied. It does not stop an independent
synthesis that imitates the sound (in US law this is explicit, 17 U.S.C.
§ 114(b); the same principle applies in most jurisdictions). In addition,
most of these sounds are so simple — a decaying sine, an impulse through a
filter, a square wave shrinking by a fixed ratio, a one-millisecond ramp —
that they are unlikely to meet the originality threshold for copyright at
all. This repository contains only independently generated audio and the
code that generates it; it contains none of the vendors' files.
