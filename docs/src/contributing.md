---
type: Policy
title: Contributing
description: Human responsibility, licensing, AI disclosure and verification requirements for contributions.
tags: [contributing, ai, licensing]
status: stable
sources:
  - resource: https://docs.kernel.org/process/coding-assistants.html
    title: Linux kernel AI Coding Assistants
  - resource: https://docs.kernel.org/process/generated-content.html
    title: Linux kernel guidelines for tool-generated content
---

# Contributing

AI assistance is welcome. If you submit a change, you take responsibility for all of it,
including code, tests, documentation and commit messages produced by a tool. You must understand
the contribution and be able to explain and defend it during review.

These rules take Linux's [AI coding assistant guidance](https://docs.kernel.org/process/coding-assistants.html)
and [tool-generated content guidelines](https://docs.kernel.org/process/generated-content.html)
as their starting point, with Charter's own licensing and AI-disclosure requirements.

## Review and licensing

Review every AI-generated part before submitting it. Check its correctness, safety and fit with
the project. Passing tests or another model's approval does not replace that review.

First-party contributions must preserve Charter's `MIT OR Apache-2.0` licensing, including the
recipient's choice of either licence. Third-party material keeps its own terms, which must be
compatible with its use in Charter. Keep licence notices and SPDX identifiers accurate for the
material they describe. The [licensing chapter](research/constraints/licensing.md) records the
project's existing constraints.

An AI tool does not establish that you have the right to submit its output. If output appears
copied from elsewhere, or its licensing is unclear, resolve the question or leave it out.

## AI disclosure

AI-assisted commits must include an `Assisted-by` trailer for each AI tool used. This
includes generated or revised code, tests, documentation and commit messages, even if a human
edited the result afterwards. Name the model when known; do not guess:

```text
Assisted-by: <tool> (<model, if known>)
```

In the commit body or pull request, briefly explain what the AI did and how its output was
checked. If AI helped find a bug, say so as well. Summarize the assistance rather than publishing
private prompts or secrets.

Keep any required `Co-authored-by` credits too: they do not replace `Assisted-by`.
The human submitter remains responsible whether they or an agent runs the commit command.
DCO `Signed-off-by` trailers and cryptographic signatures are optional; neither replaces AI
disclosure, review or licence compliance.

## Agent commit workflow

When asked to prepare or create commits:

1. Read this policy, AGENTS.md and the
   [atomic commit instructions](https://github.com/cN3rd/charter/blob/main/.agents/skills/commit/SKILL.md),
   including their local pitfalls. Inspect the working tree and index, and record the baseline
   commit. Account for untracked files and preserve work outside the request.
2. Split the requested changes into single-concern commits, ordered by dependency. Stage named
   paths or hunks rather than the whole working tree. Each staged tree must stand on its own.
3. Run `just ci` against the isolated staged state. Keep the other changes safely preserved
   until that commit is finalized. If a check fails, restore the full tree before editing,
   restage and check again.
4. Prepare the commit message outside the checkout. Explain the change, the AI's role and the
   checks actually run. Add `Assisted-by` for every tool used, including reviewers and tools
   that only helped with the message; name models only when known. Preserve required
   `Co-authored-by` credits. Do not invent certifications or claim human review that has not
   happened.
5. If the user has requested commits, create the requested atomic commits. No separate DCO
   handoff or repeated approval for each commit is needed. Use their configured Git identity
   and signing setup; if signing fails, report the error rather than disabling it.
6. Verify each commit's tree, AI trailers and signature when signing is configured. Restore
   the remaining work and repeat. Report the resulting commits and any unfinished steps;
   pushing or publishing needs a separate request.

## Finding and fixing bugs with AI

1. Read [AGENTS.md](https://github.com/cN3rd/charter/blob/main/AGENTS.md) in full, the instructions
   applying to the affected files, and every document named in the request. Keyword matches are
   not a substitute for reading the relevant process.
2. Record the baseline commit ID and trace the affected behavior through its callers.
3. For a non-trivial suspected bug, attempt a reproducer or a failing regression test before
   treating it as confirmed. Stop pursuing a finding that turns out to be wrong. Label a finding
   that could not be reproduced as unverified.
4. Produce a fix, not just a diagnosis, unless the request is explicitly report-only. Fix the
   cause shared by the affected callers rather than patching one symptom.
5. Verify the fix with the reproducer and the applicable checks in AGENTS.md, with no new build
   warnings. Follow its pre-commit gate before committing. If a fix fails verification, revise
   it and check again; do not present it as working.
6. When a commit is requested, explain the problem, cause, solution and verification in its
   message, with the AI disclosure above. Include `Fixes: <commit> ("subject")` when the
   introducing commit has been identified; do not invent one.
7. Report what was verified and what could not be done, including missing reproduction, build
   or test results. Flag potential security implications for human review rather than treating
   an automated finding as a confirmed vulnerability.
8. Leave reporting and submission to the human. Identify the appropriate project reporting
   route, but do not autonomously open issues or pull requests, contact maintainers or publish
   reports. Keep suspected vulnerabilities out of public reports until a human has reviewed
   the disclosure route.
