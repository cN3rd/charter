# cubecl-macros

cubecl-macros 0.10.0 from crates.io, patched in through `[patch.crates-io]` in the root
`Cargo.toml` so release builds are [reproducible](../../docs/src/packaging.md#reproducible-packages).
Upstream's `#[cube]` macro emits a kernel's generic registrations in the order of a `HashMap`,
which changes from one compiler run to the next, and with it cubecl-core's metadata and every
binary that links burn. The one change, marked `charter:` in `src/parse/kernel.rs`, sorts them by
name.

## Rules

- Excluded from the workspace: the code is upstream's and keeps upstream's style and lints.
- Drop this copy once upstream iterates in a stable order, and when burn moves to another cubecl,
  update or drop it to match; `cargo tree -i cubecl-macros` shows which version is in use.
- Upstream's README is `UPSTREAM-README.md`; the licence is upstream's, MIT or Apache-2.0.
