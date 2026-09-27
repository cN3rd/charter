---
type: Design Note
title: Automatic timing
description: How charter estimates a song's tempo map from its audio, what each stage does, where it lives, and how well it does against human maps.
tags: [timing, audio, dsp, machine-learning, draft]
status: draft
---

# Automatic timing

Charter can draft a song's tempo map from its audio: where the beats fall, where the tempo
changes, and how long its bars are. It follows the pipeline a survey of beat tracking and chart
tooling recommends: a neural beat tracker for robustness, then classical signal processing for
the precision a chart needs, with the segmentation and fitting stages that no published system
supplies built on top.

## Stages

| Stage | What it does | Where |
|---|---|---|
| Beats | [Beat This!](https://github.com/CPJKU/beat_this) (Foscarin, Schlüter and Widmer, ISMIR 2024, MIT code and weights) turns a 50 fps log-mel spectrogram into a beat and a downbeat logit a frame; peaks above one half are the beats | `charter-beatthis-burn` |
| Onsets | Spectral flux peaks, each moved to the steepest rise of the envelope within 5 ms | `charter-audio` |
| Refinement | Each beat moves onto the nearest onset within 30 ms; a beat without one counts a quarter as much | `charter-autotiming` |
| Counting | Beats are counted against the song's median gap, so a stretch the tracker followed at half tempo stays on the song's grid, a missed beat counts two and an extra one is dropped | `charter-autotiming` |
| Segmentation | The partition of beat number against time into straight lines with the least squared error plus a cost for each line, a little more off a downbeat, at least eight beats a line; done twice, the second time without beats far off their line | `charter-autotiming` |
| Fitting | Weighted least squares a line; the tempo snaps to a whole, half or third BPM when the snapped grid stays within 5 ms of the fitted one across the line, and keeps three places otherwise; lines whose beats scatter wider than 20 ms are dropped and their neighbours' grid carries through | `charter-autotiming` |
| Meter and timing | Bars as long as the downbeats most often are apart; the lines become [sections](format.md#anchors), each an anchor starting a bar | `charter-autotiming`, `charter-core` |

The tracker is a reimplementation in burn, running on the CPU through the flex backend, and loads
the authors' released `final0.ckpt`. Parity with the reference is pinned by fixtures from
`crates/charter-beatthis-burn/fixtures/make.py`: a seeded tiny network's logits and a log-mel
spectrogram, embedded in the tests. On real music the full network finds the reference's beats
with logits within 4 × 10⁻⁴. Each 30-second chunk runs on a thread of its own, so a four-minute
song takes about 13 seconds.

## How well it does

Against two ranked osu! maps, scored as redline's benchmark does, the distance from each of the
map's beats to the estimate's nearest grid line, allowing half and double tempo:

| Song | Map | Estimate | Within 5 / 10 / 20 ms, bias removed |
|---|---|---|---|
| Camellia, *Fly to night, tonight* | one red line, 132 BPM | 132 BPM snapped over the whole body | 87% / 88% / 89% |
| School Food Punishment, *You May Crawl* | 98 red lines around 148 BPM, a live band | eight sections from 146.5 to 149.5 BPM | 37% / 64% / 83% |

Both estimates sit about 20 ms after the maps. The same offset is there between charter's onsets
and the maps' lines, so it comes from where the onset detector puts a hit, the steepest rise of
its envelope, against where a mapper does, its start.

## Known gaps

- The onset detector's convention puts grids about 20 ms late against human maps.
- Stops, count-ins and the ×2, ÷2 and off-beat alternatives are not estimated yet.
- The estimate is not yet offered in the editor.
