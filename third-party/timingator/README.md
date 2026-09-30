# timingator

The library of timingator, a Rust port of the super timing generator of
[Mapperatorinator](https://github.com/OliBomby/Mapperatorinator): osu! timing points from a song's
audio, by the V32 model in burn. Vendored from the author's own `timingator/rust`; the `timingator`
CLI, the `timingator-prune` tool and its `prune` module, and the Python parity scripts stay
upstream. Licence and provenance are in `LICENSE` and `NOTICE`.

## Model

The weights are published pruned to what timing reads, in one GGUF file holding the tokenizer
too, at [cnf3rd/timingator-v32-f16-gguf](https://huggingface.co/cnf3rd/timingator-v32-f16-gguf):

```
https://huggingface.co/cnf3rd/timingator-v32-f16-gguf/resolve/main/timingator-v32-f16.gguf
```

415,618,240 bytes, sha256 `8554fca86c384d633442205a20f48b1072a1c842d82e2346bfc80e0811fd2792`.
`model::Model::load` takes its bytes, `tokenizer::Tokenizer::from_json` its
`timingator.tokenizer` metadata string (read with `gguf::Gguf::parse`), and `run` takes 16 kHz
mono samples normalized by `audio::normalize`.

## Rules

- Not a first-party crate: it does not opt into the workspace lints, and keeps upstream's code
  as is so it can be re-synced. Change it upstream and copy it back.
- `audio::decode` shells out to ffmpeg; charter decodes through `charter-audio` instead and
  hands `run` samples.
