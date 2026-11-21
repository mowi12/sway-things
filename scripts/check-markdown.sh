#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Lints all Markdown files with markdownlint-cli2, configured in .markdownlint-cli2.yaml. dprint owns the formatting.
#
# Check level: lint (see scripts/run-checks.sh)
#
# Usage: scripts/check-markdown.sh
#
# Needs markdownlint-cli2 (https://github.com/DavidAnson/markdownlint-cli2). `npm ci --prefix tools` installs the pinned
# version, which this script finds on its own.

set -euo pipefail

# shellcheck source=scripts/lib.sh
source "$(dirname "$0")/lib.sh"
no_arguments "$@"

cd "$(dirname "$0")/.."

PATH="$PWD/tools/node_modules/.bin:$PATH"
require_hint markdownlint-cli2 "npm ci --prefix tools"

# Tracked and new files, but none that git ignores.
mapfile -t files < <(repo_files '*.md')
markdownlint-cli2 "${files[@]}"
