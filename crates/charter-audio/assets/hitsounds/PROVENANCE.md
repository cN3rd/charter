# Provenance

## What the sounds are

Four hit sounds, each **synthesised from scratch** by a short Python script of the osu-hitsounds
pack, using the same toolkit and approach as the metronome clicks beside them (`../clicks`). Every
WAV is the output of the pack's `make_all.py`, which regenerates it bit-for-bit from fixed noise
seeds.

## What they are modelled on

Each is modelled on one of osu!'s default hitsounds:

| file | modelled on | what it is |
|---|---|---|
| `normal.wav` | `drum-hitnormal` | noise through a narrow band around 2.1 kHz, decaying over about 34 ms, with a brighter burst and a click of highs at the start |
| `clap.wav` | `drum-hitclap` and `normal-hitclap` | a flam: one clap burst twice, 10 ms apart, with a 1.78 kHz resonance and a low thump |
| `soft_clap.wav` | `soft-hitclap` | the same clap with its bursts 16.7 ms apart |
| `finish.wav` | `softl-hitfinish` | a dull 400 to 800 Hz thump, then narrow-band noise around 1.0 to 1.3 kHz swelling in and decaying over about 38 ms |

## How the recipes were made

Reference samples were **analysed**: spectra over successive time windows, decay times, attack
times and levels. Each sound was then **written as a recipe** from generic building blocks
(shaped noise, narrow-band noise, loudness envelopes, filters) with the measured numbers as its
parameters, and **rendered anew**. No sample data from the reference files is in these WAVs; the
recipes describe the sounds with measured numbers, which is factual description.

The reference sounds themselves belong to ppy and their respective creators; osu! is their
trademark. "Modelled on" here is descriptive only.
