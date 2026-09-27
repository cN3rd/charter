# charter's common commands. `just` lists them.

set shell := ["bash", "-euo", "pipefail", "-c"]

linux := "x86_64-unknown-linux-gnu"
windows := "x86_64-pc-windows-msvc"
# The C runtime built in, so Windows users need no Visual C++ redistributable.
windows_flags := "RUSTFLAGS='-C target-feature=+crt-static'"

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
    cargo clippy --workspace --all-targets

# Format everything
fmt:
    cargo fmt --all

# Fail if anything is not formatted
fmt-check:
    cargo fmt --all --check

# Run the Kani proofs of the crates that have them
kani:
    cargo kani -p charter-core
    cargo kani -p charter-ttr
    cargo kani -p charter-osu-mania

# Build the book into docs/book
docs:
    mdbook build docs

# Serve the book, rebuilding on change
docs-serve:
    mdbook serve docs

# Check the book is an OKF bundle (SUMMARY.md's one E1 is expected) and its preprocessor works
docs-check:
    python3 docs/okf-frontmatter.py selftest
    python3 docs/okf-frontmatter.py check docs/src

# Everything a change should pass before it is committed
ci: fmt-check clippy test docs docs-check

# Run the editor, passing any arguments on (a song to open)
run *args:
    cargo run -p charter-app -- {{ args }}

# Build the editor for this machine
build:
    cargo build -p charter-app

# Build the editor for this machine, optimised
release:
    cargo build -p charter-app --release

# Build the editor for 64-bit Windows, optimised, with cargo-xwin
build-windows:
    {{ windows_flags }} cargo xwin build -p charter-app --release --target {{ windows }}

# Run the Windows build under Wine
run-windows *args: build-windows
    wine target/{{ windows }}/release/charter-app.exe {{ args }}

# Build what `dist-linux` packages: release, without symbols
_dist-build-linux:
    cargo build -p charter-app --profile dist

# Build what `dist-windows` packages
_dist-build-windows:
    {{ windows_flags }} cargo xwin build -p charter-app --profile dist --target {{ windows }}

# Package the Linux build as dist/charter-<version>-linux-x86_64.tar.gz
dist-linux: _dist-build-linux (_stage "linux" "charter-app" "charter") (_licenses "linux")
    #!/usr/bin/env bash
    set -euo pipefail
    name="$(just _name linux-x86_64)"
    tar -C dist -czf "dist/$name.tar.gz" "$name"
    echo "dist/$name.tar.gz"

# Package the Windows build as dist/charter-<version>-windows-x86_64.zip
dist-windows: _dist-build-windows (_stage "windows" "charter-app.exe" "charter.exe") (_licenses "windows")
    #!/usr/bin/env bash
    set -euo pipefail
    name="$(just _name windows-x86_64)"
    rm -f "dist/$name.zip"
    (cd dist && zip -qr "$name.zip" "$name")
    echo "dist/$name.zip"

# Package both
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
        | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "charter-app"))')"
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
    cargo metadata --format-version 1 --filter-platform "$target" | python3 -c '
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
    for pkg in sorted((packages[i] for i in seen - ours), key=lambda p: p["name"]):
        pkg["license"] = pkg["license"] or "see the crate"
        print("| %(name)s | %(version)s | %(license)s |" % pkg)
    ' > "$folder/licenses/THIRD-PARTY.md"
