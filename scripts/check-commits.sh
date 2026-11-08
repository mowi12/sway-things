#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Checks that every commit since BASE is a Conventional Commit with a type from .conventional-commit-types.json:
# `type(optional-scope)!: description`. Merge commits are skipped.
#
# Usage: scripts/check-commits.sh [BASE]   (default: origin/main)
#
# Needs git and jq.

set -euo pipefail

if (($# > 1)); then
  echo "usage: $0 [BASE]" >&2
  exit 2
fi
base=${1:-origin/main}

cd "$(dirname "$0")/.."

if ! command -v jq >/dev/null; then
  echo "error: needs jq" >&2
  exit 1
fi
if ! git rev-parse --verify --quiet "$base^{commit}" >/dev/null; then
  echo "error: unknown base: $base" >&2
  exit 1
fi

types=$(jq -r '[.[].Type] | join("|")' .conventional-commit-types.json)
pattern="^($types)(\([a-z0-9-]+\))?!?: [^ ]"

count=0
errors=0
while read -r sha subject; do
  count=$((count + 1))
  if [[ ! $subject =~ $pattern ]]; then
    echo "error: $sha is not a Conventional Commit: $subject" >&2
    errors=$((errors + 1))
  fi
done < <(git log --no-merges --format='%h %s' "$base..HEAD")

if ((errors)); then
  echo "$errors of $count commit(s) don't follow the convention; see CONTRIBUTING.md#commit-messages." >&2
  exit 1
fi
echo "All $count commit(s) since $base follow the convention."
