---
applyTo: "crates/charter-ui/**,crates/charter-timing/**"
---

- `charter-ui`'s `window/`, `panels/` and `notefield/` hold one file per concern, each
  `impl Window` block beside the state it owns.
- `charter-timing` keeps its state in `lib.rs` and its I/O in `draw.rs`, `pointer.rs` and
  `keyboard.rs`.
- `cargo run -p charter-ui --example mock` draws the static mock of the book's UI draft.
