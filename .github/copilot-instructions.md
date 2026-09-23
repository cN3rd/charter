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
cargo run -p charter-app
git submodule update --init              # required once, see .github/skills/README.md
```

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
example with `include_str!`, which happens at compile time.

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
| `crates/charter-core` | the chart format: data model, JSON I/O, validation, tick to time |
| `crates/charter-app` | binary front end, thin |
| `third-party/symphonia-wrap` | build-time facade over `symphonia`, read its README first |

All three are workspace members listed in the root `Cargo.toml`.

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

`charter-core` implements the format chapter. `charter-app` is a generated stub. No UI, DSP or
plugin crate exists yet. Ask before inventing a crate boundary.
