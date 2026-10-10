# charter

A multi-game rhythm-game charting editor (cf. ArrowVortex, Moonscraper, Editor on Fire, the
StepMania and osu! editors). **Nothing game-specific belongs outside `plugins/`**. A game quirk
needing a special case in a core crate means the plugin surface is wrong; fix the surface.

Rust Cargo workspace, edition 2024, `resolver = "3"`, MSRV 1.95 (developed on 1.99).

## Commands

`just` is the entry point and lists every recipe. Run `just setup` once after cloning (skills
submodule, Windows target, cargo-xwin).

| Recipe | Runs |
|---|---|
| `just check` | `cargo check --workspace --all-targets`, the fast loop |
| `just test [args]` | `--workspace`, or the args (`just test -p charter-core format_chapter`) |
| `just clippy` | `cargo clippy --workspace --all-targets -- -D warnings` |
| `just fmt` / `fmt-check` | `cargo fmt --all [--check]` |
| `just docs` / `docs-serve` / `docs-check` | build / preview / validate the mdbook |
| `just pack-check` | `packaging/pack.py selftest`; archives are byte-reproducible |
| **`just ci`** | `fmt-check clippy test docs docs-check pack-check`, the pre-commit gate |
| `just run [song]` | the editor (`cargo run -p charter-app`) |
| `just kani` | Kani proofs; excludes the three crates linking burn, which Kani cannot build |
| `just dist` / `dist-linux` / `dist-windows` | reproducible packages into `dist/` |

`cargo run -p charter-ui --example mock` draws the static mock of the book's UI draft.
Kani needs `cargo install --locked kani-verifier && cargo kani setup` once.

Linux builds link with clang and mold and keep debug info out of the binary
(`.cargo/config.toml`); install both from your package manager. Rebuild after touching
`charter-app` is ~2s. Hot third-party crates are force-optimised in dev via
`[profile.dev.package.*]`.

## Correctness

Lints enforce, not review. Root `Cargo.toml` holds `[workspace.lints]` (clippy `pedantic`,
`restriction`, `nursery` all on; each allowed exception carries a written reason); first-party
crates opt in with `[lints] workspace = true`.

- `unsafe_code = "forbid"`. A genuine need goes in a `third-party/` wrapper crate with a reason.
- Narrow a lint at the call site with **`#[expect(.., reason = "..")]`**, never `#[allow]`, and
  never by weakening the workspace table. (277 `expect`s, 0 `allow`s.)
- Root `clippy.toml` lets test code unwrap, index, panic and `dbg!`.
- `overflow-checks = true` in release. `dist` profile adds thin LTO and strips symbols.

### Tests

Every file with logic tests itself in a trailing `#[cfg(test)] mod tests`. **No `tests/`
directories**: unit tests reach private items, so a rule is tested where it lives.

Tests are sans-io: inputs built in memory, never files, network or clock. Format tests
`include_str!` the book's format chapter example. Property tests use proptest with the crate's
`proptest_config()` (fixed seed, no regression files).

Kani proofs sit in `#[cfg(kani)] mod proofs` beside the tests. Keep each small. CBMC bit-blasts
floats, and one proof mixing division, multiplication and rounding never finished.

### Domain rules

