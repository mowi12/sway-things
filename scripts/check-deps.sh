#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Checks the dependencies with cargo-deny, configured in deny.toml: security advisories, licenses compatible with
# GPL-3.0-or-later, banned or duplicate crates, and crates.io as the only source.
#
# Usage: scripts/check-deps.sh
#
# Needs cargo-deny (https://github.com/EmbarkStudios/cargo-deny). Fetches the RustSec advisory database.

set -euo pipefail

cd "$(dirname "$0")/.."

if ! command -v cargo-deny >/dev/null; then
  echo "error: needs cargo-deny" >&2
  exit 1
fi

cargo deny --locked check
