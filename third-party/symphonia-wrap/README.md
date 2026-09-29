# symphonia-wrap

A one-line facade over [`symphonia`](https://crates.io/crates/symphonia): `pub use symphonia::*;`.
It exists for build speed and adds no API of its own.

## Build rationale

Audio decoding compiled at `opt-level = 0` is unusably slow, so the root `Cargo.toml` sets
`[profile.dev.package.<name>] opt-level = 3` for every `symphonia-*` package and for
`symphonia-wrap`. Dev builds of first-party code stay unoptimized and quick to compile while the
decoder tree is optimized.

The crate declares `crate-type = ["dylib", "rlib"]`, so the feature-selected symphonia tree links
into one shared object built once instead of being statically re-linked into every dependent.

## Rules

- Depend on `symphonia-wrap` from workspace crates, never on `symphonia` directly, so the feature
  set and the optimized artifact stay shared.
- Codec and container support is selected by the root `Cargo.toml` workspace dependency, which
  sets `default-features = false` and lists features explicitly: aac, alac, flac, mpa,
  vorbis, pcm, adpcm, isomp4, ogg, wav, aiff, caf, all-meta, opt-simd. Add formats
  there, not at the call site.
- A new third-party dependency that is hot at runtime gets its own
  `[profile.dev.package.*] opt-level = 3` entry in the root `Cargo.toml`.
