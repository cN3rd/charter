# charter-mapperatorinator-burn

Song timing from audio by [Mapperatorinator](https://github.com/OliBomby/Mapperatorinator)'s
super timing generator, ported to Rust: the V32 model in burn, on the graphics card through wgpu
or on the CPU through flex, its events fused into osu! timing points. `Timer` loads the model from
its GGUF file and times mono samples at `SAMPLE_RATE`; `graphics_card` asks whether wgpu has a
card to run it on. [Mapperatorinator port](../../docs/src/mapperatorinator.md) in the book
describes it and how it differs from upstream.

## Model

The weights are published pruned to what timing reads, in one GGUF file holding the tokenizer
too, at [cnf3rd/timingator-v32-f16-gguf](https://huggingface.co/cnf3rd/timingator-v32-f16-gguf):

```
https://huggingface.co/cnf3rd/timingator-v32-f16-gguf/resolve/main/timingator-v32-f16.gguf
```

415,618,240 bytes, sha256 `8554fca86c384d633442205a20f48b1072a1c842d82e2346bfc80e0811fd2792`.
`charter-app`'s finder downloads it into charter's data folder.

## Rules

- Nothing past this crate names a burn type or depends on burn; add what callers need here.
- Kani skips it, as it links burn.
- `audio::decode` shells out to ffmpeg; charter decodes through `charter-audio` instead and
  hands `Timer::run` samples.
- `LICENSE` is Mapperatorinator's MIT licence and `NOTICE` says what the port takes from where;
  `just dist` packages both.
