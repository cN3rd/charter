---
type: Constraint
title: Licensing
description: Copyleft licences that bound which audio and analysis code can be linked or copied.
tags: [constraints, licensing, legal]
status: draft
---

# Licensing

Characterisations below are as recorded by earlier research and have **not** been through legal
review.

| Component | Licence | Practical effect as recorded |
|---|---|---|
| aubio | GPL-3.0 | Would force a distributed binary to GPL. The `aubio-rs` bindings are MIT, which does not change the underlying library's terms |
| `bliss-audio` | GPL-3.0 | Excluded from a shipped path |
| `essentia` | AGPL or commercial | Excluded from a shipped path |
| `soundtouch` core | LGPL-2.1 | Dynamic linking described as the cleaner route |
| `symphonia` | MPL-2.0 | Weak copyleft applied per file; described as compatible with linking into a proprietary or permissive application |
| Tempora source | CC BY-NC-ND 4.0 | Cannot be copied or derived from at all |
| Etterna | MIT per repository metadata | Licensing of every dependency it pulls was not established |

charter itself is dual-licensed under MIT or Apache-2.0, at the user's choice, as is usual for Rust
crates: `LICENSE-MIT` and `LICENSE-APACHE` at the repository root, `MIT OR Apache-2.0`, which every
crate inherits through `license.workspace = true`. What it bundles keeps its own terms: Inter under the OFL
(`crates/charter-style/assets/fonts`) and the metronome sounds under CC0
(`crates/charter-audio/assets/clicks`). A dependency whose licence would bind the app beyond these
terms, as the copyleft entries above would, stays out of the shipped path.

Two consequences worth stating plainly:

- **ArrowVortex's detection quality cannot be matched by linking the same library on the same
  terms.** ArrowVortex depends on aubio; a permissively licensed product cannot. See
  [beat detection](../topics/beat-detection.md).
- **Tempora can be studied but not borrowed from.** Its observed behaviour is fair to learn from;
  its code is not available under any reuse-compatible terms.
