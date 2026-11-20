# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# List the recipes
[private]
default:
    @just --list --unsorted

# Start the app from source, with ARGS passed to it
[positional-arguments]
gui *args:
    cargo run --package sway-things-gui -- "$@"

# Run the command line from source, e.g. `just cli doctor`
[positional-arguments]
cli *args:
    cargo run --package sway-things-cli -- "$@"

# Run the tests of the workspace
test:
    scripts/check-tests.sh

# Run the fast offline checks: no tests, no sway, no network
lint:
    #!/usr/bin/env bash
    set -uo pipefail
    failed=()
    for check in fmt clippy spelling markdown shell licenses workflows metadata pot commits; do
        script=scripts/check-$check.sh
        echo "==> $script"
        "$script" || failed+=("$script")
    done
    if ((${#failed[@]})); then
        printf 'failed: %s\n' "${failed[@]}" >&2
        exit 1
    fi

# Run every check, like CI
check:
    #!/usr/bin/env bash
    set -uo pipefail
    failed=()
    for script in scripts/check-*.sh; do
        echo "==> $script"
        "$script" || failed+=("$script")
    done
    if ((${#failed[@]})); then
        printf 'failed: %s\n' "${failed[@]}" >&2
        exit 1
    fi

# Format Rust, Markdown, TOML, YAML and JSON files
fmt:
    scripts/check-fmt.sh --fix

# Serve the website with live reload (run `npm ci --prefix website` once first)
website:
    npm --prefix website run dev

# Build the binaries, the merged metadata and the translations for PREFIX (default /usr/local)
build *args:
    scripts/build.sh {{ args }}

# Install into DESTDIR/PREFIX (run `just build` first)
install:
    scripts/install.sh

# Remove what `just install` installed from DESTDIR/PREFIX
uninstall:
    scripts/install.sh --uninstall

# Fuzz the sway config parser for SECONDS (needs cargo-fuzz and a C++ compiler)
fuzz seconds="60":
    scripts/fuzz.sh run {{ seconds }}
