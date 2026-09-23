---
name: commit
description: "Splits the working tree into atomic, dependency-ordered commits matching the repo's message style. Use when asked to commit or to split mixed changes into reviewable commits."
disable-model-invocation: true
---

# Atomic commits

Turn an uncommitted working tree into a sequence of small commits where each commit has exactly one concern, builds on its predecessors, and reads like the rest of the repo's history. Never mix a refactor with a behavior change in one commit, even when they touch the same file.

## Workflow

1. Inventory the tree. Run all three; they answer different questions:

  ```sh
  git status --porcelain=v2
  git diff --stat
  git diff
  ```

  Also check untracked files (`git status` lists them; `git diff` does not). Scan the diff for secrets, debug prints, and stray files before staging anything.

2. Detect the message style from history, never invent one:

  ```sh
  git log --oneline -30
  git log -5 --format='%s%n%n%b---'
  ```

  Note: conventional-commit prefixes (`feat:`, `fix(scope):`), imperative vs past tense, subject capitalization, subject length, whether bodies exist, trailers like `Signed-off-by`. Also check for `commitlint.config.*`, `.gitmessage`, or a CONTRIBUTING section on commits. Match what you find exactly.

3. Plan the split. Cluster hunks by concern, not by file: rename-plus-behavior in one file is two commits. Order by dependency — a new helper lands before the commit that calls it; a schema change lands before code reading the new field. If the `todo` tool is available, record one todo per planned commit (message + files/hunks); otherwise keep the plan in your reply. If clustering is genuinely ambiguous (one diff plausibly serves two features), use the `ask` tool to confirm with the user; if it is unavailable, state your assumption explicitly and proceed with the safer split (more, smaller commits).

4. Stage per concern. Whole-file staging when a file belongs to one concern:

  ```sh
  git add src/parser.py tests/test_parser.py
  ```

  Hunk-level staging when a file mixes concerns — `git add -p` is interactive, so as an agent prefer patch surgery:

  ```sh
  git diff -- src/parser.py > /tmp/parser.patch
  # edit /tmp/parser.patch down to only the hunks for this commit
  git apply --cached /tmp/parser.patch
  git diff --cached   # confirm exactly what is staged
  ```

  When editing patches, keep hunk headers (`@@`) consistent with the lines you keep; if `git apply` rejects, use `git apply --cached --recount` or re-split with smaller context (`git diff -U1`).

5. Verify the staged state in isolation before committing, when a check is cheap (lint, typecheck, fast unit tests):

  ```sh
  git stash push --keep-index --include-untracked -m wip-rest
  npm run lint && npm test    # or the repo's equivalent
  git stash pop
  ```

  This tests exactly what the commit will contain. If the check fails because the commit depends on stashed code, your dependency order is wrong — restage. If `git stash pop` conflicts (overlapping hunks in the same region), the stash is kept: resolve the markers, then `git stash drop` — never commit with unmerged paths.

6. Commit with a message in the detected style:

  ```sh
  git commit -m 'fix(parser): handle empty heredoc bodies' \
    -m 'Empty bodies produced a zero-length token that crashed the lexer.'
  ```

  If a pre-commit hook rewrites files (formatters), `git status` after committing; re-stage and `git commit --amend --no-edit` only for hook-made changes to the just-created, unpushed commit.

7. Repeat until `git status` is clean (or only intentionally-uncommitted files remain), then show `git log --oneline -<count>` (e.g. `-5`) for the new commits.

## Edge cases

- Lockfiles travel with their manifest: `package.json` + `package-lock.json` in the same commit, never split.
- Renames: stage old and new paths together so `git commit` records a rename, and keep any content edits to the renamed file in a follow-up commit so the rename diff stays empty.
- Generated files: commit with the source that generates them, or not at all — follow repo precedent from `git log -- <generated-path>`.
- Binary changes get their own commit with a body explaining provenance.
- Whitespace-only or format-only hunks belong in a dedicated `style:`/format commit.
- Nothing to commit, or only untracked junk: say so; do not invent a commit.
- Never run `git add -A` or `git commit -a` in this workflow; both defeat the split.

## Done criteria

- [ ] Every commit has one concern; refactors and behavior changes are never mixed
- [ ] Commits are dependency-ordered (each commit would build on its own)
- [ ] Messages match the style observed in `git log`, not a generic template
- [ ] Cheap checks ran against each staged state where the repo makes that feasible
- [ ] No secrets, debug output, or unintended files were committed
- [ ] Final `git status` reviewed and final `git log --oneline` reported to the user
