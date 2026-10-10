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
writing any production code. Its examples are TypeScript; its `PITFALLS.md`, which a line atop
`SKILL.md` links, holds the Rust form of the red step and the mistakes it has caused here. After a
refresh, put that line back.

## commit

Vendored from [Lusinhas/pi-config](https://github.com/Lusinhas/pi-config) at commit `449dfe6`,
MIT, license retained in the skill directory. Upstream keeps it as `skills/git/commit/skill.md`;
refresh it by copying that file here as `SKILL.md` and updating the commit above.

Splits a working tree into single-concern, dependency-ordered commits, staging hunks
non-interactively with `git apply --cached`. Invoke with `/commit`. Charter's local rules for it,
and the mistakes it has caused here, are in its `PITFALLS.md`, which a line atop `SKILL.md`
links: running step 5 always, committing before `git stash pop`, scopes, and stash conflicts.
After a refresh, put that line back.
