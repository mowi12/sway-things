#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Lints the Rust code with clippy and the workspace lints from Cargo.toml. Warnings fail the check.
#
# Check level: lint (see scripts/run-checks.sh)
#
# Usage: scripts/check-clippy.sh
#
# Needs cargo with clippy.

set -euo pipefail

# shellcheck source=scripts/lib.sh
source "$(dirname "$0")/lib.sh"
no_arguments "$@"

cd "$(dirname "$0")/.."

cargo clippy --workspace --all-targets --locked -- -D warnings
