# Hit sounds

Four synthesised hit sounds for the tick on each note: `normal.wav`, `clap.wav`, `soft_clap.wav`
and `finish.wav`, 44.1 kHz, 16-bit. They are the rendered `out/` files of the osu-hitsounds pack,
renamed from `drum-hitnormal`, `drum-hitclap`, `soft-hitclap` and `softl-hitfinish`, and otherwise
unmodified. The pack's `normal-hitclap.wav` is byte-for-byte `drum-hitclap.wav`, so it is left
out. `click.rs` embeds them and resamples them to the song's rate, and to a lane's pitch.

The audio is CC0 1.0: see `LICENSE-AUDIO` and the full legal text in `CC0-1.0.txt`.
`PROVENANCE.md` says how they were made. The pack's Python scripts, the sounds' source form, are
not vendored here.
