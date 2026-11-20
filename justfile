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
    scripts/run-checks.sh lint

# Run every check that is safe on this machine: lint, tests, install, packaging, website; offline, never starts sway
check:
    scripts/run-checks.sh host

# Run every check, like CI: also the ones that need the network or containers (sway runs only in containers)
check-all:
    scripts/run-checks.sh all

# List every check with its level
checks:
    scripts/run-checks.sh --list

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
