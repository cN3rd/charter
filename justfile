# charter's common commands. `just` lists them.

set shell := ["bash", "-euo", "pipefail", "-c"]

linux := "x86_64-unknown-linux-gnu"
windows := "x86_64-pc-windows-msvc"
# The C runtime built in, so Windows users need no Visual C++ redistributable; /Brepro leaves the
# link time out of the executable, so a package rebuilds byte for byte.
windows_flags := "target." + windows + ".rustflags=['-C', 'target-feature=+crt-static', '-C', 'link-arg=/Brepro']"
# On Windows itself the MSVC toolchain builds for Windows; elsewhere cargo-xwin cross-compiles.
windows_build := if os() == "windows" { "cargo build" } else { "cargo xwin build" }
# Windows installs Python as `python`, without a `python3`.
python := if os() == "windows" { "python" } else { "python3" }

# List the recipes
default:
    @just --list --unsorted

# One-time setup: the skills submodule, the Windows target and cargo-xwin
setup:
    git submodule update --init
    rustup target add {{ windows }}
    command -v cargo-xwin >/dev/null || cargo install --locked cargo-xwin
    @echo "cargo xwin needs clang-cl and lld-link (clang and lld from your package manager)."

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
# ponytail: burn pulls in backtrace, which fails to compile under Kani's nightly (an ambiguous
# `unreachable!`), so the crates linking burn are left out. Drop the exclusions once it builds.
kani:
    cargo kani --workspace --exclude charter-beatthis-burn --exclude charter-app --exclude timingator --exclude timingator-wrap

# Build the book into docs/book
docs:
    mdbook build docs

# Serve the book, rebuilding on change
docs-serve:
    mdbook serve docs

# Check the book is an OKF bundle (SUMMARY.md's one E1 is expected) and its preprocessor works
docs-check:
    {{ python }} docs/okf-frontmatter.py selftest
    {{ python }} docs/okf-frontmatter.py check docs/src

# Check the packer writes the same archive from the same files
pack-check:
    {{ python }} packaging/pack.py selftest

# Everything a change should pass before it is committed
ci: fmt-check clippy test docs docs-check pack-check

# Run the editor, passing any arguments on (a song to open)
run *args:
    cargo run -p charter-app -- {{ args }}

# Build the editor for this machine
build:
    cargo build -p charter-app

# Build the editor for this machine, optimised
release:
    cargo build -p charter-app --release

# Build the editor for 64-bit Windows, optimised: with MSVC on Windows, cargo-xwin elsewhere
build-windows:
    {{ windows_build }} -p charter-app --release --target {{ windows }} --config "{{ windows_flags }}"

# Run the Windows build under Wine
run-windows *args: build-windows
    wine target/{{ windows }}/release/charter-app.exe {{ args }}

# Build what `dist-linux` packages: release, without symbols, reproducibly
_dist-build-linux:
    #!/usr/bin/env bash
    set -euo pipefail
    eval "$(just _reproducible {{ linux }})"
    cargo build --locked -p charter-app --profile dist --config "$remap"

# Build what `dist-windows` packages, reproducibly
_dist-build-windows:
    #!/usr/bin/env bash
    set -euo pipefail
    eval "$(just _reproducible {{ windows }})"
    {{ windows_build }} --locked -p charter-app --profile dist --target {{ windows }} \
        --config "{{ windows_flags }}" --config "$remap"

# Shell settings that make a package's build depend on its source alone: the folders it is built
# in remapped out of the binary, for Rust and C alike, the commit named for the About window, and
# the commit's time for the archives' files. docs/src/packaging.md says how to check a package.
_reproducible target:
    #!/usr/bin/env bash
    set -euo pipefail
    # rustc sees Windows paths where Git's bash shows /c/...
    native() { if command -v cygpath >/dev/null; then cygpath -w "$1"; else printf '%s' "$1"; fi; }
    here="$(native "$PWD")"
    cargo_home="$(native "${CARGO_HOME:-$HOME/.cargo}")"
    # Literal TOML strings, so a Windows path's backslashes stay as they are.
    echo "remap=\"target.{{ target }}.rustflags=['--remap-path-prefix=$here=.', '--remap-path-prefix=$cargo_home=/cargo']\""
    target_var="$(echo '{{ target }}' | tr - _)"
    echo "export CFLAGS_$target_var='-ffile-prefix-map=$here=. -ffile-prefix-map=$cargo_home=/cargo'"
    echo "export CHARTER_COMMIT=$(git rev-parse --short=12 HEAD)"
    echo "export SOURCE_DATE_EPOCH=$(git log -1 --format=%ct)"

