#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Checks the spelling of all files in American English with typos, configured in _typos.toml.
#
# Check level: lint (see scripts/run-checks.sh)
#
# Usage: scripts/check-spelling.sh
#
# Needs typos (https://github.com/crate-ci/typos).

set -euo pipefail

# shellcheck source=scripts/lib.sh
source "$(dirname "$0")/lib.sh"
no_arguments "$@"

cd "$(dirname "$0")/.."

require typos

typos
