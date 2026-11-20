#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Lints the GitHub Actions workflows with actionlint. If shellcheck is installed, actionlint also checks the shell code
# in `run:` steps.
#
# Check level: lint (see scripts/run-checks.sh)
#
# Usage: scripts/check-workflows.sh
#
# Needs actionlint (https://github.com/rhysd/actionlint); shellcheck is optional.

set -euo pipefail

# shellcheck source=scripts/lib.sh
source "$(dirname "$0")/lib.sh"
no_arguments "$@"

cd "$(dirname "$0")/.."

require actionlint

actionlint