# Package the Linux build as dist/charter-<version>-linux-x86_64.tar.gz
dist-linux: _dist-build-linux (_stage "linux" "charter-app" "charter") (_licenses "linux")
    #!/usr/bin/env bash
    set -euo pipefail
    eval "$(just _reproducible {{ linux }})"
    name="$(just _name linux-x86_64)"
    just _build-info {{ linux }} > "dist/$name/BUILD.txt"
    {{ python }} packaging/pack.py "dist/$name" "dist/$name.tar.gz"
    echo "dist/$name.tar.gz"

# Package the Windows build as dist/charter-<version>-windows-x86_64.zip
dist-windows: _dist-build-windows (_stage "windows" "charter-app.exe" "charter.exe") (_licenses "windows")
    #!/usr/bin/env bash
    set -euo pipefail
    eval "$(just _reproducible {{ windows }})"
    name="$(just _name windows-x86_64)"
    just _build-info {{ windows }} > "dist/$name/BUILD.txt"
    {{ python }} packaging/pack.py "dist/$name" "dist/$name.zip"
    echo "dist/$name.zip"

# What a package was built from, for rebuilding it: the commit and the toolchain
_build-info target:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "commit $(git rev-parse HEAD)"
    echo "target {{ target }}"
    rustc -V
    cargo -V
    echo "rebuild with: git checkout <commit>, rustup toolchain install <rustc version>, just dist"

# Package both; docs/src/packaging.md says what ships and why no assets folder is needed
dist: dist-linux dist-windows

# Remove build output and packages
clean:
    cargo clean
    rm -rf dist docs/book

# The folder name of a package for `platform`
_name platform:
    #!/usr/bin/env bash
    set -euo pipefail
    version="$(cargo metadata --no-deps --format-version 1 \
        | {{ python }} -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "charter-app"))')"
    echo "charter-$version-{{ platform }}"

# Copy the built binary into a fresh package folder, renamed
_stage os binary renamed:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ "{{ os }}" = windows ]; then
        built="target/{{ windows }}/dist/{{ binary }}"; platform=windows-x86_64
    else
        built="target/dist/{{ binary }}"; platform=linux-x86_64
    fi
    folder="dist/$(just _name "$platform")"
    rm -rf "$folder"
    mkdir -p "$folder/licenses"
    cp "$built" "$folder/{{ renamed }}"

# Put charter's licence, the bundled assets' and every linked crate's beside the binary
_licenses os:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ "{{ os }}" = windows ]; then
        target="{{ windows }}"; platform=windows-x86_64
    else
        target="{{ linux }}"; platform=linux-x86_64
    fi
    folder="dist/$(just _name "$platform")"
    cp LICENSE-MIT LICENSE-APACHE "$folder/"
    cp crates/charter-style/assets/fonts/Inter-LICENSE.txt "$folder/licenses/Inter-OFL.txt"
    clicks=crates/charter-audio/assets/clicks
    cp "$clicks/LICENSE-AUDIO" "$folder/licenses/metronome-sounds-LICENSE.txt"
    cp "$clicks/CC0-1.0.txt" "$folder/licenses/metronome-sounds-CC0-1.0.txt"
    cp "$clicks/PROVENANCE.md" "$folder/licenses/metronome-sounds-PROVENANCE.md"
    hits=crates/charter-audio/assets/hitsounds
    cp "$hits/LICENSE-AUDIO" "$folder/licenses/hit-sounds-LICENSE.txt"
    cp "$hits/CC0-1.0.txt" "$folder/licenses/hit-sounds-CC0-1.0.txt"
    cp "$hits/PROVENANCE.md" "$folder/licenses/hit-sounds-PROVENANCE.md"
    cp third-party/timingator/LICENSE "$folder/licenses/timingator-LICENSE.txt"
    cp third-party/timingator/NOTICE "$folder/licenses/timingator-NOTICE.txt"
    cargo metadata --format-version 1 --filter-platform "$target" | {{ python }} -c '
    import json, sys
    meta = json.load(sys.stdin)
    packages = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    ours = set(meta["workspace_members"])
    start = next(i for i in ours if packages[i]["name"] == "charter-app")
    # What ships: charter-app and what it links, not build scripts or tests.
    seen, todo = set(), [start]
    while todo:
        node = nodes[todo.pop()]
        for dep in node["deps"]:
            if any(kind["kind"] is None for kind in dep["dep_kinds"]) and dep["pkg"] not in seen:
                seen.add(dep["pkg"])
                todo.append(dep["pkg"])
    print("# Third-party crates in this build\n")
    print("Each crate is used under the licence named; its text is in the crate on crates.io.\n")
    print("| Crate | Version | Licence |\n|---|---|---|")
    for pkg in sorted((packages[i] for i in seen - ours), key=lambda p: (p["name"], p["version"])):
        pkg["license"] = pkg["license"] or "see the crate"
        print("| %(name)s | %(version)s | %(license)s |" % pkg)
    ' > "$folder/licenses/THIRD-PARTY.md"
