#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Lints the GitHub Actions workflows with actionlint. If shellcheck is installed, actionlint also checks the shell code
# in `run:` steps.
#
# Usage: scripts/check-workflows.sh
#
# Needs actionlint (https://github.com/rhysd/actionlint); shellcheck is optional.

set -euo pipefail

cd "$(dirname "$0")/.."

if ! command -v actionlint >/dev/null; then
  echo "error: needs actionlint" >&2
  exit 1
fi

actionlint
