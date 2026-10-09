# timingator-wrap

A plain facade over [`timingator`](../timingator): a `Timer` loaded from the model's GGUF bytes on
the graphics card or the CPU, and `graphics_card` to ask whether there is one. It keeps burn out
of `charter-app`, which picks this crate's `Device` rather than burn's.

## Rules

- Nothing past this crate names a burn type or depends on burn; add what callers need here.
- Kani skips it, as it links burn.
