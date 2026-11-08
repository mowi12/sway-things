#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Lints all Markdown files with markdownlint-cli2, configured in .markdownlint-cli2.yaml. dprint owns the formatting.
#
# Usage: scripts/check-markdown.sh
#
# Needs markdownlint-cli2 (https://github.com/DavidAnson/markdownlint-cli2). `npm ci --prefix tools` installs the pinned
# version, which this script finds on its own.

set -euo pipefail

cd "$(dirname "$0")/.."

PATH="$PWD/tools/node_modules/.bin:$PATH"
if ! command -v markdownlint-cli2 >/dev/null; then
  echo "error: needs markdownlint-cli2 (npm ci --prefix tools)" >&2
  exit 1
fi

# Tracked and new files, but none that git ignores.
mapfile -t files < <(git ls-files --cached --others --exclude-standard -- '*.md')
markdownlint-cli2 "${files[@]}"
