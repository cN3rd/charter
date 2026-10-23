# charter's common commands. `just` lists them.

set shell := ["bash", "-euo", "pipefail", "-c"]

# Windows installs Python as `python`, without a `python3`.
python := if os() == "windows" { "python" } else { "python3" }

# List the recipes
default:
    @just --list --unsorted

# Type-check everything, tests included
check:
    cargo check --workspace --all-targets

# Run every test, or those the arguments pick (`just test -p charter-core format_chapter`)
test *args:
    cargo test {{ if args == "" { "--workspace" } else { args } }}

# Lint everything
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Format everything
fmt:
    cargo fmt --all

# Fail if anything is not formatted
fmt-check:
    cargo fmt --all --check

# Run the Kani proofs of every crate Kani can build
kani:
    cargo kani --workspace --exclude charter-beatthis-burn --exclude charter-app --exclude charter-mapperatorinator-burn

# Build the book into docs/book
docs:
    mdbook build docs

# Serve the book, rebuilding on change
docs-serve:
    mdbook serve docs

# Use the platform's Python for mdbook's renderer probe and JSON input
_okf-frontmatter *args:
    @{{ python }} docs/okf-frontmatter.py {{ args }}

# Check the book is an OKF bundle (SUMMARY.md's one E1 is expected) and its preprocessor works
docs-check:
    {{ python }} docs/okf-frontmatter.py selftest
    {{ python }} docs/okf-frontmatter.py check docs/src

# Everything a change should pass before it is committed
ci: fmt-check clippy test docs docs-check

# Run the editor, passing any arguments on (a song to open)
run *args:
    cargo run -p charter-app -- {{ args }}

# Remove build output and packages
clean:
    cargo clean
    rm -rf dist docs/book

# Package this machine's build reproducibly into dist/ (docs/src/packaging.md)
dist:
    #!/usr/bin/env bash
    set -euo pipefail
    target="$(rustc -vV | sed -n 's/^host: //p')"
    exe={{ if os() == "windows" { ".exe" } else { "" } }}
    archive={{ if os() == "windows" { "zip" } else { "tar.gz" } }}

    # Remap the build folders out of Rust and C alike. rustc sees Windows paths where Git's bash
    # shows /c/..., and literal TOML strings keep their backslashes.
    native() { if command -v cygpath >/dev/null; then cygpath -w "$1"; else printf '%s' "$1"; fi; }
    here="$(native "$PWD")"
    cargo_home="$(native "${CARGO_HOME:-$HOME/.cargo}")"
    remap="target.$target.rustflags=['--remap-path-prefix=$here=.', '--remap-path-prefix=$cargo_home=/cargo']"
    export "CFLAGS_${target//-/_}=-ffile-prefix-map=$here=. -ffile-prefix-map=$cargo_home=/cargo"
    export CHARTER_COMMIT="$(git rev-parse --short=12 HEAD)"
    export SOURCE_DATE_EPOCH="$(git log -1 --format=%ct)"

    cargo build --locked -p charter-app --profile dist --config "$remap"

    # `path+file:///…/charter-app#0.1.0`, or `…#name@0.1.0` when the folder is named otherwise.
    id="$(cargo pkgid -p charter-app)"
    name="charter-${id##*[#@]}-{{ os() }}-{{ arch() }}"
    folder="dist/$name"
    rm -rf "$folder"
    mkdir -p "$folder/licenses"
    cp "target/dist/charter-app$exe" "$folder/charter$exe"

    {
        echo "commit $(git rev-parse HEAD)"
        [[ -z "${CHARTER_BUILD:-}" ]] || echo "build $CHARTER_BUILD"
        echo "target $target"
        rustc -V
        cargo -V
        echo "rebuild with: git checkout <commit>, rustup toolchain install <rustc version>," \
            "${CHARTER_BUILD:+CHARTER_BUILD=<build> }just dist"
    } > "$folder/BUILD.txt"

    cp LICENSE-MIT LICENSE-APACHE "$folder/"
    while read -r from to; do cp "$from" "$folder/licenses/$to"; done <<'END'
    crates/charter-style/assets/fonts/Inter-LICENSE.txt         Inter-OFL.txt
    crates/charter-audio/assets/clicks/LICENSE-AUDIO            metronome-sounds-LICENSE.txt
    crates/charter-audio/assets/clicks/CC0-1.0.txt              metronome-sounds-CC0-1.0.txt
    crates/charter-audio/assets/clicks/PROVENANCE.md            metronome-sounds-PROVENANCE.md
    crates/charter-audio/assets/hitsounds/LICENSE-AUDIO         hit-sounds-LICENSE.txt
    crates/charter-audio/assets/hitsounds/CC0-1.0.txt           hit-sounds-CC0-1.0.txt
    crates/charter-audio/assets/hitsounds/PROVENANCE.md         hit-sounds-PROVENANCE.md
    crates/charter-mapperatorinator-burn/LICENSE                mapperatorinator-LICENSE.txt
    crates/charter-mapperatorinator-burn/NOTICE                 mapperatorinator-NOTICE.txt
    END
    {
        echo "# Third-party crates in this build"
        echo
        echo "Each crate is used under the licence named; its text is in the crate on crates.io."
        echo
        echo "| Crate | Version | Licence |"
        echo "|---|---|---|"
        # Workspace crates print their folder, `(/…)` or `(C:\…)`; repeats end `(*)`.
        cargo tree --locked -q -e normal --target "$target" --prefix none --format '| {p} | {l} |' \
            | sed -E '/ \((\/|[A-Za-z]:)/d; s/ \((\*|proc-macro)\)//g; s/^\| (\S+) v(\S+) /| \1 | \2 /; s/\|  \|$/| see the crate |/' \
            | LC_ALL=C sort -u
    } > "$folder/licenses/THIRD-PARTY.md"

    cd dist
    if [[ {{ os() }} == windows ]]; then
        rm -f "$name.zip"
        7z a -tzip -mx=9 -mtm=off -mtc=off -mta=off -mcu=on "$name.zip" "$name" >/dev/null
    else
        tar --sort=name --mtime="@$SOURCE_DATE_EPOCH" --owner=0 --group=0 --numeric-owner \
            --mode=a=rX,u+w --format=gnu -c "$name" | gzip -9n > "$name.tar.gz"
    fi
    cd ..
    echo "dist/$name.$archive"
