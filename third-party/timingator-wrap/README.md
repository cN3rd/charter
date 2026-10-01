# timingator-wrap

A plain facade over [`timingator`](../timingator): a `Timer` loaded from the model's GGUF bytes on
the graphics card or the CPU, and `graphics_card` to ask whether there is one. It exists for build
speed: burn's kernels are generic and compile in the crate that names the backend, and this is
that crate, so a change to `charter-app` no longer recompiles them.

## Rules

- Nothing past this crate names a burn backend or depends on burn; add what callers need here,
  without generics.
- Kani skips it, as it links burn.
