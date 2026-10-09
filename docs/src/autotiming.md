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
| Onsets | Spectral flux peaks, each moved back to where its hit starts: where a 5 ms trailing mean of the rectified audio first climbs a tenth of the way from the quiet before the peak to the peak, as a mapper marks a hit | `charter-audio` |
| Refinement | The tracker's beats sit a steady few tens of milliseconds from the onsets, so they are first moved by the song's median of that; then each beat moves onto the nearest onset within 30 ms, and a beat without one counts a quarter as much | `charter-autotiming` |
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

## In the editor

Timing › Find timing with Beat This!… finds the open chart's timing and puts it in place as one
step, saying what it found; Ctrl+Z takes it back. The first time, charter asks before it
downloads the tracker's weights, 81 MB from the authors' server, into its data folder, where
they stay; the song itself is never sent. The status bar shows the download and the search, which
run off the window's thread; the host does both, and the window only asks and answers.

## How well it does

Against two ranked osu! maps, scored as redline's benchmark does, the distance from each of the
map's beats to the estimate's nearest grid line, allowing half and double tempo:

| Song | Map | Estimate | Within 5 / 10 / 20 ms |
|---|---|---|---|
| Camellia, *Fly to night, tonight* | one red line, 132 BPM | 132 BPM snapped over the whole body | 87% / 87% / 89% |
| School Food Punishment, *You May Crawl* | 98 red lines around 148 BPM, a live band | nine sections from 144.5 to 149.5 BPM | 34% / 57% / 89% |

Both estimates sit about 4 ms after the maps. Onsets used to sit about 20 ms after the maps'
lines, at the steepest rise of each hit's envelope; they now sit at a hit's start, a median of
2 ms from the lines. Beat This!'s own beats run about 35 ms later than the onsets on both songs,
which is why the refinement learns the lag before it looks for each beat's onset.

## Mapperatorinator's super timing

Timing › Find timing with Mapperatorinator… is the alternative to Beat This!: a Rust port of
[Mapperatorinator](https://github.com/OliBomby/Mapperatorinator)'s super timing generator in
`charter-mapperatorinator-burn`. Its V32 model hears 16.4 seconds of audio at a time and writes
beat, measure and timing point events; twenty passes at shifted offsets are fused into one beat
grid and then osu! timing points. Its weights are one 416 MB GGUF file, which charter asks before
downloading into its data folder, as it does for Beat This!. [Mapperatorinator
port](mapperatorinator.md) describes the port and how it differs from upstream.

The model writes the timing points itself, so charter fits nothing to them: each becomes a
[section](format.md#anchors), as an imported osu! red line does. It runs on the graphics card
through burn's wgpu backend, since on the CPU a two-minute song took over fifteen minutes; on a
laptop RTX 3070 Ti it takes about 80 seconds, plus about 20 the first time while its kernels are
tuned. The status bar shows how many of its windows are decoded. When wgpu finds no graphics card,
or only a software renderer, charter says so and asks whether to run it on the CPU through burn's
flex backend instead. Super timing turns small differences in its input into different results: the same song
decoded by symphonia and resampled by rubato, as charter does, or by ffmpeg, as upstream's command
line does, can come out with different timing points.

## Known gaps

- A beatless or sparse intro can still come out as a stretch or two at a wrong tempo.
- Stops, count-ins and the ×2, ÷2 and off-beat alternatives are not estimated yet.
