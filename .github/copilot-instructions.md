# charter

A charting/editor app for rhythm games, serving several games rather than one. Comparable tools:
ArrowVortex, Moonscraper, Editor on Fire, the StepMania editor, the osu! editor. Nothing
game-specific belongs in the shared core.

Rust Cargo workspace, edition 2024, `resolver = "3"` (needs Rust 1.85+; developed on 1.96).

## Commands

```bash
cargo check --workspace --all-targets   # fast feedback loop
cargo test --workspace
cargo test -p charter-core format_chapter  # single test: -p <crate> <substring of test path>
cargo test -p charter-core -- --nocapture --exact json::tests::the_format_chapter_example_survives_a_round_trip
cargo clippy --workspace --all-targets
cargo fmt --all
cargo run -p charter-app                 # the editor; add `-- path/to/song.json` to open a song
cargo run -p charter-ui --example mock   # the static mock of the UI draft
just kani                                # Kani proofs of every crate but the two linking burn, which Kani cannot build; `cargo kani -p <crate>` for one; `cargo install --locked kani-verifier && cargo kani setup` once
git submodule update --init              # required once, see .github/skills/README.md
```

The `justfile` wraps these and adds packaging: `just` lists every recipe, `just ci` runs format,
lint, tests and the book, `just build-windows` cross-compiles for `x86_64-pc-windows-msvc` with
cargo-xwin, and `just dist` writes a Linux `.tar.gz` and a Windows `.zip` to `dist/`, each holding
the stripped binary, `LICENSE-MIT` and `LICENSE-APACHE`, the bundled assets' licences and a list of the linked crates'.

## Correctness

Lints do the enforcing. The root `Cargo.toml` carries `[workspace.lints]` and first-party crates
opt in with `[lints] workspace = true`. `unsafe_code` is forbidden there, so a genuine need for
`unsafe` goes into a `third-party/` wrapper crate with a written reason. Narrow an individual
lint with `#[allow(...)]` at the call site instead of weakening the workspace table. The root
`clippy.toml` lets test code unwrap and index.

Every source file with logic tests itself in a `#[cfg(test)] mod tests` at its end; there are no
`tests/` directories. Unit tests can reach private items, so a rule is tested where it lives.

Tests are sans-io: they build inputs in memory and feed bytes or values to the code under test,
never touching files, the network or the clock. The format tests embed the format chapter's
example with `include_str!`, which happens at compile time. Property tests use proptest with
`crate::proptest_config()`, a fixed seed and no regression files, so they repeat exactly and
stay sans-io.

Kani proofs sit in a `#[cfg(kani)] mod proofs` beside the tests. Keep each one small: CBMC
bit-blasts floating-point arithmetic, and a proof mixing division, multiplication and rounding
did not finish in 15 minutes.

- Newtype anything carrying a unit: ticks, beats, measures, seconds, samples, pixels, scroll
  offsets. Silently swapping two of them is a common bug class in charting tools. Make
  conversions explicit and mix-ups a compile error. The format's units live in
  `crates/charter-core/src/unit.rs`, each showing a rejected mix-up as a `compile_fail` doctest
  beside a passing twin one line apart, because stable rustdoc does not check which error a
  `compile_fail` block hits.
- Chart files are untrusted input. Format crates parse bytes into already-valid domain types;
  nothing downstream handles a `Chart` that might be malformed, and no parser panics on hostile
  input.
- Make illegal states unrepresentable (enums over bool pairs, `NonZero*` for counts that cannot
  be zero) before adding runtime checks.
- Release builds keep `overflow-checks = true`.
- Undo/redo and multi-select edits are where borrow-checker pressure will show up. Redesign the
  ownership there before reaching for `Rc<RefCell<_>>`.

## Documentation

Project documentation lives in an mdbook under `docs/`, built with `mdbook build docs` and
previewed with `mdbook serve docs`. Anything explaining design, formats or workflows goes in the
book. READMEs stay short and stay next to the thing they describe, pointing into the book rather
than duplicating it.

