#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Lints all shell scripts and the PKGBUILDs with shellcheck.
#
# Check level: lint (see scripts/run-checks.sh)
#
# Usage: scripts/check-shell.sh
#
# Needs shellcheck (https://www.shellcheck.net/).

set -euo pipefail

# shellcheck source=scripts/lib.sh
source "$(dirname "$0")/lib.sh"
no_arguments "$@"

cd "$(dirname "$0")/.."

require shellcheck

# Tracked and new files, but none that git ignores.
mapfile -t files < <(repo_files '*.sh')
# --external-sources follows the scripts that source scripts/lib.sh into it.
shellcheck --external-sources "${files[@]}"

# A PKGBUILD has no shebang and is sourced by makepkg, which reads its variables, sets pkgdir and srcdir, and runs each
# function with errexit, so a failed cd stops the build: SC2034 (unused variable), SC2154 (pkgdir not assigned) and
# SC2164 (cd without || exit) don't apply.
mapfile -t pkgbuilds < <(repo_files '*/PKGBUILD')
if ((${#pkgbuilds[@]})); then
  shellcheck --shell=bash --exclude=SC2034,SC2154,SC2164 "${pkgbuilds[@]}"
fi
