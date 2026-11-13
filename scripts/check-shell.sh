#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Lints all shell scripts and the PKGBUILDs with shellcheck.
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

# A PKGBUILD has no shebang and is sourced by makepkg, which reads its variables, sets pkgdir and srcdir, and runs each
# function with errexit, so a failed cd stops the build: SC2034 (unused variable), SC2154 (pkgdir not assigned) and
# SC2164 (cd without || exit) don't apply.
mapfile -t pkgbuilds < <(git ls-files --cached --others --exclude-standard -- '*/PKGBUILD')
if ((${#pkgbuilds[@]})); then
  shellcheck --shell=bash --exclude=SC2034,SC2154,SC2164 "${pkgbuilds[@]}"
fi
