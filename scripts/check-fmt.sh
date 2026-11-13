#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Checks the formatting of Rust code (rustfmt, including the fuzz crate in fuzz/, a workspace of its own) and of
# Markdown, TOML, YAML and JSON (dprint, configured in dprint.json). With --fix, formats the files instead.
#
# Usage: scripts/check-fmt.sh [--fix]
#
# Needs cargo with rustfmt and dprint (https://dprint.dev/install/).

set -euo pipefail

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

for tool in cargo dprint; do
  if ! command -v "$tool" >/dev/null; then
    echo "error: needs $tool" >&2
    exit 1
  fi
done

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
