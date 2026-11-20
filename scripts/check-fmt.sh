#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Checks the formatting of Rust code (rustfmt, including the fuzz crate in fuzz/, a workspace of its own) and of
# Markdown, TOML, YAML, JSON, the JavaScript, CSS and Astro files of the website (dprint, configured in dprint.json).
# With --fix, formats the files instead.
#
# Check level: lint (see scripts/run-checks.sh)
#
# Usage: scripts/check-fmt.sh [--fix]
#
# Needs cargo with rustfmt and dprint (https://dprint.dev/install/).

set -euo pipefail

# shellcheck source=scripts/lib.sh
source "$(dirname "$0")/lib.sh"
help_option "$@"

fix=false
case "${1-}" in
  "") ;;
  --fix) fix=true ;;
  *)
    echo "usage: $0 [--fix]" >&2
    exit 2
    ;;
esac

cd "$(dirname "$0")/.."

require cargo dprint

if "$fix"; then
  cargo fmt --all
  cargo fmt --manifest-path fuzz/Cargo.toml
  dprint fmt
  exit
fi

status=0
cargo fmt --all --check || status=1
cargo fmt --manifest-path fuzz/Cargo.toml --check || status=1
dprint check || status=1
exit "$status"
