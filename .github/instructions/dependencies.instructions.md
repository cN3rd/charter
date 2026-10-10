---
applyTo: "Cargo.toml,**/Cargo.toml,third-party/**"
---

Read `third-party/symphonia-wrap/README.md` before touching audio dependencies, their features or
the `[profile.dev.package.*]` tables: dev builds decode at usable speed only through the wrapper's
one shared, optimised symphonia tree.