`docs/src/` is simultaneously an [OKF](https://github.com/GoogleCloudPlatform/open-knowledge-format)
bundle, so the same files serve human readers and agents. Every chapter carries OKF frontmatter
with at least a `type`; `docs/okf-frontmatter.py` is an mdbook preprocessor that removes it before
rendering, since mdbook would otherwise show it as a stray rule and heading. Check conformance
with:

```bash
.github/skills/okf-open-knowledge-format/scripts/validate.sh docs/src
python3 docs/okf-frontmatter.py selftest
```

`docs/src/SUMMARY.md` is the one file that must not have frontmatter: mdbook parses it as a
sidebar separator and part title, which corrupts the navigation. The validator reports one
expected `E1` for it. Do not silence that by adding frontmatter.

## Layout

| Path | Role |
|---|---|
| `crates/charter-audio` | DSP: a song's audio decoded through `symphonia-wrap` into stereo frames; no device |
| `crates/charter-autotiming` | DSP: a tempo map from a song's beats and onsets, as sections of constant tempo; the back end of automatic timing, fed by any tracker |
| `crates/charter-beatthis-burn` | DSP: beats and downbeats from audio by the Beat This! network in burn on the CPU, loading the released `final0.ckpt`; parity fixtures from `fixtures/make.py` |
| `crates/charter-core` | the chart format: data model, JSON I/O, validation, tick to time; `RulesetPlugin`, the surface a ruleset plugin implements |
| `crates/charter-editor` | editing a song without drawing it: open chart, cursor, snaps, note edits the ruleset plugin checks, selection, undo, unsaved changes, the demo song; `Song` lives in `song/`, one file per concern (layers, notes, selection, tempo, markers, history, cursor, settings) |
| `crates/charter-style` | the shared look: colour tokens, Inter and Phosphor fonts, and the widgets drawn in them |
| `crates/charter-timing` | the timing window in egui: the audio one measure a row, anchors pinning beats to the audio; hands its edits back as commands; `lib.rs` holds the state, `draw.rs`, `pointer.rs` and `keyboard.rs` its drawing and input |
| `crates/charter-ui` | the editor window in egui: notefield, panels, keys, settings; hosts the timing window; asks its host for files and dialogs; the static mock as an example. `window/`, `panels/` and `notefield/` are split one file per concern, each `impl Window` block beside the state sub-struct it owns |
| `crates/charter-app` | binary front end, thin: registers the built-in ruleset plugins, answers the window's file and dialog requests with `std::fs` and rfd, plays the song's audio through kira, keeps settings in eframe's storage; its file I/O goes through a `Disk` trait so it is tested in memory |
| `plugins/stepmania`, `plugins/ttr`, `plugins/osu-mania` | ruleset plugins: typed game models over `charter-core`, each with a `Plugin` the editor checks songs and edits through |
| `third-party/symphonia-wrap` | build-time facade over `symphonia`, read its README first |

All are workspace members listed in the root `Cargo.toml`. The ruleset plugin conventions are in
the book's ruleset chapter, `docs/src/rulesets.md`.

The workspace grows into small crates with one job each. Put a new crate in the kind it belongs
to and keep the dependency direction one-way:

- data: chart/note/timing types and file formats. No UI, no audio, no I/O beyond parsing.
- DSP: audio analysis and processing (waveforms, onset detection, pitch, time-stretch).
- UI: widgets and views. Depends on data and DSP; they never depend on it.
- built-in plugins: per-game support and optional features, written against the same extension
  surface a third-party plugin would use. A game quirk that needs a special case inside a data or
  UI crate means the extension surface is wrong; fix the surface.

## The symphonia build strategy

The `[profile.dev.package.*]` block in the root `Cargo.toml` and the `dylib` crate type on
`symphonia-wrap` both exist for build speed. Read `third-party/symphonia-wrap/README.md` before
changing audio dependencies, features or dev-profile settings.

## Tooling

Rust rules live in the `.github/skills/rust-skills` submodule, invoked with `/rust-skills`.
Production code is written test-first with `/test-driven-development`, and changes are split into
atomic commits with `/commit`. `.github/skills/README.md` lists every skill and its local rules.
`.github/lsp.json` registers `rust-analyzer` for agent code intelligence.

## Current state

`charter-core` implements the format chapter, and the three ruleset plugins model their games on top
of it without file import or export, and start empty songs. The editor starts songs from a dialog,
draws charts generically, moves the cursor by snap and by measure, and edits a song whose ruleset
plugin accepts it: lane keys and Ctrl with the mouse place taps, holds and mines, the mouse selects
and Delete removes the selection, every edit is checked by the format and the plugin, the tempo and
the section at the cursor can be changed, the marker list jumps the cursor, and undo and redo return
to where each edit was made. A song without its plugin is read-only. Songs open and save through
native dialogs, unsaved changes are asked about before they are lost and autosaved for restoring
after a crash, and a settings window keeps its choices between runs. `charter-editor` holds that
state without drawing it, `charter-ui` draws it in egui and never touches a file, and `charter-app`
registers the plugins, opens the window and does its file I/O. `charter-ui`'s `mock` example is the
static mock of the book's UI draft. `charter-audio` decodes the song's audio and `charter-app` plays
it through kira; Space plays from the cursor, and the playback rate changes the audio's speed and
pitch, never the chart. Ask before inventing a crate boundary.