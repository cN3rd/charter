# charter

A multi-game rhythm-game charting editor (cf. ArrowVortex, Moonscraper, Editor on Fire, the
StepMania and osu! editors). **Nothing game-specific belongs outside `plugins/`**. A game quirk
needing a special case in a core crate means the plugin surface is wrong; fix the surface.

Rust Cargo workspace, edition 2024, MSRV 1.95.

## Zeroth Law

Never do these unless the user asks for that exact action. A higher rule wins over a lower one.

1. Rewrite history `origin/main` already has: force-push, or rebase or amend a pushed commit.
   The repository is public, and CI publishes every prerelease and the book from `main`.
2. Publish or delete outside the repository: `cargo publish`, uploads to
   `huggingface.co/cnf3rd`, or deleting a `ci-*` release or tag. Shipped builds pin the Hugging
   Face model's SHA-256, and CI's release notes start from the previous `ci-*` tag.
3. Commit model weights (`*.ckpt`, `*.gguf`, ignored for this reason). They download at runtime,
   a committed copy stays in every clone, and removing it breaks rule 1.

Ask before any action that can't be undone and isn't covered above.

## Commands

`just` lists every recipe; run `just setup` once after cloning. **`just ci`** is the pre-commit
gate: fmt-check, clippy with `-D warnings`, the tests, the book and its checks, and the packager's
self-test.

- `just check` is the fast loop; `just test -p charter-core format_chapter` runs a subset.
- `just run [song]` starts the editor; `just dist` writes reproducible packages into `dist/`.
- `just kani` runs the Kani proofs, minus the three crates linking burn, which Kani can't build.
  It needs `cargo install --locked kani-verifier && cargo kani setup` once.

## Correctness

Lints enforce, not review. Root `Cargo.toml` holds `[workspace.lints]` (clippy `pedantic`,
`restriction`, `nursery` all on; each allowed exception carries a written reason); first-party
crates opt in with `[lints] workspace = true`.

- `unsafe_code = "forbid"`. A genuine need goes in a `third-party/` wrapper crate with a reason.
- Narrow a lint at the call site with **`#[expect(.., reason = "..")]`**, never `#[allow]`, and
  never by weakening the workspace table.
- Root `clippy.toml` lets test code unwrap, index, panic and `dbg!`.

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
| `crates/charter-beatthis-burn` | DSP: beats and downbeats via the Beat This! network in burn on CPU |
| `crates/charter-mapperatorinator-burn` | DSP: osu! timing points via a port of Mapperatorinator's super timing, the V32 model in burn on wgpu, or flex on CPU; nothing past it depends on burn |
| `crates/charter-style` | shared look: colour tokens, Inter and Phosphor fonts, widgets |
| `crates/charter-timing` | the timing window in egui: audio a measure per row, anchors pinning beats to audio; returns edits as commands |
| `crates/charter-ui` | the editor window in egui: notefield, panels, keys, settings, minimap, recovery; hosts the timing window; asks its host for files and dialogs |
| `crates/charter-app` | thin binary: registers plugins, serves file/dialog requests (`std::fs`, rfd), plays audio (kira), time-stretch, settings in eframe storage, crash reports (`report.rs`, color-eyre). File I/O behind a `Disk` trait, so it is tested in memory |
| `plugins/{stepmania,ttr,osu-mania}` | ruleset plugins: typed game models over `charter-core` with a `Plugin` the editor validates through |
| `third-party/symphonia-wrap` | build-time facade over `symphonia`, `dylib` + `rlib` |
| `packaging/pack.py` | writes `.tar.gz`/`.zip` deterministically; `just pack-check` self-tests it |

Keep dependency direction one-way: **data → DSP → UI**, with plugins written against the same
public surface a third-party plugin would use. Ask before inventing a crate boundary.

## Documentation

mdbook under `docs/`, published to <https://cn3rd.github.io/charter/>. Design, formats and
workflows go in the book; READMEs stay short and point into it. Keep `docs/src/guide/` in step
with user-visible changes, and `docs/src/packaging.md` with packaging changes.

## Tooling

`.agents/skills/README.md` lists every skill and where it comes from: `/rust-skills` (a
submodule, fetched by `just setup`), `/test-driven-development` before writing production code,
`/commit` to split work into atomic commits. A skill's `PITFALLS.md` holds charter's local rules
for it; when a skill leads you wrong, append a trigger, wrong, right entry there.
`.github/lsp.json` registers rust-analyzer for code intelligence. Rules for one area sit in
`.github/instructions/` and load with the files they cover.

CI (`.github/workflows/ci.yml`) runs the `just ci` pieces, Kani, and packages both platforms on
push, publishing a `ci-<run>` prerelease and the book to GitHub Pages.
