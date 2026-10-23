# <img src="docs/src/guide/images/charter.svg" width="40" height="40" alt=""> Contributing to Charter

[![CI](https://img.shields.io/github/actions/workflow/status/cN3rd/charter/ci.yml?branch=main&style=for-the-badge&label=CI)](https://github.com/cN3rd/charter/actions/workflows/ci.yml)
[![MIT or Apache-2.0 licence](https://img.shields.io/badge/licence-MIT%20OR%20Apache--2.0-blue?style=for-the-badge)](README.md#licence)

You don't have to write Rust to help. Clear bug reports and corrections to the guide are useful
contributions too.

[README](README.md) |
[Issues](https://github.com/cN3rd/charter/issues) |
[Contribution policy](docs/src/contributing.md)

## Bugs and ideas

Check [existing issues](https://github.com/cN3rd/charter/issues) before opening a new one.
A useful bug report includes:

- The build identification line from **Help > About charter**, plus your operating system.
- Steps to reproduce, what you expected, and what happened instead.
- A small example file or screenshot, if it helps. Only include material you have permission to
  share, and remove private information first.

For feature ideas, describe what you're trying to chart and what gets in the way. Start with an
issue before a large change, so the workflow or design can be discussed before you write the code.

Keep suspected security problems out of public issues until the disclosure route has been
reviewed, as described in the [contribution policy](docs/src/contributing.md#finding-and-fixing-bugs-with-ai).

## Working on the code

Install [Rust](https://rustup.rs/) 1.95 or newer with `rustfmt` and `clippy`,
[just](https://github.com/casey/just), [mdbook](https://rust-lang.github.io/mdBook/), and Python 3.
The recipes use Bash. On Windows, use Git Bash and the MSVC Rust toolchain.
On Linux, also install clang, mold, pkg-config and the ALSA development headers
(`libasound2-dev` on Debian and Ubuntu).

Fork and clone the repository, and use a branch for your change. From the checkout:

```sh
git submodule update --init
just run
```

Use `just check` while editing. To run a focused test, for example:

```sh
just test -p charter-core format_chapter
```

Game-specific code belongs in `plugins/`; improve the plugin surface rather than adding a
game-specific special case to a core crate. Tests live beside the logic they exercise and use
in-memory inputs. [AGENTS.md](AGENTS.md) holds the shared coding rules and crate layout.

## Working on the guide

The book's sources are in [`docs/src/`](docs/src/). Use `just docs-serve` to preview changes.
If a change affects how someone uses Charter, update the matching
[user guide chapter](docs/src/guide/index.md) along with it.

## Before committing

```sh
just fmt
just ci
```

`just ci` checks formatting, Clippy, tests, and the book. If you couldn't run a
check, say which one and why.

Read the [contribution policy](docs/src/contributing.md) before submitting. You are responsible
for the entire change, including any AI-generated work, and for preserving Charter's
`MIT OR Apache-2.0` licensing.

AI-assisted commits need `Assisted-by` trailers naming each tool, with the model when known.
Briefly explain what the AI helped with and how you checked the result. `Co-authored-by` alone
does not replace that disclosure.

## Making a commit

Check the name and email Git will use, then commit the staged change:

```sh
git var GIT_COMMITTER_IDENT
git diff --cached
git commit
```

You can run this yourself or ask an agent to create the commits. You remain responsible for
what you submit. The [agent workflow](docs/src/contributing.md#agent-commit-workflow) covers
atomic staging, checks and AI attribution without a separate sign-off handoff.

DCO `Signed-off-by` trailers are optional. No certificate or signing key is needed to contribute.

### Optional GPG signing

A cryptographic signature identifies the key used to sign a commit. It does not replace AI
disclosure or your responsibility for the change. Charter does not require it.

If you want GPG-signed commits, create a key with `gpg --full-generate-key`, or use an existing
signing key. Keep the private key and its revocation certificate outside the checkout; enter
the passphrase only into GnuPG's prompt. List your keys with
`gpg --list-secret-keys --with-fingerprint` and use the chosen key's full fingerprint:

```sh
git config --local gpg.format openpgp
git config --local user.signingkey YOUR_FULL_FINGERPRINT
git config --local commit.gpgsign true
```

Replace `YOUR_FULL_FINGERPRINT` before running the command. These settings apply only to this
repository; future commits are GPG-signed automatically with ordinary `git commit`.

For GitHub's Verified badge, follow its
[GPG key setup guide](https://docs.github.com/en/authentication/managing-commit-signature-verification/generating-a-new-gpg-key)
and [add only the public key to your account](https://docs.github.com/en/authentication/managing-commit-signature-verification/adding-a-gpg-key-to-your-github-account).
The key needs an email matching your commit email and verified on GitHub.

## Opening a pull request

- Link the related issue, if there is one.
- Explain what changed and how you checked it.
- Include screenshots for visual changes.
- Keep unrelated fixes in separate pull requests.
