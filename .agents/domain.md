# Domain Docs

How the engineering skills should consume this repo's domain documentation when exploring the codebase.

charter is **single-context**: one glossary and one ADR set cover the whole workspace. The crates
under `crates/` and `plugins/` are layers of one product, not separate domains, so they share a
vocabulary rather than keeping per-crate glossaries.

The mdbook under `docs/` is charter's canonical documentation and internal knowledge base, so the
glossary and the ADRs live inside the book rather than in a parallel tree.

## Before exploring, read these

- **`docs/src/glossary.md`**: the project vocabulary.
- **`docs/src/adr/`**: read ADRs that touch the area you're about to work in.
- **`docs/src/research/topics/`**: the pre-existing decision write-ups. These predate the ADR
  directory and cover much of the same ground (note model, undo model, primary format, timing).
  Read the relevant topic before proposing a decision, and prefer extending a topic page over
  writing an ADR that restates it.

If any of these don't exist, **proceed silently**. Don't flag their absence; don't suggest creating
them upfront. The `/domain-modeling` skill (reached via `/grill-with-docs` and
`/improve-codebase-architecture`) creates them lazily when terms or decisions actually get resolved.

## File structure

```
docs/
├── book.toml
└── src/
    ├── SUMMARY.md
    ├── glossary.md
    ├── adr/
    │   ├── 0001-<slug>.md
    │   └── 0002-<slug>.md
    └── research/topics/      ← existing decision write-ups
```

Anything added under `docs/src/` must also get a line in `docs/src/SUMMARY.md`, or mdbook will not
build it into the book. A new ADR goes under an `ADRs` section; a new glossary goes at the top
level. This is the one extra step the book costs over a loose `docs/adr/` directory.

## Use the glossary's vocabulary

When your output names a domain concept (in an issue title, a refactor proposal, a hypothesis, a
test name), use the term as defined in `docs/src/glossary.md`. Don't drift to synonyms the glossary
explicitly avoids.

If the concept you need isn't in the glossary yet, that's a signal: either you're inventing language
the project doesn't use (reconsider) or there's a real gap (note it for `/domain-modeling`).

## Flag ADR conflicts

If your output contradicts an existing ADR, surface it explicitly rather than silently overriding:

> _Contradicts ADR-0007 (chart model stays format-agnostic), but worth reopening because…_
