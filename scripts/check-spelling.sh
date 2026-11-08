#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Checks the spelling of all files in American English with typos, configured in _typos.toml.
#
# Usage: scripts/check-spelling.sh
#
# Needs typos (https://github.com/crate-ci/typos).

set -euo pipefail

cd "$(dirname "$0")/.."

if ! command -v typos >/dev/null; then
  echo "error: needs typos" >&2
  exit 1
fi

typos
