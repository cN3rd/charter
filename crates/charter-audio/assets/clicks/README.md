# Metronome sounds

Nine synthesised click pairs, `accent.wav` for the first beat of a measure and `beat.wav` for the
rest: 44.1 kHz, 16-bit, mono. They are the rendered `out/<preset>/{accent,beat}.wav` files of the
metronome-clicks pack, unmodified. `click.rs` embeds them and resamples them to the song's rate.

The audio is CC0 1.0: see `LICENSE-AUDIO`, whose "everything under out/" means these files, and
the full legal text in `CC0-1.0.txt`. `PROVENANCE.md` says how they were made and carries the
trademark notice; keep any "modelled on" wording descriptive, as it asks. The pack's Python
scripts, the sounds' source form, are not vendored here.
