#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Checks that every file declares its copyright and license, following the REUSE specification (SPDX headers and
# REUSE.toml).
#
# Check level: lint (see scripts/run-checks.sh)
#
# Usage: scripts/check-licenses.sh
#
# Needs reuse (https://reuse.software/).

set -euo pipefail

# shellcheck source=scripts/lib.sh
source "$(dirname "$0")/lib.sh"
no_arguments "$@"

cd "$(dirname "$0")/.."

require reuse

reuse lint