- **Newtype anything with a unit**: ticks, beats, measures, seconds, samples, pixels, scroll
  offsets. Swapping two silently is the classic charting-tool bug. See
  `crates/charter-core/src/unit.rs`, where each rejected mix-up is a `compile_fail` doctest beside
  a passing twin one line apart (stable rustdoc doesn't check *which* error fires).
- **Chart files are untrusted.** Format crates parse bytes into already-valid domain types; no
  downstream code handles a possibly-malformed `Chart`, and no parser panics on hostile input.
- Make illegal states unrepresentable (enums over bool pairs, `NonZero*`) before adding runtime
  checks.
- Undo/redo and multi-select are where borrow-checker pressure shows. Redesign ownership before
  reaching for `Rc<RefCell<_>>`.
- Mark deliberate simplifications with a `ponytail:` comment naming the ceiling and the upgrade
  path. The convention runs through `Cargo.toml`, `clippy.toml`, the `justfile`, CI and the source.

## Layout

| Path | Role |
|---|---|
| `crates/charter-core` | the chart format: data model, JSON I/O, validation, tick↔time, identifiers, grid, sections; `RulesetPlugin`, the surface plugins implement |
| `crates/charter-editor` | editing a song without drawing it: cursor, snaps, plugin-checked note edits, selection, undo, unsaved changes, demo song. `song/` is one file per concern |
| `crates/charter-audio` | DSP: decode via `symphonia-wrap`, waveform peaks, onsets, spectrum, resampling, tags, metronome clicks. No device |
| `crates/charter-autotiming` | DSP: tempo map from beats and onsets, as constant-tempo sections; back end of automatic timing, fed by any tracker |
| `crates/charter-beatthis-burn` | DSP: beats and downbeats via the Beat This! network in burn on CPU (`final0.ckpt`); parity fixtures from `fixtures/make.py` |
| `crates/charter-mapperatorinator-burn` | DSP: osu! timing points via a port of Mapperatorinator's super timing, the V32 model in burn on wgpu, or flex on CPU; nothing past it depends on burn. Weights on Hugging Face, read its README |
| `crates/charter-style` | shared look: colour tokens, Inter and Phosphor fonts, widgets |
| `crates/charter-timing` | the timing window in egui: audio a measure per row, anchors pinning beats to audio; returns edits as commands. State in `lib.rs`, I/O in `draw/pointer/keyboard.rs` |
| `crates/charter-ui` | the editor window in egui: notefield, panels, keys, settings, minimap, recovery; hosts the timing window; asks its host for files and dialogs. `window/`, `panels/`, `notefield/` are one file per concern, each `impl Window` block beside the state it owns |
| `crates/charter-app` | thin binary: registers plugins, serves file/dialog requests (`std::fs`, rfd), plays audio (kira), time-stretch, settings in eframe storage, crash reports (`report.rs`, color-eyre). File I/O behind a `Disk` trait, so it is tested in memory |
| `plugins/{stepmania,ttr,osu-mania}` | ruleset plugins: typed game models over `charter-core` with a `Plugin` the editor validates through. `ttr` reads/writes `.ttr2_track` and EOF MIDI through one `exchange.rs` model; `osu-mania` imports osu! `[TimingPoints]`, pasted or from a `.osu` |
| `third-party/symphonia-wrap` | build-time facade over `symphonia`, `dylib` + `rlib`. **Read its README before touching audio deps, features or dev profiles** |
| `packaging/pack.py` | writes `.tar.gz`/`.zip` deterministically; `just pack-check` self-tests it |

Keep dependency direction one-way: **data → DSP → UI**, with plugins written against the same
public surface a third-party plugin would use. Ask before inventing a crate boundary.

## Documentation

mdbook under `docs/`, published to <https://cn3rd.github.io/charter/>. Design, formats and
workflows go in the book; READMEs stay short and point into it. Keep `docs/src/guide/` in step
with user-visible changes, and `docs/src/packaging.md` with packaging changes.

`docs/src/` is simultaneously an [OKF](https://github.com/GoogleCloudPlatform/open-knowledge-format)
bundle: every chapter carries frontmatter with at least a `type`, and `docs/okf-frontmatter.py` is
an mdbook preprocessor stripping it before render. `just docs-check` validates the bundle and
self-tests the preprocessor.

`docs/src/SUMMARY.md` must **not** have frontmatter, since mdbook parses it as a sidebar separator
and the navigation breaks. The validator reports one expected `E1` for it; do not silence it.

## Tooling

`.github/skills/README.md` lists every skill and its local rules: `/rust-skills` (a submodule,
fetched by `just setup`), `/test-driven-development` before writing production code, `/commit` to
split work into atomic commits. `.github/lsp.json` registers rust-analyzer for code intelligence.

CI (`.github/workflows/ci.yml`) runs the `just ci` pieces, Kani, and packages both platforms on
push, publishing a `ci-<run>` prerelease and the book to GitHub Pages.
