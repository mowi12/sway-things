#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Checks that every file declares its copyright and license, following the REUSE specification (SPDX headers and
# REUSE.toml).
#
# Usage: scripts/check-licenses.sh
#
# Needs reuse (https://reuse.software/).

set -euo pipefail

cd "$(dirname "$0")/.."

if ! command -v reuse >/dev/null; then
  echo "error: needs reuse" >&2
  exit 1
fi

reuse lint
