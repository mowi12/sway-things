#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Checks that the gettext template po/sway-things.pot holds exactly the strings marked with msg!("…"), as
# scripts/update-pot.sh extracts them. The creation date doesn't count.
#
# Check level: lint (see scripts/run-checks.sh)
#
# Usage: scripts/check-pot.sh
#
# Needs xgettext 0.24 or newer (GNU gettext), the first version that reads Rust, and diff (GNU diffutils).

set -euo pipefail

# shellcheck source=scripts/lib.sh
source "$(dirname "$0")/lib.sh"
no_arguments "$@"

cd "$(dirname "$0")/.."

require_hint diff "GNU diffutils"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

scripts/update-pot.sh "$work/sway-things.pot"

if ! diff -u -I '^"POT-Creation-Date: ' po/sway-things.pot "$work/sway-things.pot"; then
  echo "error: po/sway-things.pot is out of date; run scripts/update-pot.sh" >&2
  exit 1
fi
