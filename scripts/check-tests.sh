#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Runs all tests of the workspace (unit, integration and doc tests) with the current toolchain. CI runs it on the MSRV
# and on stable.
#
# Usage: scripts/check-tests.sh
#
# Needs cargo.

set -euo pipefail

cd "$(dirname "$0")/.."

cargo test --workspace --locked
