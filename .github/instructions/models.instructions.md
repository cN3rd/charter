---
applyTo: "crates/charter-beatthis-burn/**,crates/charter-mapperatorinator-burn/**"
---

- Weights stay out of the repo: `charter-app`'s finder downloads Beat This!'s `final0.ckpt` and
  Mapperatorinator's GGUF at runtime and checks each against a pinned SHA-256.
- Parity fixtures come from the reference implementations, through
  `charter-beatthis-burn/fixtures/make.py` and the scripts
  `charter-mapperatorinator-burn/fixtures/README.md` names. Regenerate them; don't edit them.
- `charter-mapperatorinator-burn/README.md` lists that crate's rules; read them first.
