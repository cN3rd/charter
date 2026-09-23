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

## test-driven-development

Vendored from [obra/superpowers](https://github.com/obra/superpowers) at commit `5bf4e78`
(release v6.4.1), MIT, license retained in the skill directory. Copied for the same reason as the
OKF skill. Refresh it by copying `skills/test-driven-development/` from upstream again and
updating the commit above.

Red-green-refactor with a verified red step. Invoke with `/test-driven-development` before
writing any production code. Its examples are TypeScript; in Rust a test calling a function that
does not exist yet is a compile error, which the skill counts as an error rather than a failure.
Add the signature with a stub that returns a wrong value, so `cargo test` shows the assertion
failing, then implement.

## commit

Vendored from [Lusinhas/pi-config](https://github.com/Lusinhas/pi-config) at commit `449dfe6`,
MIT, license retained in the skill directory. Upstream keeps it as `skills/git/commit/skill.md`;
refresh it by copying that file here as `SKILL.md` and updating the commit above.

Splits a working tree into single-concern, dependency-ordered commits, staging hunks
non-interactively with `git apply --cached`. Invoke with `/commit`. Two rules tighten it here:

- Step 5 is mandatory. With the rest stashed, every staged state passes
  `cargo clippy --workspace --all-targets` and `cargo test --workspace` before it is committed.
- Commit before `git stash pop`, so a conflicting pop can never leak into the commit.
