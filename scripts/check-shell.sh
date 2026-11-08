#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Lints all shell scripts with shellcheck.
#
# Usage: scripts/check-shell.sh
#
# Needs shellcheck (https://www.shellcheck.net/).

set -euo pipefail

cd "$(dirname "$0")/.."

if ! command -v shellcheck >/dev/null; then
  echo "error: needs shellcheck" >&2
  exit 1
fi

# Tracked and new files, but none that git ignores.
mapfile -t files < <(git ls-files --cached --others --exclude-standard -- '*.sh')
shellcheck "${files[@]}"
