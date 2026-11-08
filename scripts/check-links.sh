#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Checks the links in all Markdown files with lychee, configured in lychee.toml. By default only relative links and
# their anchors, which needs no network and is never flaky. With --external, external URLs too, printed as a Markdown
# report (used by the weekly workflow).
#
# Usage: scripts/check-links.sh [--external]
#
# Needs lychee (https://lychee.cli.rs/). For --external, set GITHUB_TOKEN to avoid GitHub's rate limits.

set -euo pipefail

external=false
case "${1-}" in
  "") ;;
  --external) external=true ;;
  *)
    echo "usage: $0 [--external]" >&2
    exit 2
    ;;
esac

cd "$(dirname "$0")/.."

if ! command -v lychee >/dev/null; then
  echo "error: needs lychee" >&2
  exit 1
fi

# Tracked and new files, but none that git ignores.
mapfile -t files < <(git ls-files --cached --others --exclude-standard -- '*.md')

if "$external"; then
  lychee --format markdown "${files[@]}"
else
  lychee --offline "${files[@]}"
fi
