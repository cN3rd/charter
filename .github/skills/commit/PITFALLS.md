# Pitfalls

Append-only. Each entry: trigger, wrong behavior, right behavior. Charter's local rules for this
vendored skill and the mistakes it has caused here.

- Trigger: a staged state that obviously builds. Wrong: skipping step 5, so a commit passes only
  because of code still unstaged. Right: in charter step 5 always runs: `just clippy` and
  `just test` with the rest stashed.
- Trigger: step 5 passed. Wrong: `git stash pop` before `git commit`; resolving a conflicting pop
  stages whole files, so unrelated hunks slip into the commit. Right: commit while the rest is
  stashed, then pop.
- Trigger: a check fails while the rest is stashed. Wrong: fixing it there, `cargo fmt` included;
  the fix overlaps stashed hunks, the pop conflicts, and taking the stash's side reverts the fix.
  Right: pop, fix in the full tree, restage, and repeat step 5. Run `cargo fmt --all` before
  staging anything.
- Trigger: a file with staged and unstaged hunks on adjacent lines. Wrong: expecting a clean pop;
  it conflicts even though nothing changed while stashed, and the stash is kept. Right: with
  nothing edited while stashed, the stash holds the whole intended file:
  `git checkout --theirs <file>`, `git reset`, check `git diff`, then `git stash drop`.
- Trigger: picking a scope. Wrong: a topic such as `types`. Right: the crate's short name (`core`,
  `editor`, `ui`, `timing`, `app`, `audio`, `autotiming`, `beatthis`, `mapperatorinator`, `style`,
  `ttr`, `osu-mania`, `stepmania`), `deps` for dependency bumps, or none when the change spans
  crates.
- Trigger: a message body stating a count. Wrong: writing it from memory; "thirteen" had to be
  amended to "eleven". Right: count in the staged diff first, or leave the number out.
- Trigger: committing a subagent's work. Wrong: trusting each of its commits to hold one concern;
  one bundled an unrelated change. Right: read each with `git show --stat` and split it before
  pushing.
