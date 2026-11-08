#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Lints the Rust code with clippy and the workspace lints from Cargo.toml. Warnings fail the check.
#
# Usage: scripts/check-clippy.sh
#
# Needs cargo with clippy.

set -euo pipefail

cd "$(dirname "$0")/.."

cargo clippy --workspace --all-targets --locked -- -D warnings
