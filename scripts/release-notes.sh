#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Prints the release notes of TAG: the section "## X.Y.Z (…)" of CHANGELOG.md without its heading, the same text the
# changelog has. For a pre-release (vX.Y.Z-rc.N), which only tests the release workflow, a fixed note.
#
# Usage: scripts/release-notes.sh TAG
#
# Needs awk.

set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: scripts/release-notes.sh TAG" >&2
  exit 2
fi
tag=$1

cd "$(dirname "$0")/.."

if [[ $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+-rc\.[0-9]+$ ]]; then
  echo "A pre-release that tests the release workflow. It is no release; don't install it."
  exit
fi
if [[ ! $tag =~ ^v([0-9]+\.[0-9]+\.[0-9]+)$ ]]; then
  echo "error: $tag is no version tag" >&2
  exit 1
fi
version=${BASH_REMATCH[1]}

notes=$(awk -v heading="## $version (" '
  index($0, heading) == 1 { found = 1; next }
  found && /^## / { exit }
  found { print }
' CHANGELOG.md)
# Without the empty lines around the section.
notes=$(printf '%s\n' "$notes" | sed -e '/./,$!d' | tac | sed -e '/./,$!d' | tac)
if [[ -z $notes ]]; then
  echo "error: CHANGELOG.md has no section \"## $version (…)\"" >&2
  exit 1
fi
printf '%s\n' "$notes"
