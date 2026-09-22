# skills

## rust-skills

A git submodule (`leonardomso/rust-skills`, `master`). Run `git submodule update --init` once
after cloning, or the directory is empty.

It holds 265 Rust rules across 26 categories in `rules/`, each file prefixed by its category:
`own-`, `err-`, `mem-`, `unsafe-`, `api-`, `async-`, `anti-`, and so on. Invoke with
`/rust-skills`; check the relevant prefix before writing non-trivial ownership, error-handling,
async or `unsafe` code.

## okf-open-knowledge-format

Vendored from [fabricioctelles/skills](https://github.com/fabricioctelles/skills) at commit
`3b8da2c`, Apache-2.0, license retained in the skill directory. It is a copy rather than a
submodule because upstream ships many unrelated skills in one repo. Refresh it by copying
`skills/okf-open-knowledge-format/` from upstream again and updating the commit above.

Covers the Open Knowledge Format spec for markdown-plus-YAML knowledge bundles. Invoke with
`/okf-open-knowledge-format`.
