---
applyTo: "docs/**"
---

`docs/src/` is also an [OKF](https://github.com/GoogleCloudPlatform/open-knowledge-format)
bundle: every chapter carries frontmatter with at least a `type`, and `docs/okf-frontmatter.py` is
an mdbook preprocessor stripping it before render. `just docs-check` validates the bundle and
self-tests the preprocessor.

`docs/src/SUMMARY.md` must **not** have frontmatter, since mdbook parses it as a sidebar separator
and the navigation breaks. The validator reports one expected `E1` for it; do not silence it.
